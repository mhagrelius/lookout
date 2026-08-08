//! Render the real widget tree to a PNG.
//!
//! Screenshotting a live GNOME Wayland session needs interactive consent,
//! which makes "does this look right?" hard to answer while iterating. This
//! builds the actual Overview against a seeded snapshot and paints it
//! offscreen instead.
//!
//! ```sh
//! cargo run --example preview -- /tmp/preview
//! cargo run --example preview -- /tmp/preview dark
//! ```
//!
//! The seed is the **measured** reply from a DS-series, not invented numbers,
//! so the picture shows the widths and roundings real data produces.

use adw::prelude::*;
use gtk::glib;

use lookout::core::model::{
    Container, Cooling, LogPage, NetworkInterface, Package, Project, Session, Share, Storage,
    SystemInfo, Ups, Utilization,
};
use lookout::core::poll::Snapshot;
use lookout::core::{Range, Sample, Trends};
use lookout::ui::container_page::ContainerPage;
use lookout::ui::log_page::LogPageView;
use lookout::ui::resource_page::ResourcePage;
use lookout::ui::storage_page::StoragePage;
use lookout::ui::Overview;

use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "/tmp/preview".to_string());
    let dark = args.next().is_some_and(|scheme| scheme == "dark");

    gtk::init().expect("a display — run under xvfb-run if there is none");
    adw::init().expect("libadwaita");

    // An animating widget is a widget that is not finished being laid out.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }

    adw::StyleManager::default().set_color_scheme(if dark {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    });
    if let Some(display) = gtk::gdk::Display::default() {
        lookout::ui::load_stylesheet(&display);
    }

    std::fs::create_dir_all(&out).expect("output directory");

    let overview = Overview::new();
    overview.update(&seeded(), &seeded_trends(), Range::Hour);
    render(
        &overview.widget,
        1180,
        1400,
        &format!("{out}/overview-{}.png", scheme(dark)),
    );

    // The same page at a narrow width, because the paired sections are only
    // responsive if they actually reflow — and that is invisible at 1180.
    let narrow = Overview::new();
    narrow.update(&seeded(), &seeded_trends(), Range::Hour);
    render(
        &narrow.widget,
        720,
        1500,
        &format!("{out}/overview-narrow-{}.png", scheme(dark)),
    );

    // The same page with everything unavailable, because the degraded state
    // is the one that is never looked at until it is wrong.
    let empty = Overview::new();
    empty.update(&Snapshot::default(), &Trends::new(), Range::Hour);
    render(
        &empty.widget,
        1180,
        700,
        &format!("{out}/overview-unavailable-{}.png", scheme(dark)),
    );

    // The drill-in, which is the pattern the remaining detail pages reuse.
    let storage = StoragePage::new();
    if let Some(snapshot_storage) = &seeded().storage {
        storage.update(snapshot_storage);
    }
    render(
        &storage.page,
        1180,
        620,
        &format!("{out}/storage-{}.png", scheme(dark)),
    );

    // The same page on a box the target NAS is not: two pools, and a drive in
    // neither. The single-pool render cannot show whether the cards separate,
    // whether the two tables line up, or that a spare appears at all.
    let multi = StoragePage::new();
    multi.update(&two_pool_storage());
    render(
        &multi.page,
        1180,
        1000,
        &format!("{out}/storage-multipool-{}.png", scheme(dark)),
    );

    let resources = ResourcePage::new();
    resources.update(&seeded_trends(), seeded().utilization.as_ref(), Range::Hour);
    render(
        &resources.page,
        1180,
        1100,
        &format!("{out}/resources-{}.png", scheme(dark)),
    );

    let containers = ContainerPage::new();
    if let Some(list) = &seeded().containers {
        containers.update(list, &seeded_projects());
    }
    render(
        &containers.page,
        1180,
        900,
        &format!("{out}/containers-{}.png", scheme(dark)),
    );

    // The same page on a box with no compose projects — a DSM without the
    // Project API, or Container Manager with nothing deployed from a file.
    // The projects half should be gone rather than an empty heading, and the
    // remaining section titled "Containers" rather than "Not in a project".
    let projectless = ContainerPage::new();
    if let Some(list) = &seeded().containers {
        projectless.update(list, &[]);
    }
    render(
        &projectless.page,
        1180,
        560,
        &format!("{out}/containers-no-projects-{}.png", scheme(dark)),
    );

    let logs = LogPageView::new();
    if let Some(log) = &seeded().log {
        logs.update(log);
    }
    render(
        &logs.page,
        1180,
        560,
        &format!("{out}/logs-{}.png", scheme(dark)),
    );

    // The four generic template pages.
    for (page, fill) in [
        (
            lookout::ui::detail_pages::system_page(),
            lookout::ui::detail_pages::fill_system
                as fn(&lookout::ui::table_page::TablePage, &Snapshot),
        ),
        (
            lookout::ui::detail_pages::packages_page(),
            lookout::ui::detail_pages::fill_packages,
        ),
        (
            lookout::ui::detail_pages::shares_page(),
            lookout::ui::detail_pages::fill_shares,
        ),
        (
            lookout::ui::detail_pages::sessions_page(),
            lookout::ui::detail_pages::fill_sessions,
        ),
        (
            lookout::ui::detail_pages::network_page(),
            lookout::ui::detail_pages::fill_network,
        ),
        (
            lookout::ui::detail_pages::power_page(),
            lookout::ui::detail_pages::fill_power,
        ),
    ] {
        fill(&page, &seeded());
        let tag = page.page.tag().unwrap_or_default().to_string();
        render(
            &page.page,
            1180,
            520,
            &format!("{out}/page-{tag}-{}.png", scheme(dark)),
        );
    }

    println!("wrote {out}");
}

fn scheme(dark: bool) -> &'static str {
    if dark {
        "dark"
    } else {
        "light"
    }
}

/// Compose projects owning some of the containers, in the keyed-by-id shape
/// `SYNO.Docker.Project`/`list` actually answers with.
///
/// `pihole` is deliberately in no project, so the render
/// shows the section that catches a plain `docker run` container.
fn seeded_projects() -> Vec<Project> {
    Project::list_from_json(&json!({
        "bddfea05-8010-4dd9-a1c8-8d93867040b8": {
            "id": "bddfea05-8010-4dd9-a1c8-8d93867040b8",
            "name": "brain", "status": "running",
            "share_path": "/volume1/docker/brain",
            "containerIds": ["a", "b"]
        },
        "8ec29f37-76bb-49c9-bf07-9b30403fed54": {
            "id": "8ec29f37-76bb-49c9-bf07-9b30403fed54",
            "name": "llama", "status": "partial",
            "share_path": "/volume1/docker/llama",
            "containerIds": ["c"]
        }
    }))
}

/// The container listing with the resource call folded in.
///
/// `SYNO.Docker.Container.Resource` is a second call, and seeding only the
/// first left every CPU and memory cell rendering as an em dash — so the
/// preview showed a degraded state as if it were the normal one.
///
/// The `State` objects are the measured shape, `up_time: null` and all: a
/// fixture carrying a flat `up_time` renders an uptime column that no real
/// compose container produces.
fn seeded_containers() -> Vec<Container> {
    let mut containers = Container::list_from_json(&json!({"containers": [
        {"id": "a", "name": "brain-server", "image": "localhost:5050/brain-server:2026-08-04-2024",
         "status": "running", "up_time": null,
         "State": {"Running": true, "ExitCode": 0, "OOMKilled": false,
                   "StartedAt": "2026-08-05T13:26:32.78343004Z",
                   "Health": {"Status": "healthy", "FailingStreak": 0}}},
        {"id": "b", "name": "planner-server", "image": "localhost:5050/planner-server:2026-08-05",
         "status": "running", "up_time": null,
         // No Health object at all, which is the common case.
         "State": {"Running": true, "ExitCode": 0, "OOMKilled": false,
                   "StartedAt": "2026-08-07T09:10:00Z"}},
        {"id": "c", "name": "llama-embed", "image": "ghcr.io/ggml-org/llama.cpp:server",
         "status": "running", "up_time": null,
         // Running and not answering — the case a state pill alone hides.
         "State": {"Running": true, "ExitCode": 0, "OOMKilled": false,
                   "StartedAt": "2026-08-03T22:00:00Z",
                   "Health": {"Status": "unhealthy", "FailingStreak": 4}}},
        {"id": "d", "name": "pihole", "image": "pihole/pihole:latest",
         "status": "exited", "up_time": null,
         "State": {"Running": false, "ExitCode": 137, "OOMKilled": true,
                   "StartedAt": "2026-08-02T04:15:00Z",
                   "FinishedAt": "2026-08-02T05:02:11Z"}}
    ]}));

    Container::apply_resources(
        &mut containers,
        &json!({"containers": [
            {"name": "brain-server", "cpu": 1.8, "memory": 486539264},
            {"name": "planner-server", "cpu": 0.4, "memory": 201326592},
            {"name": "llama-embed", "cpu": 12.6, "memory": 3221225472u64}
        ]}),
    );
    containers
}

/// Two pools and a spare, in the shape DSM sends them.
///
/// Invented rather than measured — the DS-series has one pool — so it is used
/// only to render a layout, never to justify a field name.
fn two_pool_storage() -> Storage {
    Storage::from_json(&json!({
        "volumes": [
            {"id": "volume_1", "device_type": "shr_1", "fs_type": "btrfs",
             "status": "normal", "summary_status": "fs_normal",
             "pool_path": "reuse_1", "vol_path": "/volume1",
             "size": {"total": "28770439729152", "used": "14604487745536"}},
            {"id": "volume_2", "display_name": "media", "device_type": "raid_1",
             "fs_type": "ext4", "status": "normal",
             "summary_status": "background_scrubbing",
             "pool_path": "reuse_2", "vol_path": "/volume2",
             "size": {"total": "3840000000000", "used": "3610000000000"}}
        ],
        "disks": [
            {"id": "sda", "name": "Drive 1", "model": "ST10000VN0008-2PJ103",
             "temp": 31, "smart_status": "normal", "status": "normal",
             "size_total": "10000831348736", "used_by": "reuse_1"},
            {"id": "sdb", "name": "Drive 2", "model": "ST10000VN0008-2PJ103",
             "temp": 47, "smart_status": "normal", "status": "normal",
             "size_total": "10000831348736", "used_by": "reuse_1"},
            {"id": "sdc", "name": "Drive 3", "model": "ST10000VN0008-2PJ103",
             "temp": 33, "smart_status": "normal", "status": "normal",
             "size_total": "10000831348736", "used_by": "reuse_1"},
            {"id": "sdd", "name": "Drive 4", "model": "WD40EFRX-68N32N0",
             "temp": 38, "smart_status": "normal", "status": "normal",
             "size_total": "4000787030016", "used_by": "reuse_2"},
            {"id": "sde", "name": "Drive 5", "model": "WD40EFRX-68N32N0",
             "temp": 39, "smart_status": "warning", "status": "normal",
             "size_total": "4000787030016", "used_by": "reuse_2"},
            {"id": "sdf", "name": "Drive 6", "model": "ST4000VN008-2DR166",
             "temp": 29, "smart_status": "normal", "status": "normal",
             "size_total": "4000787030016"}
        ],
        "storagePools": [
            {"id": "reuse_1", "device_type": "shr_1", "status": "normal",
             "disks": ["sda", "sdb", "sdc"],
             "size": {"total": "29969208049664", "used": "14604487745536"}},
            {"id": "reuse_2", "device_type": "raid_1", "status": "background",
             "disks": ["sdd", "sde"],
             "size": {"total": "4000787030016", "used": "3610000000000"}}
        ]
    }))
}

/// A snapshot built from the measured replies.
fn seeded() -> Snapshot {
    Snapshot {
        system: Some(SystemInfo::from_json(&json!({
            "model": "DS-series",
            "serial": "0000ABC000000",
            "firmware_ver": "DSM 7.2.2-72806 Update 3",
            "up_time": "64:48:7",
            "sys_temp": 42,
            "cpu_vendor": "INTEL", "cpu_series": "D-1527",
            "cpu_cores": "4", "cpu_clock_speed": 2200,
            "ram_size": 32768,
            "time_zone_desc": "(GMT-05:00) Eastern Time"
        }))),
        utilization: Some(Utilization::from_json(&json!({
            "cpu": {"user_load": 4, "system_load": 2, "other_load": 1,
                    "1min_load": 27, "5min_load": 10, "15min_load": 2},
            "memory": {"real_usage": 7, "total_real": 32641796,
                       "avail_real": 24333872, "cached": 5707596},
            "network": [{"device": "total", "rx": 1048576, "tx": 262144}],
            "disk": {"total": {"utilization": 2, "read_byte": 9478, "write_byte": 4096}}
        }))),
        storage: Some(Storage::from_json(&json!({
            "volumes": [{"id": "volume_1", "device_type": "shr_1", "fs_type": "btrfs",
                         "status": "normal", "summary_status": "background_scrubbing",
                         "pool_path": "reuse_1", "vol_path": "/volume1",
                         "size": {"total": "28770439729152", "used": "14604487745536"}}],
            "disks": [
                {"id": "sda", "name": "Drive 1", "model": "ST10000VN0008-2PJ103",
                 "temp": 31, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"},
                {"id": "sdb", "name": "Drive 2", "model": "ST10000VN0008-2PJ103",
                 "temp": 47, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"},
                {"id": "sdc", "name": "Drive 3", "model": "ST10000VN0008-2PJ103",
                 "temp": 33, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"},
                {"id": "sdd", "name": "Drive 4", "model": "ST10000VN0008-2PJ103",
                 "temp": 32, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"},
                {"id": "sde", "name": "Drive 5", "model": "ST10000VN0008-2PJ103",
                 "temp": 30, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"}
            ],
            "storagePools": [{"id": "reuse_1", "device_type": "shr_1", "status": "normal",
                              "disks": ["sda", "sdb", "sdc", "sdd", "sde"],
                              "size": {"total": "29969208049664", "used": "14604487745536"}}]
        }))),
        containers: Some(seeded_containers()),
        projects: Some(seeded_projects()),
        shares: Some(Share::list_from_json(&json!({"shares": [
            {"name": "docker", "vol_path": "/volume1", "quota_value": 0,
             "share_quota_used": 322.18359375},
            {"name": "Games", "vol_path": "/volume1", "quota_value": 0,
             "share_quota_used": 12418721.0},
            {"name": "photo", "vol_path": "/volume1", "quota_value": 0,
             "share_quota_used": 507941.09375},
            {"name": "homes", "vol_path": "/volume1", "quota_value": 1048576.0,
             "share_quota_used": 282196.75}
        ]}))),
        log: Some(LogPage::from_json(&json!({
            "errorCount": 0, "warnCount": 1, "infoCount": 2, "total": 3206,
            "items": [
                {"descr": "Server back online.", "level": "info", "logtype": "System",
                 "time": "2026/08/04 10:09:51", "who": "SYSTEM"},
                {"descr": "System successfully started up from an improper shutdown.",
                 "level": "warn", "logtype": "System",
                 "time": "2026/08/04 10:09:12", "who": "SYSTEM"},
                {"descr": "User [admin] logged in from 100.101.102.103 via DSM.",
                 "level": "info", "logtype": "Connection",
                 "time": "2026/08/04 09:58:03", "who": "admin"}
            ]
        }))),
        packages: Some(Package::list_from_json(&json!({"packages": [
            {"id": "ContainerManager", "name": "Container Manager",
             "version": "24.0.2-1606", "additional": {"status": "running"}},
            {"id": "SynologyPhotos", "name": "Synology Photos",
             "version": "1.7.0-0794", "additional": {"status": "running"},
             "available_version": "1.8.0-0801"},
            {"id": "AntiVirus", "name": "Antivirus Essential",
             "version": "1.4.6-0272", "additional": {"status": "stopped"}},
            {"id": "FileStation", "name": "File Station",
             "version": "1.4.3-1610", "additional": {"status": "running"}}
        ]}))),
        sessions: Some(Session::list_from_json(&json!({"items": [
            {"who": "admin", "from": "100.101.102.103", "descr": "DSM",
             "first_login_time": "2026/08/04 09:58:03",
             "can_be_kicked": true, "is_current_connected": true},
            {"who": "someone", "from": "192.0.2.9", "descr": "SMB",
             "first_login_time": "2026/08/04 08:00:00",
             "can_be_kicked": true, "is_current_connected": false}
        ]}))),
        interfaces: Some(NetworkInterface::list_from_json(&json!({
            "0": {"ifname": "ovs_eth0", "ip": "192.0.2.4", "mask": "255.255.255.0",
                  "speed": 10000, "status": "connected", "type": "ovseth", "use_dhcp": false},
            "1": {"ifname": "eth1", "ip": "", "mask": "", "speed": 0,
                  "status": "disconnected", "type": "eth", "use_dhcp": true}
        }))),
        cooling: Some(Cooling::from_json(&json!({
            "all_disk_temp_fail": "no", "dual_fan_speed": "coolfan"
        }))),
        ups: Some(Ups::from_json(&json!({
            "enable": true, "manufacture": "APC", "model": "Back-UPS 1500",
            "charge": 100, "runtime": 3900, "mode": "usb"
        }))),
        failures: Vec::new(),
    }
}

/// An hour of plausible history, so the charts have something to draw.
fn seeded_trends() -> Trends {
    let mut trends = Trends::new();
    let start = chrono::Utc::now() - chrono::Duration::hours(1);

    for i in 0..720 {
        let t = i as f64;
        // A quiet box with one burst in it, so the fixed axis and the
        // peak-preserving downsample are both visible in the picture.
        let busy = (330.0..360.0).contains(&t);
        trends.record(Sample {
            at: start + chrono::Duration::seconds(i * 5),
            cpu_percent: if busy {
                74
            } else {
                (6.0 + (t / 40.0).sin() * 4.0) as u8
            },
            memory_percent: (7.0 + (t / 160.0).sin() * 2.0) as u8,
            network_rx: if busy { 41_000_000 } else { 900_000 },
            network_tx: if busy { 9_000_000 } else { 240_000 },
            disk_utilization: if busy { 61 } else { 3 },
            temperature_c: Some(42 + ((t / 200.0).sin() * 2.0) as i64),
        });
    }
    trends
}

/// Paint a widget offscreen and write it out.
fn render(widget: &impl IsA<gtk::Widget>, width: i32, height: i32, path: &str) {
    let window = gtk::Window::builder()
        .default_width(width)
        .default_height(height)
        .child(widget)
        .build();
    window.set_titlebar(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.present();

    settle();
    snapshot(&window, width, height, path);
    window.destroy();
}

/// Run the main loop until there is nothing left to lay out.
fn settle() {
    let context = glib::MainContext::default();
    for _ in 0..100 {
        let mut worked = false;
        while context.iteration(false) {
            worked = true;
        }
        if !worked {
            break;
        }
    }
}

/// Paint a realised window into a PNG.
fn snapshot(window: &impl IsA<gtk::Widget>, width: i32, height: i32, path: &str) {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);

    let Some(node) = snapshot.to_node() else {
        eprintln!("{path}: nothing was drawn");
        return;
    };
    let renderer = gtk::gsk::CairoRenderer::new();
    renderer
        .realize(gtk::gdk::Surface::NONE)
        .expect("a renderer");
    let texture = renderer.render_texture(&node, None);
    texture.save_to_png(path).expect("write the png");
    renderer.unrealize();
}
