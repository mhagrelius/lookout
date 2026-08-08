//! The window: a header bar, a navigation view, and the Overview inside it.
//!
//! Detail pages push onto the `AdwNavigationView`, which supplies the back
//! button. The header carries the refresh button and the "polled N seconds
//! ago" indicator the design asks for.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use std::cell::RefCell;

use lookout_core::poll::Snapshot;
use lookout_core::{Range, Trends};

use crate::ui::container_page::ContainerPage;
use crate::ui::detail_pages;
use crate::ui::log_page::LogPageView;
use crate::ui::resource_page::ResourcePage;
use crate::ui::storage_page::StoragePage;
use crate::ui::table_page::TablePage;
use crate::ui::Overview;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct LookoutWindow {
        pub overview: RefCell<Option<Overview>>,
        pub storage_page: RefCell<Option<StoragePage>>,
        pub resource_page: RefCell<Option<ResourcePage>>,
        pub container_page: RefCell<Option<ContainerPage>>,
        pub log_page: RefCell<Option<LogPageView>>,
        /// The four generic table pages, in the order they are filled.
        pub table_pages: RefCell<Vec<TablePage>>,
        pub navigation: RefCell<Option<adw::NavigationView>>,
        pub subtitle: RefCell<Option<adw::WindowTitle>>,
        pub poll_pill: RefCell<Option<gtk::Label>>,
        pub toasts: RefCell<Option<adw::ToastOverlay>>,
        pub status: RefCell<Option<adw::StatusPage>>,
        pub stack: RefCell<Option<gtk::Stack>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LookoutWindow {
        const NAME: &'static str = "LookoutWindow";
        type Type = super::LookoutWindow;
        type ParentType = adw::ApplicationWindow;
    }

    impl ObjectImpl for LookoutWindow {}
    impl WidgetImpl for LookoutWindow {}
    impl WindowImpl for LookoutWindow {}
    impl ApplicationWindowImpl for LookoutWindow {}
    impl AdwApplicationWindowImpl for LookoutWindow {}
}

glib::wrapper! {
    pub struct LookoutWindow(ObjectSubclass<imp::LookoutWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Native,
                    gtk::Root, gtk::ShortcutManager, gio::ActionGroup, gio::ActionMap;
}

impl LookoutWindow {
    pub fn new(app: &impl IsA<gtk::Application>) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        window.build();
        window.install_actions();
        window
    }

    fn build(&self) {
        self.set_title(Some("Lookout"));
        self.set_default_size(1180, 820);

        let title = adw::WindowTitle::new("Lookout", "Not connected");

        let refresh = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh.set_tooltip_text(Some("Refresh now"));
        refresh.set_action_name(Some("app.refresh"));

        let menu = gio::Menu::new();
        let pages = gio::Menu::new();
        pages.append(Some("System information"), Some("win.show::system"));
        pages.append(Some("Packages"), Some("win.show::packages"));
        pages.append(Some("Shared folders"), Some("win.show::shares"));
        pages.append(Some("Users & sessions"), Some("win.show::sessions"));
        pages.append(Some("Network"), Some("win.show::network"));
        pages.append(Some("Temperature & power"), Some("win.show::power"));
        menu.append_section(None, &pages);
        menu.append(Some("Connect…"), Some("app.connect"));
        menu.append(Some("About Lookout"), Some("app.about"));
        let menu_button = gtk::MenuButton::new();
        menu_button.set_icon_name("open-menu-symbolic");
        menu_button.set_menu_model(Some(&menu));

        let poll_pill = gtk::Label::new(Some("—"));
        poll_pill.add_css_class("caption");
        poll_pill.add_css_class("poll-pill");
        poll_pill.add_css_class("dim-label");

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        header.pack_start(&refresh);
        header.pack_end(&menu_button);
        header.pack_end(&poll_pill);

        // Two states share the content area: the page, and an AdwStatusPage
        // for "not connected" or "host unreachable". A stack rather than
        // swapping children so neither has to be rebuilt to come back.
        let overview = Overview::new();
        let status = adw::StatusPage::new();
        status.set_icon_name(Some("network-server-symbolic"));
        status.set_title("Not connected");
        status.set_description(Some("Choose a DiskStation to monitor."));
        let connect_button = gtk::Button::with_label("Connect…");
        connect_button.add_css_class("suggested-action");
        connect_button.add_css_class("pill");
        connect_button.set_halign(gtk::Align::Center);
        connect_button.set_action_name(Some("app.connect"));
        status.set_child(Some(&connect_button));

        let stack = gtk::Stack::new();
        stack.add_named(&status, Some("status"));
        stack.add_named(&overview.widget, Some("overview"));
        stack.set_visible_child_name("status");

        let page = adw::NavigationPage::new(&stack, "Overview");
        let navigation = adw::NavigationView::new();
        navigation.add(&page);

        // The drill-in. The detail page is built once and kept: pushing a
        // freshly-built one each time would drop the user's column widths and
        // scroll position every visit.
        let storage_page = StoragePage::new();
        overview.connect_open_storage({
            let navigation = navigation.clone();
            let detail = storage_page.page.clone();
            move || navigation.push(&detail)
        });

        let resource_page = ResourcePage::new();
        overview.connect_open_resources({
            let navigation = navigation.clone();
            let detail = resource_page.page.clone();
            move || navigation.push(&detail)
        });

        let container_page = ContainerPage::new();
        overview.connect_open_containers({
            let navigation = navigation.clone();
            let detail = container_page.page.clone();
            move || navigation.push(&detail)
        });

        let log_page = LogPageView::new();
        overview.connect_open_logs({
            let navigation = navigation.clone();
            let detail = log_page.page.clone();
            move || navigation.push(&detail)
        });

        // The generic pages. They have no section on the Overview, so they
        // are reached from the primary menu — which is also where DSM's own
        // Control Panel keeps their equivalents.
        let table_pages = vec![
            detail_pages::system_page(),
            detail_pages::packages_page(),
            detail_pages::shares_page(),
            detail_pages::sessions_page(),
            detail_pages::network_page(),
            detail_pages::power_page(),
        ];
        for table in &table_pages {
            navigation.add(&table.page);
        }

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&navigation));

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&toolbar));

        self.set_content(Some(&toasts));

        let imp = self.imp();
        imp.overview.replace(Some(overview));
        imp.storage_page.replace(Some(storage_page));
        imp.resource_page.replace(Some(resource_page));
        imp.container_page.replace(Some(container_page));
        imp.log_page.replace(Some(log_page));
        imp.table_pages.replace(table_pages);
        imp.navigation.replace(Some(navigation));
        imp.subtitle.replace(Some(title));
        imp.poll_pill.replace(Some(poll_pill));
        imp.toasts.replace(Some(toasts));
        imp.status.replace(Some(status));
        imp.stack.replace(Some(stack));
    }

    /// A `win.show::<tag>` action, so the menu can name a page by its tag
    /// rather than the window exposing four near-identical actions.
    fn install_actions(&self) {
        let show = gio::SimpleAction::new("show", Some(glib::VariantTy::STRING));
        show.connect_activate({
            let window = self.clone();
            move |_, tag| {
                let Some(tag) = tag.and_then(|t| t.str()) else {
                    return;
                };
                if let Some(navigation) = window.imp().navigation.borrow().as_ref() {
                    navigation.push_by_tag(tag);
                }
            }
        });
        self.add_action(&show);
    }

    /// Show a poll result.
    pub fn apply(&self, snapshot: &Snapshot, trends: &Trends, range: Range) {
        let imp = self.imp();

        if snapshot.is_empty() {
            // Nothing came back at all: that is a dead host, not a quiet one,
            // and a page of blank cards would misreport it.
            self.show_disconnected("The DiskStation did not answer.");
            return;
        }

        if let Some(stack) = imp.stack.borrow().as_ref() {
            stack.set_visible_child_name("overview");
        }
        if let Some(overview) = imp.overview.borrow().as_ref() {
            overview.update(snapshot, trends, range);
        }
        // The detail page is fed on every poll whether or not it is on screen.
        // It is one table; the alternative is showing stale drive
        // temperatures for a tick after pushing it.
        if let (Some(storage_page), Some(storage)) =
            (imp.storage_page.borrow().as_ref(), &snapshot.storage)
        {
            storage_page.update(storage);
        }
        if let Some(resource_page) = imp.resource_page.borrow().as_ref() {
            resource_page.update(trends, snapshot.utilization.as_ref(), resource_page.range());
        }
        if let (Some(container_page), Some(containers)) =
            (imp.container_page.borrow().as_ref(), &snapshot.containers)
        {
            // Projects are their own call and their own capability: a box can
            // run containers with none, so an absent list means "no projects",
            // not "wait".
            container_page.update(containers, snapshot.projects.as_deref().unwrap_or(&[]));
        }
        if let (Some(log_page), Some(log)) = (imp.log_page.borrow().as_ref(), &snapshot.log) {
            log_page.update(log);
        }

        let tables = imp.table_pages.borrow();
        if let [system, packages, shares, sessions, network, power] = tables.as_slice() {
            detail_pages::fill_system(system, snapshot);
            detail_pages::fill_packages(packages, snapshot);
            detail_pages::fill_shares(shares, snapshot);
            detail_pages::fill_sessions(sessions, snapshot);
            detail_pages::fill_network(network, snapshot);
            detail_pages::fill_power(power, snapshot);
        }
        if let Some(title) = imp.subtitle.borrow().as_ref() {
            let model = snapshot
                .system
                .as_ref()
                .and_then(|s| s.model.clone())
                .unwrap_or_else(|| "DiskStation".into());
            let containers = snapshot
                .containers
                .as_ref()
                .map(|c| format!(" · {} containers", c.len()))
                .unwrap_or_default();
            title.set_subtitle(&format!("{model}{containers}"));
        }
    }

    /// Update the "polled N seconds ago" indicator.
    pub fn set_poll_age(&self, seconds: u64) {
        if let Some(pill) = self.imp().poll_pill.borrow().as_ref() {
            pill.remove_css_class("error");
            pill.add_css_class("dim-label");
            pill.set_text(&match seconds {
                0 => "Polled just now".into(),
                1 => "Polled 1 s ago".into(),
                n => format!("Polled {n} s ago"),
            });
        }
    }

    /// Put the window into its disconnected state.
    pub fn show_disconnected(&self, reason: &str) {
        let imp = self.imp();
        if let Some(status) = imp.status.borrow().as_ref() {
            status.set_title("Disconnected");
            status.set_description(Some(reason));
        }
        if let Some(stack) = imp.stack.borrow().as_ref() {
            stack.set_visible_child_name("status");
        }
        if let Some(pill) = imp.poll_pill.borrow().as_ref() {
            pill.remove_css_class("dim-label");
            pill.add_css_class("error");
            pill.set_text("Disconnected");
        }
    }

    /// Run `f` when a container row's button is pressed. The window does not
    /// know how to reach a DiskStation; the application does.
    pub fn connect_container_action<F>(&self, f: F)
    where
        F: Fn(lookout_core::ContainerAction, String) + 'static,
    {
        if let Some(page) = self.imp().container_page.borrow().as_ref() {
            page.connect_action(f);
        }
    }

    /// Run `f` when the resource page's range toggle changes.
    pub fn connect_range_changed<F: Fn(Range) + 'static>(&self, f: F) {
        if let Some(page) = self.imp().resource_page.borrow().as_ref() {
            page.connect_range_changed(f);
        }
    }

    /// Put the resource page's toggle where the saved config says.
    pub fn set_range(&self, range: Range) {
        if let Some(page) = self.imp().resource_page.borrow().as_ref() {
            page.set_range(range);
        }
    }

    pub fn toast(&self, message: &str) {
        if let Some(toasts) = self.imp().toasts.borrow().as_ref() {
            toasts.add_toast(adw::Toast::new(message));
        }
    }
}
