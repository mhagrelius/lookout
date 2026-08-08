//! The GTK 4/libadwaita half.
//!
//! Everything here knows about widgets; nothing here knows how to talk to a
//! DiskStation. [`LookoutApplication`] owns the connection and the recorded
//! history and is the only thing that polls; pages are handed a
//! [`Snapshot`](lookout_core::poll::Snapshot) and render it.

pub mod application;
pub mod chart;
pub mod connect;
pub mod container_object;
pub mod container_page;
pub mod detail_pages;
pub mod disk_object;
pub mod log_page;
pub mod overview;
pub mod resource_page;
pub mod storage_page;
pub mod table_page;
pub mod widgets;
pub mod window;

pub use application::LookoutApplication;
pub use overview::Overview;
pub use window::LookoutWindow;

/// The stylesheet, compiled into the binary.
pub const STYLE: &str = include_str!("style.css");

/// Install the stylesheet on a display, once.
pub fn load_stylesheet(display: &gtk::gdk::Display) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// The colours the charts draw with.
///
/// The handoff is explicit that charts must read their colours from the style
/// context rather than hardcoding Adwaita's blue, so that they follow the
/// user's accent colour and the light/dark preference. The accent comes from
/// libadwaita directly; the semantic colours are the named Adwaita values for
/// the current scheme, which is the closest thing to "ask the stylesheet"
/// available without rendering a widget to sample it.
pub struct Palette {
    pub accent: (f64, f64, f64),
    pub success: (f64, f64, f64),
    pub warning: (f64, f64, f64),
    pub dim: (f64, f64, f64),
}

pub fn palette() -> Palette {
    let manager = adw::StyleManager::default();
    let dark = manager.is_dark();

    let accent_rgba = manager.accent_color_rgba();
    let accent = (
        accent_rgba.red() as f64,
        accent_rgba.green() as f64,
        accent_rgba.blue() as f64,
    );

    if dark {
        Palette {
            accent,
            success: (0.561, 0.941, 0.643), // #8ff0a4
            warning: (0.973, 0.894, 0.361), // #f8e45c
            dim: (0.6, 0.6, 0.6),
        }
    } else {
        Palette {
            accent,
            success: (0.180, 0.761, 0.494), // #2ec27e
            warning: (0.784, 0.533, 0.0),   // #c88800
            dim: (0.55, 0.55, 0.55),
        }
    }
}
