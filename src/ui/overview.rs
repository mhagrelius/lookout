//! The Overview: "is everything OK?" in one screen.
//!
//! Built once and then updated in place on each poll. Rebuilding the widget
//! tree every five seconds would lose scroll position, focus and any open
//! popover, so the only thing that changes is the text and the numbers.

use adw::prelude::*;
use gtk::pango;

use lookout_core::model::{format_uptime, owning_project, Health, Severity};
use lookout_core::poll::Snapshot;
use lookout_core::{Range, Trends};

use crate::ui::chart::{Axis, Chart, Series};
use crate::ui::palette;
use crate::ui::widgets::{
    action_row, bay_grid, bay_tile, boxed_list, container_pill, format_bytes, format_memory_kb,
    format_rate, health_class, health_word, level_bar, page_body, panel, pill, section_header,
    two_column, volume_row, StatTile,
};

/// How full a volume gets before the tile turns amber.
const VOLUME_WARN: f64 = 0.80;
/// How hot the box gets before the temperature tile turns amber.
const TEMP_WARN: i64 = 50;
/// Sustained CPU before the tile turns amber.
const CPU_WARN: u8 = 90;

pub struct Overview {
    pub widget: gtk::Widget,

    // Banner
    model_label: gtk::Label,
    health_pill: gtk::Box,
    properties: gtk::Grid,

    // Tiles
    cpu_tile: StatTile,
    memory_tile: StatTile,
    volume_tile: StatTile,
    temp_tile: StatTile,

    // Trends
    cpu_chart: Chart,
    network_chart: Chart,
    temp_chart: Chart,
    cpu_now: gtk::Label,
    network_now: gtk::Label,
    temp_now: gtk::Label,

    // Lists
    /// One card per pool, each holding its volumes and its drive bays.
    storage_pools: gtk::Box,
    /// The drill-in buttons. The window owns what pushing one means; the page
    /// only knows that they exist.
    open_storage: gtk::Button,
    open_resources: gtk::Button,
    open_containers: gtk::Button,
    open_logs: gtk::Button,
    containers_list: gtk::ListBox,
    containers_section: gtk::Box,
    packages_list: gtk::ListBox,
    shares_list: gtk::ListBox,
    sessions_list: gtk::ListBox,
    log_list: gtk::ListBox,
}

impl Overview {
    pub fn new() -> Self {
        let (scroller, content) = page_body();

        // --- host banner -------------------------------------------------
        let banner = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        banner.set_margin_top(18);
        banner.set_margin_bottom(18);
        banner.set_margin_start(18);
        banner.set_margin_end(18);

        let left = gtk::Box::new(gtk::Orientation::Vertical, 10);
        left.set_hexpand(true);
        // The property grid gets a floor so the tiles are the thing that
        // yields when the banner is squeezed. Without it the tiles hold four
        // across and the grid's values ellipsize down to "DS…" and "In…",
        // which is the worst of both.
        left.set_size_request(380, -1);

        let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let model_label = gtk::Label::new(Some("Connecting…"));
        model_label.add_css_class("title-2");
        model_label.set_xalign(0.0);
        let health_pill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        health_pill.set_valign(gtk::Align::Center);
        title_row.append(&model_label);
        title_row.append(&health_pill);
        left.append(&title_row);

        let properties = gtk::Grid::new();
        properties.set_row_spacing(4);
        properties.set_column_spacing(18);
        left.append(&properties);

        // A flow box rather than a row: at full width the four tiles sit in a
        // line beside the property grid, and when the pane is squeezed they
        // fold to two-by-two instead of shrinking until the numbers clip.
        let tiles = gtk::FlowBox::new();
        tiles.set_selection_mode(gtk::SelectionMode::None);
        tiles.set_max_children_per_line(4);
        tiles.set_min_children_per_line(2);
        tiles.set_column_spacing(10);
        tiles.set_row_spacing(10);
        tiles.set_homogeneous(true);
        tiles.set_valign(gtk::Align::Start);
        // Two tiles wide, so the fold is to two-by-two rather than to one
        // column of four.
        tiles.set_size_request(230, -1);
        let cpu_tile = StatTile::new("CPU");
        let memory_tile = StatTile::new("MEMORY");
        let volume_tile = StatTile::new("VOLUME");
        let temp_tile = StatTile::new("TEMP");
        for tile in [&cpu_tile, &memory_tile, &volume_tile, &temp_tile] {
            tile.widget.set_size_request(110, -1);
            tiles.append(&tile.widget);
        }

        banner.append(&left);
        banner.append(&tiles);

        let banner_frame = gtk::Frame::new(None);
        banner_frame.add_css_class("card");
        banner_frame.set_child(Some(&banner));

        content.append(&banner_frame);

        // --- trend cards -------------------------------------------------
        let open_resources = gtk::Button::with_label("Open resource monitor →");
        content.append(&section_header(
            "Trends",
            "recorded locally",
            Some(&open_resources),
        ));

        let trends_row = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        trends_row.set_homogeneous(true);

        let (cpu_card, cpu_chart, cpu_now) =
            trend_card("CPU & memory", "user + system · memory", Axis::Percent);
        let (net_card, network_chart, network_now) = trend_card("Network", "rx · tx", Axis::Bytes);
        let (temp_card, temp_chart, temp_now) = trend_card(
            "Temperature",
            "system",
            Axis::Celsius {
                min: 20.0,
                max: 70.0,
            },
        );
        trends_row.append(&cpu_card);
        trends_row.append(&net_card);
        trends_row.append(&temp_card);
        content.append(&trends_row);

        // --- storage -----------------------------------------------------
        let open_storage = gtk::Button::with_label("Open pools & drives →");
        content.append(&section_header(
            "Storage",
            "SYNO.Storage.CGI.Storage",
            Some(&open_storage),
        ));
        let storage_pools = gtk::Box::new(gtk::Orientation::Vertical, 16);
        content.append(&storage_pools);

        // --- containers | packages -----------------------------------------
        // Paired into two columns that reflow to one when the window narrows,
        // which is what stops the page being a single tall ribbon.
        let containers_section = gtk::Box::new(gtk::Orientation::Vertical, 12);
        let open_containers = gtk::Button::with_label("Open →");
        containers_section.append(&section_header(
            "Containers",
            "SYNO.Docker.Container",
            Some(&open_containers),
        ));
        let containers_list = boxed_list();
        containers_section.append(&containers_list);

        let packages_list = boxed_list();
        let packages_panel = panel("Packages", "SYNO.Core.Package", None, &packages_list);

        content.append(&two_column(vec![
            containers_section.clone().upcast(),
            packages_panel.upcast(),
        ]));

        // --- shares | sessions ----------------------------------------------
        let shares_list = boxed_list();
        let shares_panel = panel("Shared folders", "SYNO.Core.Share", None, &shares_list);

        let sessions_list = boxed_list();
        let sessions_panel = panel(
            "Users & sessions",
            "SYNO.Core.CurrentConnection",
            None,
            &sessions_list,
        );

        content.append(&two_column(vec![
            shares_panel.upcast(),
            sessions_panel.upcast(),
        ]));

        // --- log ----------------------------------------------------------
        let open_logs = gtk::Button::with_label("Open log viewer →");
        content.append(&section_header(
            "Recent log",
            "SYNO.Core.SyslogClient.Log",
            Some(&open_logs),
        ));
        let log_list = boxed_list();
        content.append(&log_list);

        Overview {
            widget: scroller.upcast(),
            model_label,
            health_pill,
            properties,
            cpu_tile,
            memory_tile,
            volume_tile,
            temp_tile,
            cpu_chart,
            network_chart,
            temp_chart,
            cpu_now,
            network_now,
            temp_now,
            storage_pools,
            open_storage,
            open_resources,
            open_containers,
            open_logs,
            containers_list,
            containers_section,
            packages_list,
            shares_list,
            sessions_list,
            log_list,
        }
    }

    /// Run `f` when the storage section's Open button is activated.
    pub fn connect_open_storage<F: Fn() + 'static>(&self, f: F) {
        self.open_storage.connect_clicked(move |_| f());
    }

    pub fn connect_open_resources<F: Fn() + 'static>(&self, f: F) {
        self.open_resources.connect_clicked(move |_| f());
    }

    pub fn connect_open_containers<F: Fn() + 'static>(&self, f: F) {
        self.open_containers.connect_clicked(move |_| f());
    }

    pub fn connect_open_logs<F: Fn() + 'static>(&self, f: F) {
        self.open_logs.connect_clicked(move |_| f());
    }

    /// Apply a poll result.
    pub fn update(&self, snap: &Snapshot, trends: &Trends, range: Range) {
        self.update_banner(snap);
        self.update_tiles(snap);
        self.update_charts(trends, range);
        self.update_storage(snap);
        self.update_containers(snap);
        self.update_packages(snap);
        self.update_shares(snap);
        self.update_sessions(snap);
        self.update_log(snap);
    }

    fn update_banner(&self, snap: &Snapshot) {
        let Some(system) = &snap.system else { return };

        self.model_label
            .set_text(system.model.as_deref().unwrap_or("DiskStation"));

        // The banner's health is the storage health, since that is the thing
        // that actually goes wrong on a NAS. A temperature warning overrides
        // it because a hot box is the more urgent sentence.
        let health = match (&snap.storage, system.temperature_warning) {
            (_, true) => Health::Warning,
            (Some(storage), _) => storage.worst_health(),
            (None, _) => Health::Unknown,
        };
        while let Some(child) = self.health_pill.first_child() {
            self.health_pill.remove(&child);
        }
        self.health_pill
            .append(&pill(health_word(health), health_class(health)));

        let rows: Vec<(&str, String)> = vec![
            (
                "DSM",
                system
                    .firmware_version
                    .clone()
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "Uptime",
                system
                    .uptime
                    .map(format_uptime)
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "CPU",
                system.cpu_description().unwrap_or_else(|| "—".into()),
            ),
            (
                "Memory",
                system
                    .ram_mb
                    .map(|mb| format_memory_kb(mb * 1024))
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "Serial",
                system.serial.clone().unwrap_or_else(|| "—".into()),
            ),
            (
                "Time zone",
                system.time_zone.clone().unwrap_or_else(|| "—".into()),
            ),
        ];

        // Rebuilt rather than diffed: six rows, and the alternative is
        // tracking six label handles for text that rarely changes.
        while let Some(child) = self.properties.first_child() {
            self.properties.remove(&child);
        }
        for (i, (name, value)) in rows.iter().enumerate() {
            let row = i as i32 / 2;
            let column = (i as i32 % 2) * 2;

            let label = gtk::Label::new(Some(name));
            label.add_css_class("dim-label");
            label.add_css_class("caption");
            label.set_xalign(0.0);

            let value_label = gtk::Label::new(Some(value));
            value_label.add_css_class("caption");
            value_label.add_css_class("monospace");
            value_label.set_xalign(0.0);
            value_label.set_ellipsize(pango::EllipsizeMode::End);
            value_label.set_max_width_chars(28);

            self.properties.attach(&label, column, row, 1, 1);
            self.properties.attach(&value_label, column + 1, row, 1, 1);
        }
    }

    fn update_tiles(&self, snap: &Snapshot) {
        if let Some(u) = &snap.utilization {
            let cpu = u.cpu.total();
            self.cpu_tile.set(
                &format!("{cpu}%"),
                &format!("load {:.2}", u.cpu.load_1),
                cpu >= CPU_WARN,
            );
            self.memory_tile.set(
                &format!("{}%", u.memory.usage_percent),
                &format!("{} used", format_memory_kb(u.memory.used_kb())),
                u.memory.usage_percent >= 90,
            );
        }

        if let Some(storage) = &snap.storage {
            if let Some(volume) = storage.volumes.first() {
                let fraction = volume.used_fraction();
                self.volume_tile.set(
                    &format!("{:.0}%", fraction * 100.0),
                    &format!("{} free", format_bytes(volume.free_bytes())),
                    fraction >= VOLUME_WARN,
                );
            }
        }

        if let Some(system) = &snap.system {
            match system.temperature_c {
                Some(t) => self.temp_tile.set(
                    &format!("{t}°C"),
                    "system",
                    system.temperature_warning || t >= TEMP_WARN,
                ),
                None => self.temp_tile.set("—", "no sensor", false),
            }
        }
    }

    fn update_charts(&self, trends: &Trends, range: Range) {
        let samples = trends.window(range, chrono::Utc::now());
        let colors = palette();

        let cpu: Vec<f64> = samples.iter().map(|s| s.cpu_percent as f64).collect();
        let memory: Vec<f64> = samples.iter().map(|s| s.memory_percent as f64).collect();
        if let Some(last) = cpu.last() {
            self.cpu_now.set_text(&format!("{last:.0}%"));
        }
        self.cpu_chart.set_series(vec![
            Series {
                values: cpu,
                color: colors.accent,
            },
            Series {
                values: memory,
                color: colors.dim,
            },
        ]);

        let rx: Vec<f64> = samples.iter().map(|s| s.network_rx as f64).collect();
        let tx: Vec<f64> = samples.iter().map(|s| s.network_tx as f64).collect();
        if let (Some(r), Some(t)) = (rx.last(), tx.last()) {
            self.network_now.set_text(&format!(
                "{} ↓ {} ↑",
                format_rate(*r as u64),
                format_rate(*t as u64)
            ));
        }
        self.network_chart.set_series(vec![
            Series {
                values: rx,
                color: colors.accent,
            },
            Series {
                values: tx,
                color: colors.success,
            },
        ]);

        let temps: Vec<f64> = samples
            .iter()
            .filter_map(|s| s.temperature_c)
            .map(|t| t as f64)
            .collect();
        if let Some(last) = temps.last() {
            self.temp_now.set_text(&format!("{last:.0}°C"));
        }
        self.temp_chart.set_series(vec![Series {
            values: temps,
            color: colors.warning,
        }]);
    }

    fn update_storage(&self, snap: &Snapshot) {
        while let Some(child) = self.storage_pools.first_child() {
            self.storage_pools.remove(&child);
        }

        let Some(storage) = &snap.storage else {
            let list = boxed_list();
            list.append(&unavailable_row("Storage"));
            self.storage_pools.append(&list);
            return;
        };

        // One card per pool, holding the volumes on it and the drives that
        // make it up. This is DSM's own hierarchy — a volume carries
        // `pool_path` and a pool carries its `disks` — and showing the three
        // as one flat list threw that structure away.
        for pool in &storage.pools {
            self.storage_pools.append(&pool_card(storage, pool));
        }

        // Anything DSM has not allocated still has to appear somewhere.
        let loose = storage.unassigned_disks();
        if !loose.is_empty() {
            let grid = bay_grid();
            for disk in &loose {
                grid.append(&bay_tile(
                    &disk.name,
                    disk.temperature_c,
                    disk.smart_health,
                    disk.is_hot(),
                ));
            }
            self.storage_pools
                .append(&panel("Unassigned drives", "", None, &grid));
        }

        if storage.pools.is_empty() && loose.is_empty() {
            let list = boxed_list();
            list.append(&action_row(
                "No storage pools",
                "This DiskStation reports none",
            ));
            self.storage_pools.append(&list);
        }
    }

    fn update_packages(&self, snap: &Snapshot) {
        clear(&self.packages_list);
        let Some(packages) = &snap.packages else {
            self.packages_list.append(&unavailable_row("Packages"));
            return;
        };

        // Updates first, then running, so the row worth acting on is at the
        // top of a list that is otherwise alphabetical and long.
        let mut sorted: Vec<_> = packages.iter().collect();
        sorted.sort_by_key(|p| (!p.has_update(), !p.is_running()));

        for package in sorted.iter().take(6) {
            let row = action_row(&package.name, &package.version);
            if package.has_update() {
                row.add_suffix(&pill("Update", "accent"));
            } else if package.is_running() {
                row.add_suffix(&pill("Running", "success"));
            } else {
                row.add_suffix(&pill("Stopped", "dim-label"));
            }
            self.packages_list.append(&row);
        }
    }

    fn update_sessions(&self, snap: &Snapshot) {
        clear(&self.sessions_list);
        let Some(sessions) = &snap.sessions else {
            self.sessions_list.append(&unavailable_row("Sessions"));
            return;
        };
        if sessions.is_empty() {
            self.sessions_list
                .append(&action_row("Nobody connected", "No active sessions"));
            return;
        }

        for session in sessions.iter().take(5) {
            let row = action_row(
                &session.who,
                &format!("{} · {}", session.service, session.from),
            );
            if session.is_current {
                row.add_suffix(&pill("You", "accent"));
            }
            self.sessions_list.append(&row);
        }
    }

    fn update_containers(&self, snap: &Snapshot) {
        // No Container Manager means no section at all, rather than an empty
        // card with an error in it.
        let Some(containers) = &snap.containers else {
            self.containers_section.set_visible(false);
            return;
        };
        self.containers_section.set_visible(true);
        clear(&self.containers_list);

        for container in containers {
            let cpu = container
                .cpu_percent
                .map(|c| format!("{c:.1}% CPU"))
                .unwrap_or_else(|| "—".into());
            let memory = container
                .memory_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "—".into());
            // The compose project that deployed it, first: it is the field
            // that says what stops and starts alongside this container, and
            // leading with it keeps it legible once the subtitle ellipsizes.
            //
            // In the subtitle rather than as a suffix widget. A suffix label
            // adds to the row's minimum width, the row's minimum sets the
            // panel's, and the panel's is what decides whether Containers and
            // Packages fit side by side — one extra label collapsed the pair
            // into a single column.
            let owner = owning_project(snap.projects.as_deref().unwrap_or(&[]), container);
            let subtitle = match owner {
                Some(project) => {
                    format!("{} · {} · {cpu} · {memory}", project.name, container.image)
                }
                None => format!("{} · {cpu} · {memory}", container.image),
            };
            let row = action_row(&container.name, &subtitle);

            // Health-aware: this screen answers "is everything OK?", so the
            // one pill it has room for carries the worst true thing. A
            // container failing its health check is still `running`, and a
            // green pill for it is the answer being wrong.
            let (word, class) = container_pill(container);
            row.add_suffix(&pill(word, class));
            self.containers_list.append(&row);
        }
    }

    fn update_shares(&self, snap: &Snapshot) {
        clear(&self.shares_list);
        let Some(shares) = &snap.shares else {
            self.shares_list.append(&unavailable_row("Shared folders"));
            return;
        };

        for share in shares {
            let subtitle = match share.quota_bytes {
                Some(quota) => format!(
                    "{} of {} · {}",
                    format_bytes(share.used_bytes),
                    format_bytes(quota),
                    share.volume_path
                ),
                None => format!(
                    "{} · no quota · {}",
                    format_bytes(share.used_bytes),
                    share.volume_path
                ),
            };
            let row = action_row(&share.name, &subtitle);
            if let Some(fraction) = share.used_fraction() {
                let bar = level_bar(fraction);
                bar.set_size_request(140, -1);
                row.add_suffix(&bar);
            }
            self.shares_list.append(&row);
        }
    }

    fn update_log(&self, snap: &Snapshot) {
        clear(&self.log_list);
        let Some(log) = &snap.log else {
            self.log_list.append(&unavailable_row("System log"));
            return;
        };

        for entry in log.entries.iter().take(6) {
            let row = action_row(&entry.message, &format!("{} · {}", entry.time, entry.who));
            let class = match entry.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "dim-label",
            };
            let severity = pill(entry.severity.label(), class);
            severity.set_size_request(74, -1);
            row.add_prefix(&severity);
            self.log_list.append(&row);
        }
    }
}

impl Default for Overview {
    fn default() -> Self {
        Overview::new()
    }
}

/// A card holding a title, a current value, and a chart.
fn trend_card(title: &str, legend: &str, axis: Axis) -> (gtk::Frame, Chart, gtk::Label) {
    let boxed = gtk::Box::new(gtk::Orientation::Vertical, 8);
    boxed.set_margin_top(14);
    boxed.set_margin_bottom(14);
    boxed.set_margin_start(18);
    boxed.set_margin_end(18);

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title_label = gtk::Label::new(Some(title));
    title_label.add_css_class("heading");
    title_label.set_xalign(0.0);
    title_label.set_hexpand(true);
    let now = gtk::Label::new(Some("—"));
    now.add_css_class("numeric");
    now.add_css_class("accent");
    header.append(&title_label);
    header.append(&now);
    boxed.append(&header);

    let chart = Chart::new(axis, 62);
    boxed.append(&chart.area);

    let legend_label = gtk::Label::new(Some(legend));
    legend_label.add_css_class("caption");
    legend_label.add_css_class("dim-label");
    legend_label.set_xalign(0.0);
    boxed.append(&legend_label);

    let frame = gtk::Frame::new(None);
    frame.add_css_class("card");
    frame.set_child(Some(&boxed));

    (frame, chart, now)
}

/// A pool, with its volumes and its drive bays nested inside it.
fn pool_card(
    storage: &lookout_core::model::Storage,
    pool: &lookout_core::model::Pool,
) -> gtk::Frame {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.add_css_class("pool-card");

    // Header: the pool itself.
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let title = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let name = gtk::Label::new(Some(&format!("Storage Pool {}", pool.id)));
    name.add_css_class("heading");
    name.set_xalign(0.0);
    let detail = gtk::Label::new(Some(&format!(
        "{} · {} · {} drives",
        pool.raid_type.as_deref().unwrap_or("unknown layout"),
        format_bytes(pool.total_bytes),
        storage.disks_in(pool).len(),
    )));
    detail.add_css_class("caption");
    detail.add_css_class("dim-label");
    detail.set_xalign(0.0);
    title.append(&name);
    title.append(&detail);
    title.set_hexpand(true);
    header.append(&title);
    header.append(&pill(health_word(pool.health), health_class(pool.health)));
    body.append(&header);

    // Nested: the volumes on this pool, then the drives under it.
    let children = gtk::Box::new(gtk::Orientation::Vertical, 12);
    children.add_css_class("pool-children");

    let volumes = storage.volumes_in(pool);
    if !volumes.is_empty() {
        let list = boxed_list();
        for volume in &volumes {
            list.append(&volume_row(volume));
        }
        children.append(&list);
    }

    let disks = storage.disks_in(pool);
    if !disks.is_empty() {
        let grid = bay_grid();
        for disk in &disks {
            grid.append(&bay_tile(
                &disk.name,
                disk.temperature_c,
                disk.smart_health,
                disk.is_hot(),
            ));
        }
        children.append(&grid);
    }

    body.append(&children);

    let frame = gtk::Frame::new(None);
    frame.add_css_class("card");
    frame.set_child(Some(&body));
    frame
}

fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

/// The row shown where a card's endpoint failed.
///
/// Greying out one card is the design's rule; the alternative — an empty list
/// — reads as "you have no shares", which is a different and wrong statement.
fn unavailable_row(what: &str) -> adw::ActionRow {
    let row = action_row(what, "Not available from this DiskStation right now");
    row.add_css_class("dim-label");
    row
}
