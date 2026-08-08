//! The connection dialog.
//!
//! Asks for the address, the account and the password, and for a two-factor
//! code only when DSM has said it needs one. The code is asked for **once**:
//! the login that carries it also asks for a device token, which is stored and
//! used instead from then on.

use adw::prelude::*;
use gtk::glib;

use lookout_core::Config;

/// What the dialog collected.
pub struct Answer {
    pub config: Config,
    pub password: String,
    pub otp_code: Option<String>,
}

/// Show the dialog. `on_connect` runs when the user commits.
///
/// `needs_otp` puts the code field in from the start, for the second pass
/// after DSM has answered 403.
pub fn present<F>(parent: &impl IsA<gtk::Widget>, config: &Config, needs_otp: bool, on_connect: F)
where
    F: Fn(Answer) + 'static,
{
    let dialog = adw::Dialog::new();
    dialog.set_title("Connect to DiskStation");
    dialog.set_content_width(420);

    let page = adw::PreferencesPage::new();

    let host_group = adw::PreferencesGroup::new();
    host_group.set_title("DiskStation");

    let address = adw::EntryRow::new();
    address.set_title("Address");
    address.set_text(&config.address);
    host_group.add(&address);

    let port = adw::EntryRow::new();
    port.set_title("Port");
    port.set_text(&config.port.to_string());
    host_group.add(&port);

    let https = adw::SwitchRow::new();
    https.set_title("Use HTTPS");
    https.set_active(config.https);
    host_group.add(&https);

    let verify = adw::SwitchRow::new();
    verify.set_title("Verify certificate");
    // The subtitle earns its place: a DiskStation ships a self-signed
    // certificate, and without saying so this switch looks like a bug when
    // turning it off is what makes the app work.
    verify.set_subtitle("Turn off for a DiskStation using its own self-signed certificate");
    verify.set_active(config.verify_tls);
    host_group.add(&verify);

    let account_group = adw::PreferencesGroup::new();
    account_group.set_title("Account");

    let account = adw::EntryRow::new();
    account.set_title("Account");
    account.set_text(&config.account);
    account_group.add(&account);

    let password = adw::PasswordEntryRow::new();
    password.set_title("Password");
    account_group.add(&password);

    let otp = adw::EntryRow::new();
    otp.set_title("Two-factor code");
    otp.set_visible(needs_otp);
    account_group.add(&otp);

    let note = gtk::Label::new(Some(
        "The password is not saved. A two-factor code is needed once — after that \
         this machine is remembered.",
    ));
    note.add_css_class("caption");
    note.add_css_class("dim-label");
    note.set_wrap(true);
    note.set_xalign(0.0);
    note.set_margin_top(8);

    let connect = gtk::Button::with_label("Connect");
    connect.add_css_class("suggested-action");
    connect.add_css_class("pill");
    connect.set_halign(gtk::Align::End);
    connect.set_margin_top(12);

    let actions = adw::PreferencesGroup::new();
    actions.add(&note);
    actions.add(&connect);

    page.add(&host_group);
    page.add(&account_group);
    page.add(&actions);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    dialog.set_child(Some(&toolbar));

    connect.connect_clicked({
        let dialog = dialog.clone();
        let base = config.clone();
        let otp = otp.clone();
        let password = password.clone();
        move |_| {
            let typed_port = port.text().parse::<u16>().unwrap_or(base.port);
            let config = Config {
                address: address.text().trim().to_string(),
                port: typed_port,
                https: https.is_active(),
                verify_tls: verify.is_active(),
                account: account.text().trim().to_string(),
                ..base.clone()
            }
            .sanitised();

            let code = otp.text().trim().to_string();
            on_connect(Answer {
                config,
                password: password.text().to_string(),
                otp_code: (!code.is_empty()).then_some(code),
            });
            dialog.close();
        }
    });

    dialog.present(Some(parent));

    // Focus the field the user actually has to fill in: everything else is
    // usually remembered from last time.
    glib::idle_add_local_once(move || {
        if needs_otp {
            otp.grab_focus();
        } else {
            password.grab_focus();
        }
    });
}
