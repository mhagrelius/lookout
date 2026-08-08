//! A Synology DiskStation monitor for the GNOME desktop.
//!
//! The crate splits in two, and the split is the design rather than tidiness.
//! [`lookout_core`] is plain Rust — the DSM client, the records, the recorded
//! history — with no GTK, no display and no main loop, so it is testable
//! anywhere and **a shell on Windows or macOS keeps it whole**. The [`ui`]
//! half wraps it in GTK 4/libadwaita and is the only place that knows a window
//! exists.
//!
//! Anything added to `ui` that a second frontend would also need belongs in
//! `lookout-core` instead.

/// The half that does not draw, in its own crate so another frontend can link
/// it without libadwaita.
pub use lookout_core as core;

pub mod ui;

/// The application ID, used for D-Bus, the desktop file, and the icon.
pub const APP_ID: &str = "us.hagreli.Lookout";
