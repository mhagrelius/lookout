//! The application: owns the connection, the recorded history, and the poll.
//!
//! **Threading.** The DSM client is blocking, so a poll runs on a worker via
//! `gio::spawn_blocking` and its result is applied back on the main thread by
//! the surrounding `glib::spawn_future_local`. No async runtime, which is the
//! house rule; the connection lives behind a mutex because the worker needs it
//! and the main thread must not touch it while it does.
//!
//! **One poll at a time.** A tick that arrives while the previous one is still
//! out is dropped rather than queued. A NAS that has gone slow would otherwise
//! accumulate a backlog of requests that all arrive at once when it recovers.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lookout_core::dsm::{self, Capabilities, Client, Credentials, Error};
use lookout_core::poll::{self, Snapshot};
use lookout_core::{action, Config, ContainerAction, Range, Sample, Trends};

use crate::ui::connect;
use crate::ui::window::LookoutWindow;
use crate::APP_ID;

/// How long history is kept on disk between runs.
const TRENDS_FILE: &str = "trends.json";

/// How many consecutive failures before the window says it is disconnected.
///
/// One is a NAS asleep or a laptop changing networks; saying so immediately
/// would teach the user to ignore the indicator.
const FAILURES_BEFORE_DISCONNECTED: u32 = 3;

/// What a live connection consists of.
///
/// `pub` rather than private because the subclass's `imp` struct holds one in
/// a public field, which is how `glib::wrapper!` types are written; a narrower
/// visibility here is "more private than the item" and fails the lint.
pub struct Connection {
    client: Client,
    capabilities: Capabilities,
}

mod imp {
    use super::*;

    pub struct LookoutApplication {
        pub config: RefCell<Config>,
        pub connection: Arc<Mutex<Option<Connection>>>,
        pub trends: RefCell<Trends>,
        pub in_flight: Cell<bool>,
        /// The last good poll, so a range change can redraw without asking
        /// the DiskStation for data it does not keep.
        pub last_snapshot: RefCell<Option<Snapshot>>,
        pub failures: Cell<u32>,
        pub last_poll: Cell<Option<std::time::Instant>>,
        /// Counts timer fires, so an unfocused window can skip most of them.
        pub tick_count: Cell<u64>,
        pub tick: RefCell<Option<glib::SourceId>>,
    }

    impl Default for LookoutApplication {
        fn default() -> Self {
            LookoutApplication {
                config: RefCell::new(Config::load(&Config::default_path())),
                connection: Arc::new(Mutex::new(None)),
                trends: RefCell::new(Trends::load(&trends_path())),
                in_flight: Cell::new(false),
                last_snapshot: RefCell::new(None),
                failures: Cell::new(0),
                last_poll: Cell::new(None),
                tick_count: Cell::new(0),
                tick: RefCell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LookoutApplication {
        const NAME: &'static str = "LookoutApplication";
        type Type = super::LookoutApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for LookoutApplication {}

    impl ApplicationImpl for LookoutApplication {
        fn startup(&self) {
            self.parent_startup();
            if let Some(display) = gtk::gdk::Display::default() {
                crate::ui::load_stylesheet(&display);
            }
            self.obj().install_actions();
        }

        fn activate(&self) {
            let app = self.obj();
            let window = match app.active_window() {
                Some(window) => window.downcast::<LookoutWindow>().expect("our window type"),
                None => {
                    let window = LookoutWindow::new(&*app);
                    app.wire_window(&window);
                    window
                }
            };
            window.present();

            // A configured host still needs a password, so the dialog opens
            // either way on a cold start — but with everything else filled in.
            if app.imp().connection.lock().expect("not poisoned").is_none() {
                app.present_connect(false);
            }
        }

        fn shutdown(&self) {
            if let Some(id) = self.tick.borrow_mut().take() {
                id.remove();
            }
            // Saving here rather than on every sample: the store is a few
            // thousand points and writing it five times a minute is pointless
            // I/O on a laptop.
            let _ = self.trends.borrow().save(&trends_path());
            let _ = self.config.borrow().save(&Config::default_path());
            self.parent_shutdown();
        }
    }

    impl GtkApplicationImpl for LookoutApplication {}
    impl AdwApplicationImpl for LookoutApplication {}
}

glib::wrapper! {
    pub struct LookoutApplication(ObjectSubclass<imp::LookoutApplication>)
        @extends adw::Application, gtk::Application, gio::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl Default for LookoutApplication {
    fn default() -> Self {
        Self::new()
    }
}

impl LookoutApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("flags", gio::ApplicationFlags::empty())
            .build()
    }

    /// Connect the window's outgoing signals to the things that talk to DSM.
    fn wire_window(&self, window: &LookoutWindow) {
        window.set_range(self.imp().config.borrow().range);

        window.connect_range_changed({
            let app = self.clone();
            move |range| {
                // Persisted, because a range is a preference rather than a
                // mode: picking 7 d should still be 7 d tomorrow.
                app.imp().config.borrow_mut().range = range;
                app.redraw();
            }
        });

        window.connect_container_action({
            let app = self.clone();
            move |action, name| app.request_container_action(action, name)
        });
    }

    /// Redraw from the last snapshot without polling again.
    ///
    /// Changing the range only changes which recorded samples are read; going
    /// back to the DiskStation for that would be a round trip for data it
    /// never had.
    fn redraw(&self) {
        let imp = self.imp();
        let Some(window) = self.window() else { return };
        let Some(snapshot) = imp.last_snapshot.borrow().clone() else {
            return;
        };
        let range = imp.config.borrow().range;
        window.apply(&snapshot, &imp.trends.borrow(), range);
    }

    /// Confirm if the action interrupts something, then run it on a worker.
    fn request_container_action(&self, action: ContainerAction, name: String) {
        let Some(window) = self.window() else { return };

        let run = {
            let app = self.clone();
            let name = name.clone();
            move || app.run_container_action(action, name.clone())
        };

        if action.needs_confirmation() {
            crate::ui::container_page::confirm(&window, action, &name, run);
        } else {
            run();
        }
    }

    fn run_container_action(&self, what: ContainerAction, name: String) {
        let connection = self.imp().connection.clone();
        let app = self.clone();

        glib::spawn_future_local(async move {
            let label = name.clone();
            let result = gio::spawn_blocking(move || {
                let guard = connection.lock().expect("not poisoned");
                let Some(established) = guard.as_ref() else {
                    return Err(Error::Transport("not connected".into()));
                };
                action::container(&established.client, &established.capabilities, what, &name)
            })
            .await;

            if let Some(window) = app.window() {
                match result {
                    Ok(Ok(())) => {
                        window.toast(&format!("{label} {}", what.past_tense()));
                        // Poll straight away so the row's state catches up
                        // rather than waiting out the rest of the interval.
                        app.poll_once();
                    }
                    Ok(Err(e)) => window.toast(&e.to_string()),
                    Err(_) => window.toast("The action did not finish"),
                }
            }
        });
    }

    fn window(&self) -> Option<LookoutWindow> {
        self.active_window()?.downcast::<LookoutWindow>().ok()
    }

    fn install_actions(&self) {
        let connect = gio::SimpleAction::new("connect", None);
        connect.connect_activate({
            let app = self.clone();
            move |_, _| app.present_connect(false)
        });
        self.add_action(&connect);

        let refresh = gio::SimpleAction::new("refresh", None);
        refresh.connect_activate({
            let app = self.clone();
            move |_, _| app.poll_once()
        });
        self.add_action(&refresh);

        let about = gio::SimpleAction::new("about", None);
        about.connect_activate({
            let app = self.clone();
            move |_, _| app.present_about()
        });
        self.add_action(&about);

        self.set_accels_for_action("app.refresh", &["<Control>r", "F5"]);
    }

    fn present_about(&self) {
        let Some(window) = self.window() else { return };
        let about = adw::AboutDialog::new();
        about.set_application_name("Lookout");
        about.set_application_icon(APP_ID);
        about.set_version(env!("CARGO_PKG_VERSION"));
        about.set_developer_name("Matthew Hagrelius");
        about.set_license_type(gtk::License::Gpl30);
        about.set_comments("A monitor for a Synology DiskStation.");
        about.present(Some(&window));
    }

    fn present_connect(&self, needs_otp: bool) {
        let Some(window) = self.window() else { return };
        let config = self.imp().config.borrow().clone();
        connect::present(&window, &config, needs_otp, {
            let app = self.clone();
            move |answer| app.attempt_login(answer)
        });
    }

    /// Log in on a worker, then start polling.
    fn attempt_login(&self, answer: connect::Answer) {
        let config = answer.config.clone();
        self.imp().config.replace(config.clone());

        let creds = Credentials {
            account: config.account.clone(),
            password: answer.password,
            otp_code: answer.otp_code,
            device_id: config.device_id.clone(),
            device_name: "lookout".into(),
        };

        let connection = self.imp().connection.clone();
        let host = config.host();
        let app = self.clone();

        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let mut client = Client::new(host);
                // Discovery first, and deliberately before the login: it needs
                // no session, so if it fails the problem is the address or the
                // certificate, which is a much better thing to tell someone
                // than "login failed".
                let capabilities = dsm::discover(&client)?;
                client.login(&creds)?;
                Ok::<_, Error>(Connection {
                    client,
                    capabilities,
                })
            })
            .await;

            match result {
                Ok(Ok(established)) => {
                    // A login that carried a code gets a device token back;
                    // keeping it is what makes this the last time a code is
                    // needed on this machine.
                    let device_id = established
                        .client
                        .session()
                        .and_then(|s| s.device_id.clone());
                    {
                        let mut config = app.imp().config.borrow_mut();
                        if device_id.is_some() {
                            config.device_id = device_id;
                        }
                        let _ = config.save(&Config::default_path());
                    }

                    *connection.lock().expect("not poisoned") = Some(established);
                    app.imp().failures.set(0);
                    if let Some(window) = app.window() {
                        window.toast("Connected");
                    }
                    app.start_polling();
                    app.poll_once();
                }
                Ok(Err(e)) => app.report_login_failure(e),
                Err(_) => {
                    if let Some(window) = app.window() {
                        window.show_disconnected("The login task did not finish.");
                    }
                }
            }
        });
    }

    fn report_login_failure(&self, error: Error) {
        // A DiskStation that wants a second factor is not a failure, it is the
        // next step: reopen the dialog with the code field showing.
        if let Error::Dsm(dsm_error) = &error {
            if dsm_error.needs_otp() {
                if let Some(window) = self.window() {
                    window.toast("Enter the code from your authenticator");
                }
                self.present_connect(true);
                return;
            }
        }

        if let Some(window) = self.window() {
            window.show_disconnected(&error.to_string());
            window.toast(&error.to_string());
        }
    }

    fn start_polling(&self) {
        let imp = self.imp();
        if let Some(id) = imp.tick.borrow_mut().take() {
            id.remove();
        }

        let interval = Duration::from_secs(imp.config.borrow().poll_interval);
        let id = glib::timeout_add_local(interval, {
            let app = self.clone();
            move || {
                let tick = app.imp().tick_count.get();
                app.imp().tick_count.set(tick.wrapping_add(1));

                // A window nobody is looking at polls a sixth as often. The
                // age indicator keeps counting either way, so the pill is
                // honest about how stale the page is.
                let focused = app.window().is_some_and(|w| w.is_active());
                if poll::should_poll(focused, tick) {
                    app.poll_once();
                }
                app.refresh_poll_age();
                glib::ControlFlow::Continue
            }
        });
        imp.tick.replace(Some(id));
    }

    fn refresh_poll_age(&self) {
        let Some(window) = self.window() else { return };
        if let Some(last) = self.imp().last_poll.get() {
            window.set_poll_age(last.elapsed().as_secs());
        }
    }

    /// Run one refresh, unless one is already out.
    fn poll_once(&self) {
        let imp = self.imp();
        if imp.in_flight.get() {
            return;
        }
        if imp.connection.lock().expect("not poisoned").is_none() {
            return;
        }
        imp.in_flight.set(true);

        let connection = imp.connection.clone();
        let app = self.clone();

        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let guard = connection.lock().expect("not poisoned");
                let Some(established) = guard.as_ref() else {
                    return Err(Error::Transport("not connected".into()));
                };
                poll::overview(&established.client, &established.capabilities)
            })
            .await;

            app.imp().in_flight.set(false);

            match result {
                Ok(Ok(snapshot)) => app.apply_snapshot(snapshot),
                Ok(Err(e)) => app.record_failure(e),
                Err(_) => {
                    app.record_failure(Error::Transport("the poll task did not finish".into()))
                }
            }
        });
    }

    fn apply_snapshot(&self, snapshot: Snapshot) {
        let imp = self.imp();
        imp.failures.set(0);
        imp.last_poll.set(Some(std::time::Instant::now()));

        // Record before rendering, so the chart drawn this tick includes the
        // point this tick produced.
        if let Some(utilization) = &snapshot.utilization {
            let temperature = snapshot.system.as_ref().and_then(|s| s.temperature_c);
            imp.trends.borrow_mut().record(Sample::new(
                chrono::Utc::now(),
                utilization,
                temperature,
            ));
        }

        let range = imp.config.borrow().range;
        if let Some(window) = self.window() {
            window.apply(&snapshot, &imp.trends.borrow(), range);
            window.set_poll_age(0);
        }
        imp.last_snapshot.replace(Some(snapshot));
    }

    fn record_failure(&self, error: Error) {
        let imp = self.imp();

        if error.needs_login() {
            // The session expired. Ask again rather than retrying forever
            // against a sid DSM has already thrown away.
            *imp.connection.lock().expect("not poisoned") = None;
            if let Some(window) = self.window() {
                window.show_disconnected("The DiskStation session expired.");
            }
            self.present_connect(false);
            return;
        }

        let failures = imp.failures.get() + 1;
        imp.failures.set(failures);
        if failures >= FAILURES_BEFORE_DISCONNECTED {
            if let Some(window) = self.window() {
                window.show_disconnected(&error.to_string());
            }
        }
    }
}

/// Where recorded history is kept.
fn trends_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    base.join("lookout").join(TRENDS_FILE)
}

/// The ranges the UI can show, in the order the toggle group lists them.
pub fn ranges() -> [Range; 4] {
    Range::ALL
}
