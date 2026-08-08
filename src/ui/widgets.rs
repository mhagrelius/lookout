//! The small repeated pieces of the design, built from stock widgets.
//!
//! Every one of these is a `GtkBox`/`GtkLabel`/`GtkFrame` arrangement with
//! Adwaita style classes, not a custom-drawn thing. The handoff maps each
//! mock element to a stock widget and this is that mapping — the only place
//! anything is hand-drawn is `chart.rs`.

use adw::prelude::*;
use gtk::pango;

use lookout_core::model::{Health, Volume};

/// Severity, as the style classes Adwaita already defines.
pub fn health_class(health: Health) -> &'static str {
    match health {
        Health::Normal => "success",
        Health::Warning => "warning",
        Health::Critical => "error",
        Health::Unknown => "dim-label",
    }
}

pub fn health_word(health: Health) -> &'static str {
    match health {
        Health::Normal => "Normal",
        Health::Warning => "Warning",
        Health::Critical => "Critical",
        Health::Unknown => "Unknown",
    }
}

/// `background_scrubbing` → `Scrubbing`.
///
/// DSM's `summary_status` is the field that says a volume is busy — `status`
/// reads "normal" right through a scrub — so anywhere a volume's verdict is
/// shown runs the compound word through here rather than showing the bare
/// health.
fn prettify_status(status: &str) -> String {
    let cleaned = status
        .trim_start_matches("fs_")
        .trim_start_matches("background_")
        .replace('_', " ");
    let mut chars = cleaned.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A volume's verdict and the class to colour it: `summary_status` when DSM
/// sent one, the plain health word otherwise.
fn volume_status(volume: &Volume) -> (String, &'static str) {
    let word = volume
        .summary_status
        .as_deref()
        .map(prettify_status)
        .unwrap_or_else(|| health_word(volume.health).to_string());
    (word, health_class(volume.health))
}

/// A status pill: a label with a rounded background in a severity colour.
pub fn pill(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("pill");
    label.add_css_class("caption");
    label.add_css_class(class);
    label.set_valign(gtk::Align::Center);
    label
}

/// A stat tile: caption, big number, note.
///
/// `.numeric` on the value is not decoration — it selects tabular figures, so
/// a value changing from 11% to 88% does not shift the tile's width every
/// five seconds.
pub struct StatTile {
    pub widget: gtk::Frame,
    value: gtk::Label,
    note: gtk::Label,
}

impl StatTile {
    pub fn new(caption: &str) -> Self {
        let boxed = gtk::Box::new(gtk::Orientation::Vertical, 2);
        boxed.set_margin_top(10);
        boxed.set_margin_bottom(10);
        boxed.set_margin_start(12);
        boxed.set_margin_end(12);

        let caption_label = gtk::Label::new(Some(caption));
        caption_label.add_css_class("caption");
        caption_label.add_css_class("dim-label");
        caption_label.set_xalign(0.0);

        let value = gtk::Label::new(Some("—"));
        value.add_css_class("title-2");
        value.add_css_class("numeric");
        value.set_xalign(0.0);

        let note = gtk::Label::new(None);
        note.add_css_class("caption");
        note.add_css_class("dim-label");
        note.set_xalign(0.0);
        note.set_ellipsize(pango::EllipsizeMode::End);

        boxed.append(&caption_label);
        boxed.append(&value);
        boxed.append(&note);

        let frame = gtk::Frame::new(None);
        frame.add_css_class("card");
        frame.set_child(Some(&boxed));

        StatTile {
            widget: frame,
            value,
            note,
        }
    }

    /// Set the number and its caption. `warn` turns the value amber — the
    /// handoff's thresholds are volume ≥ 80%, temperature ≥ 50 °C, CPU ≥ 90%.
    pub fn set(&self, value: &str, note: &str, warn: bool) {
        self.value.set_text(value);
        self.note.set_text(note);
        // Remove before adding: this is called on every poll, and repeatedly
        // adding the same class leaks style classes onto the widget.
        self.value.remove_css_class("warning");
        if warn {
            self.value.add_css_class("warning");
        }
    }
}

/// A container's pill: what it is doing, and whether it is answering.
///
/// Health outranks state, because a container that is running and failing its
/// health check reports `running` — showing a green "Running" pill for it is
/// the one case where the state word alone actively misleads.
pub fn container_pill(container: &lookout_core::model::Container) -> (&'static str, &'static str) {
    use lookout_core::model::ContainerHealth;

    if container.state.is_up() {
        match container.health {
            Some(ContainerHealth::Unhealthy) => return ("Unhealthy", "error"),
            Some(ContainerHealth::Starting) => return ("Starting", "warning"),
            _ => {}
        }
    }

    match container.state {
        lookout_core::model::State::Running => ("Running", "success"),
        lookout_core::model::State::Paused => ("Paused", "warning"),
        lookout_core::model::State::Restarting => ("Restarting", "warning"),
        lookout_core::model::State::Exited => ("Exited", "dim-label"),
        lookout_core::model::State::Unknown => ("Unknown", "dim-label"),
    }
}

/// A section header: a heading, a monospace API chip, and an optional
/// trailing button.
pub fn section_header(title: &str, api: &str, open: Option<&gtk::Button>) -> gtk::Box {
    section_header_parts(title, api, open).0
}

/// The same, handing back the heading label for a header whose title changes
/// with what is on the page.
pub fn section_header_parts(
    title: &str,
    api: &str,
    open: Option<&gtk::Button>,
) -> (gtk::Box, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_top(4);

    let heading = gtk::Label::new(Some(title));
    heading.add_css_class("heading");
    heading.set_xalign(0.0);
    row.append(&heading);

    let chip = gtk::Label::new(Some(api));
    chip.add_css_class("dim-label");
    chip.add_css_class("monospace");
    chip.add_css_class("caption");
    chip.set_valign(gtk::Align::Center);
    row.append(&chip);

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    row.append(&spacer);

    if let Some(button) = open {
        button.add_css_class("flat");
        row.append(button);
    }

    (row, heading)
}

/// A two-line row for a boxed list.
pub fn action_row(title: &str, subtitle: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    row.set_subtitle(subtitle);
    // Titles here are share names, container names and image tags, which are
    // not prose and must not be parsed as Pango markup — an image tag with an
    // ampersand in it would otherwise fail to render at all.
    row.set_title_lines(1);
    // Ellipsize rather than demand the full width. A subtitle carrying an
    // image name like `localhost:5050/brain-server:2026-08-04-2024` otherwise
    // sets the row's minimum width, which sets the panel's, which is what
    // decides whether a paired pair of panels fits side by side at all.
    row.set_subtitle_lines(1);
    row.set_use_markup(false);
    row
}

/// One volume, as a row for a boxed list.
///
/// Shared rather than written twice: the Overview's pool card and the drill-in
/// page show the same volume, and when only one of them read `summary_status`
/// the two screens disagreed about whether a scrub was running.
pub fn volume_row(volume: &Volume) -> adw::ActionRow {
    let row = action_row(
        &volume.name,
        &format!(
            "{} · {} of {} used · {} free",
            volume.filesystem.as_deref().unwrap_or("—"),
            format_bytes(volume.used_bytes),
            format_bytes(volume.total_bytes),
            format_bytes(volume.free_bytes()),
        ),
    );

    let bar = level_bar(volume.used_fraction());
    bar.set_size_request(160, -1);
    row.add_suffix(&bar);

    let (word, class) = volume_status(volume);
    row.add_suffix(&pill(&word, class));
    row
}

/// A boxed list, ready to have rows appended.
pub fn boxed_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    list
}

/// A capacity bar with a caption under it.
pub fn level_bar(fraction: f64) -> gtk::LevelBar {
    let bar = gtk::LevelBar::new();
    bar.set_min_value(0.0);
    bar.set_max_value(1.0);
    bar.set_value(fraction.clamp(0.0, 1.0));
    bar.set_valign(gtk::Align::Center);
    bar.set_hexpand(true);
    // Adwaita styles these offsets; setting them is what turns the bar amber
    // near full rather than leaving it accent-coloured all the way.
    bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_LOW, 0.8);
    bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_HIGH, 0.95);
    bar.add_offset_value(gtk::LEVEL_BAR_OFFSET_FULL, 1.0);
    bar
}

/// Two panels side by side that fall to one column when the window narrows.
///
/// `GtkFlowBox` rather than `AdwWrapBox`: a wrap box decides by measuring
/// widths, and a panel containing an image name like
/// `localhost:5050/brain-server:2026-08-04-2024` measures far wider than half
/// the page — so every pair fell onto its own line and the layout collapsed
/// back into the single tall column it was supposed to fix. A flow box is
/// told *how many* per line instead, which is the guarantee this needs: two
/// when there is room, one when there is not.
pub fn two_column(panels: Vec<gtk::Widget>) -> gtk::FlowBox {
    let flow = gtk::FlowBox::new();
    flow.set_selection_mode(gtk::SelectionMode::None);
    flow.set_homogeneous(true);
    flow.set_column_spacing(16);
    flow.set_row_spacing(24);
    flow.set_max_children_per_line(2);
    flow.set_min_children_per_line(1);
    flow.set_hexpand(true);
    // Without this a flow box centres its lines and leaves the second column
    // floating away from the first.
    flow.set_valign(gtk::Align::Start);

    // Both headers get the height of the tallest, so the two lists start on
    // the same line. A section header carrying an "Open →" button is a flat
    // button tall, one without it is a label tall, and the difference pushed
    // the Containers list a half-row below the Packages list beside it.
    let headers = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);

    for panel in panels {
        panel.set_hexpand(true);
        // A floor, so both pairs break to one column at the same width.
        // Without it a pair of short panels stays two-across while a pair of
        // wide ones has already wrapped, and the page breaks up unevenly as
        // the window narrows.
        panel.set_size_request(380, -1);
        // A panel is a vertical box whose first child is its section header;
        // that is what [`panel`] builds and what the hand-built ones match.
        if let Some(header) = panel.first_child() {
            headers.add_widget(&header);
        }
        flow.append(&panel);
    }
    flow
}

/// A titled panel for one half of a [`two_column`] pair.
pub fn panel(
    title: &str,
    api: &str,
    open: Option<&gtk::Button>,
    body: &impl IsA<gtk::Widget>,
) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    column.set_valign(gtk::Align::Start);
    column.append(&section_header(title, api, open));
    column.append(body);
    column
}

/// One drive, as a compact bay tile.
///
/// The design's drive overview is a grid of these rather than a list: eight
/// bays are a shape you read at a glance, and eight list rows are not. The
/// 3 px left border carries the status colour, which is why the tile is a
/// `GtkBox` with a style class rather than a `GtkFrame`.
pub fn bay_tile(bay: &str, temperature: Option<i64>, health: Health, hot: bool) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 2);
    tile.add_css_class("bay-tile");
    tile.add_css_class(match (hot, health) {
        (true, _) | (_, Health::Warning) => "bay-warning",
        (_, Health::Critical) => "bay-critical",
        (_, Health::Normal) => "bay-normal",
        (_, Health::Unknown) => "bay-unknown",
    });

    let label = gtk::Label::new(Some(bay));
    label.add_css_class("caption");
    label.add_css_class("dim-label");
    label.add_css_class("monospace");
    label.set_xalign(0.0);
    tile.append(&label);

    let temp = gtk::Label::new(Some(&match temperature {
        Some(t) => format!("{t} °C"),
        None => "—".into(),
    }));
    temp.add_css_class("numeric");
    temp.add_css_class("heading");
    temp.set_xalign(0.0);
    if hot {
        temp.add_css_class("warning");
    }
    tile.append(&temp);

    let status = gtk::Label::new(Some(health_word(health)));
    status.add_css_class("caption");
    status.add_css_class(health_class(health));
    status.set_xalign(0.0);
    tile.append(&status);

    tile
}

/// A grid of bay tiles that reflows with the width.
pub fn bay_grid() -> gtk::FlowBox {
    let grid = gtk::FlowBox::new();
    grid.set_selection_mode(gtk::SelectionMode::None);
    grid.set_homogeneous(true);
    grid.set_column_spacing(10);
    grid.set_row_spacing(10);
    // Four across at full width, per the design, but free to drop to two or
    // one as the pane narrows rather than clipping.
    grid.set_max_children_per_line(4);
    grid.set_min_children_per_line(2);
    grid
}

/// A page-level vertical box, clamped to the design's 1180 px.
pub fn page_body() -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_margin_top(22);
    content.set_margin_bottom(48);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(1180);
    clamp.set_child(Some(&content));

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&clamp));

    (scroller, content)
}

/// Bytes as a human phrase: `14.6 TB`.
///
/// Decimal units, because that is what both DSM and drive manufacturers use;
/// showing 13.3 TiB against a NAS that says 14.6 TB invites a bug report.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "kB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Bytes per second, for the network legend.
pub fn format_rate(bytes_per_second: u64) -> String {
    format!("{}/s", format_bytes(bytes_per_second))
}

/// Memory, which is binary where storage is decimal.
///
/// Not pedantry: a box with 32 GiB installed reports `ram_size: 32768` MiB,
/// and running that through the decimal formatter renders "34.4 GB" for a
/// machine whose spec sheet, BIOS and DSM all say 32 GB. RAM is sold, labelled
/// and reported in powers of two; drives are not.
pub fn format_memory_kb(kilobytes: u64) -> String {
    const UNITS: [&str; 4] = ["kB", "MB", "GB", "TB"];
    let mut value = kilobytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 || (value - value.round()).abs() < 0.05 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_render_in_decimal_units_the_way_dsm_reports_them() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1000), "1.0 kB");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
    }

    #[test]
    fn a_volume_the_size_of_the_real_one_reads_correctly() {
        // 28,770,439,729,152 bytes is the measured capacity of volume_1.
        assert_eq!(format_bytes(28_770_439_729_152), "28.8 TB");
    }

    #[test]
    fn large_values_drop_the_decimal_so_the_tile_does_not_grow() {
        assert_eq!(format_bytes(123_000_000_000), "123 GB");
    }

    #[test]
    fn rates_carry_a_per_second_suffix() {
        assert_eq!(format_rate(2048), "2.0 kB/s");
    }

    #[test]
    fn installed_memory_reads_the_way_the_spec_sheet_does() {
        // 32768 MiB is 32 GB of RAM. Through the decimal formatter it renders
        // "34.4 GB", which matches nothing the user has ever been told about
        // this machine.
        assert_eq!(format_memory_kb(32768 * 1024), "32 GB");
        assert_eq!(format_memory_kb(16 * 1024 * 1024), "16 GB");
    }

    #[test]
    fn memory_in_use_keeps_one_decimal_when_it_is_not_a_round_number() {
        // 8,307,924 kB is the measured in-use figure.
        assert_eq!(format_memory_kb(8_307_924), "7.9 GB");
    }

    #[test]
    fn storage_stays_decimal_because_drives_are_sold_that_way() {
        // The two formatters must not converge: a 10 TB drive is 10 TB.
        assert_eq!(format_bytes(10_000_831_348_736), "10.0 TB");
    }

    #[test]
    fn a_running_container_that_fails_its_health_check_does_not_read_as_fine() {
        use lookout_core::model::{Container, ContainerHealth, State};

        let running = Container {
            state: State::Running,
            ..Container::default()
        };
        assert_eq!(container_pill(&running), ("Running", "success"));

        // Its `status` is still "running". A green pill here is the Overview
        // answering "is everything OK?" with the wrong answer.
        let sick = Container {
            state: State::Running,
            health: Some(ContainerHealth::Unhealthy),
            ..Container::default()
        };
        assert_eq!(container_pill(&sick), ("Unhealthy", "error"));

        let warming = Container {
            state: State::Running,
            health: Some(ContainerHealth::Starting),
            ..Container::default()
        };
        assert_eq!(container_pill(&warming), ("Starting", "warning"));
    }

    #[test]
    fn a_stopped_containers_stale_health_does_not_outrank_being_stopped() {
        use lookout_core::model::{Container, ContainerHealth, State};

        // Docker keeps the last health verdict on a stopped container. The
        // fact that matters is that it is not running.
        let stopped = Container {
            state: State::Exited,
            health: Some(ContainerHealth::Healthy),
            ..Container::default()
        };
        assert_eq!(container_pill(&stopped), ("Exited", "dim-label"));
    }

    #[test]
    fn a_compound_summary_status_reads_as_a_word() {
        // The measured value on a mid-scrub DS-series.
        assert_eq!(prettify_status("background_scrubbing"), "Scrubbing");
        assert_eq!(prettify_status("fs_normal"), "Normal");
        assert_eq!(prettify_status("volume_rebuilding"), "Volume rebuilding");
    }

    #[test]
    fn a_volume_reports_what_it_is_doing_not_just_that_it_is_healthy() {
        use lookout_core::model::Volume;

        // A scrubbing volume's plain `status` is "normal", so showing the
        // health word alone hides the scrub the user opened the page to see.
        let scrubbing = Volume {
            health: Health::Warning,
            summary_status: Some("background_scrubbing".into()),
            ..Volume::default()
        };
        assert_eq!(
            volume_status(&scrubbing),
            ("Scrubbing".to_string(), "warning")
        );

        // DSM omits the field on some versions; the health word is the
        // fallback rather than a blank pill.
        let quiet = Volume {
            health: Health::Normal,
            ..Volume::default()
        };
        assert_eq!(volume_status(&quiet), ("Normal".to_string(), "success"));
    }

    #[test]
    fn health_maps_onto_style_classes_adwaita_already_defines() {
        assert_eq!(health_class(Health::Normal), "success");
        assert_eq!(health_class(Health::Critical), "error");
        // Unknown must not read as healthy.
        assert_eq!(health_class(Health::Unknown), "dim-label");
        assert_eq!(health_word(Health::Unknown), "Unknown");
    }
}
