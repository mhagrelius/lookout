//! Entry point. Everything worth reading is in [`lookout::ui::LookoutApplication`].

use gtk::prelude::*;
use lookout::ui::LookoutApplication;

fn main() -> gtk::glib::ExitCode {
    LookoutApplication::new().run()
}
