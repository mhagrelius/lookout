//! The generic detail pages, built from [`TablePage`].
//!
//! Each is a function that builds the page and one that fills it from a
//! [`Snapshot`]. Keeping the filling separate from the building is what lets
//! the window construct all of them once and feed them on every poll.

use lookout_core::model::{format_uptime, NetworkInterface, Package, Session, Share};
use lookout_core::poll::Snapshot;

use crate::ui::table_page::{Row, Stat, TablePage};
use crate::ui::widgets::{format_bytes, format_memory_kb};

/// System information — hardware and time, as key/value rows.
pub fn system_page() -> TablePage {
    TablePage::new(
        "System information",
        "system",
        &["MODEL", "DSM", "UPTIME", "TEMPERATURE"],
        &[
            ("Hardware", "SYNO.Core.System"),
            ("Time", "SYNO.Core.System"),
        ],
    )
}

pub fn fill_system(page: &TablePage, snapshot: &Snapshot) {
    let Some(system) = &snapshot.system else {
        page.set_unavailable(0);
        page.set_unavailable(1);
        return;
    };

    let dash = || "—".to_string();
    page.set_stats(&[
        Stat::new(system.model.clone().unwrap_or_else(dash), "model"),
        Stat::new(
            short_dsm(system.firmware_version.as_deref()),
            system.firmware_date.clone().unwrap_or_else(dash),
        ),
        Stat::new(
            system.uptime.map(format_uptime).unwrap_or_else(dash),
            "since last boot",
        ),
        Stat::new(
            system
                .temperature_c
                .map(|t| format!("{t} °C"))
                .unwrap_or_else(dash),
            "system",
        )
        .warn(system.temperature_warning),
    ]);

    page.set_rows(
        0,
        vec![
            Row::new("Model", system.model.clone().unwrap_or_else(dash)),
            Row::new("Serial number", system.serial.clone().unwrap_or_else(dash)),
            Row::new("DSM", system.firmware_version.clone().unwrap_or_else(dash)),
            Row::new("CPU", system.cpu_description().unwrap_or_else(dash)),
            Row::new(
                "Memory",
                system
                    .ram_mb
                    .map(|mb| format_memory_kb(mb * 1024))
                    .unwrap_or_else(dash),
            ),
        ],
    );

    page.set_rows(
        1,
        vec![
            Row::new("Time zone", system.time_zone.clone().unwrap_or_else(dash)),
            Row::new(
                "NTP",
                if system.ntp_enabled {
                    system
                        .ntp_server
                        .clone()
                        .unwrap_or_else(|| "enabled".into())
                } else {
                    "disabled".into()
                },
            )
            .badge(
                if system.ntp_enabled { "On" } else { "Off" },
                if system.ntp_enabled {
                    "success"
                } else {
                    "dim-label"
                },
            ),
        ],
    );
}

/// "DSM 7.2.2-72806 Update 3" is too wide for a tile; the release is enough.
fn short_dsm(version: Option<&str>) -> String {
    let Some(version) = version else {
        return "—".into();
    };
    version
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.split('-').next())
        .map(str::to_owned)
        .unwrap_or_else(|| version.to_owned())
}

/// Packages.
pub fn packages_page() -> TablePage {
    TablePage::new(
        "Packages",
        "packages",
        &["INSTALLED", "RUNNING", "STOPPED", "UPDATES"],
        &[("Installed packages", "SYNO.Core.Package")],
    )
}

pub fn fill_packages(page: &TablePage, snapshot: &Snapshot) {
    let Some(packages) = &snapshot.packages else {
        page.set_unavailable(0);
        return;
    };

    let running = packages.iter().filter(|p| p.is_running()).count();
    let updates = packages.iter().filter(|p| p.has_update()).count();
    page.set_stats(&[
        Stat::new(packages.len().to_string(), "packages"),
        Stat::new(running.to_string(), "running"),
        Stat::new((packages.len() - running).to_string(), "stopped"),
        Stat::new(updates.to_string(), "available").warn(updates > 0),
    ]);

    page.set_rows(0, packages.iter().map(package_row).collect());
}

fn package_row(p: &Package) -> Row {
    let row = Row::new(&p.name, format!("{} · {}", p.version, p.id));
    if p.has_update() {
        // The accent, not a warning: an update is an invitation, not a fault.
        row.badge(
            format!(
                "Update to {}",
                p.available_version.as_deref().unwrap_or_default()
            ),
            "accent",
        )
    } else if p.is_running() {
        row.badge("Running", "success")
    } else {
        row.badge("Stopped", "dim-label")
    }
}

/// Shared folders.
pub fn shares_page() -> TablePage {
    TablePage::new(
        "Shared folders",
        "shares",
        &["SHARES", "USED", "QUOTAS", "HIDDEN"],
        &[("Shared folders", "SYNO.Core.Share")],
    )
}

pub fn fill_shares(page: &TablePage, snapshot: &Snapshot) {
    let Some(shares) = &snapshot.shares else {
        page.set_unavailable(0);
        return;
    };

    let used: u64 = shares.iter().map(|s| s.used_bytes).sum();
    let quotas = shares.iter().filter(|s| s.quota_bytes.is_some()).count();
    let hidden = shares.iter().filter(|s| s.hidden).count();
    page.set_stats(&[
        Stat::new(shares.len().to_string(), "shares"),
        Stat::new(format_bytes(used), "across all shares"),
        Stat::new(quotas.to_string(), "with a quota"),
        Stat::new(hidden.to_string(), "hidden"),
    ]);

    page.set_rows(0, shares.iter().map(share_row).collect());
}

fn share_row(s: &Share) -> Row {
    let subtitle = match s.quota_bytes {
        Some(quota) => format!(
            "{} of {} · {}",
            format_bytes(s.used_bytes),
            format_bytes(quota),
            s.volume_path
        ),
        None => format!(
            "{} · no quota · {}",
            format_bytes(s.used_bytes),
            s.volume_path
        ),
    };
    let row = Row::new(&s.name, subtitle);
    match s.used_fraction() {
        // Only a share with a quota can be near full, so only it gets a badge.
        Some(f) if f >= 0.9 => row.badge("Nearly full", "warning"),
        _ if s.hidden => row.badge("Hidden", "dim-label"),
        _ => row,
    }
}

/// Users & sessions.
pub fn sessions_page() -> TablePage {
    TablePage::new(
        "Users & sessions",
        "sessions",
        &["CONNECTED", "SERVICES", "ACCOUNTS", "THIS MACHINE"],
        &[("Active sessions", "SYNO.Core.CurrentConnection")],
    )
}

pub fn fill_sessions(page: &TablePage, snapshot: &Snapshot) {
    let Some(sessions) = &snapshot.sessions else {
        page.set_unavailable(0);
        return;
    };

    let mut services: Vec<&str> = sessions.iter().map(|s| s.service.as_str()).collect();
    services.sort_unstable();
    services.dedup();
    let mut accounts: Vec<&str> = sessions.iter().map(|s| s.who.as_str()).collect();
    accounts.sort_unstable();
    accounts.dedup();

    page.set_stats(&[
        Stat::new(sessions.len().to_string(), "connections"),
        Stat::new(services.len().to_string(), "services"),
        Stat::new(accounts.len().to_string(), "accounts"),
        Stat::new(
            sessions.iter().filter(|s| s.is_current).count().to_string(),
            "this session",
        ),
    ]);

    page.set_rows(0, sessions.iter().map(session_row).collect());
}

fn session_row(s: &Session) -> Row {
    let row = Row::new(
        &s.who,
        format!("{} · {} · since {}", s.service, s.from, s.since),
    );
    if s.is_current {
        row.badge("This session", "accent")
    } else {
        row
    }
}

/// Network.
pub fn network_page() -> TablePage {
    TablePage::new(
        "Network",
        "network",
        &["INTERFACES", "CONNECTED", "LINK", "ADDRESS"],
        &[("Interfaces", "SYNO.Core.Network.Interface")],
    )
}

pub fn fill_network(page: &TablePage, snapshot: &Snapshot) {
    let Some(interfaces) = &snapshot.interfaces else {
        page.set_unavailable(0);
        return;
    };

    let up: Vec<&NetworkInterface> = interfaces.iter().filter(|i| i.is_connected()).collect();
    // The fastest connected link is the one worth reporting: a NAS with a
    // 10 GbE port and three idle gigabit ones is a 10 GbE NAS.
    let fastest = up.iter().max_by_key(|i| i.speed_mbit);

    page.set_stats(&[
        Stat::new(interfaces.len().to_string(), "interfaces"),
        Stat::new(up.len().to_string(), "connected").warn(up.is_empty()),
        Stat::new(
            fastest
                .map(|i| i.speed_label())
                .unwrap_or_else(|| "—".into()),
            "fastest link",
        ),
        Stat::new(
            fastest
                .and_then(|i| i.ip.clone())
                .unwrap_or_else(|| "—".into()),
            "primary address",
        ),
    ]);

    page.set_rows(0, interfaces.iter().map(interface_row).collect());
}

fn interface_row(i: &NetworkInterface) -> Row {
    let address = match (&i.ip, &i.netmask) {
        (Some(ip), Some(mask)) => format!("{ip} / {mask}"),
        (Some(ip), None) => ip.clone(),
        _ => "no address".into(),
    };
    let row = Row::new(
        &i.name,
        format!(
            "{address} · {} · {}",
            i.speed_label(),
            if i.dhcp { "DHCP" } else { "static" }
        ),
    );
    if i.is_connected() {
        row.badge("Connected", "success")
    } else {
        row.badge("Disconnected", "dim-label")
    }
}

/// Temperature & power.
pub fn power_page() -> TablePage {
    TablePage::new(
        "Temperature & power",
        "power",
        &["SYSTEM", "DRIVES", "FAN", "UPS"],
        &[
            ("Cooling", "SYNO.Core.Hardware.FanSpeed"),
            ("UPS", "SYNO.Core.ExternalDevice.UPS"),
        ],
    )
}

pub fn fill_power(page: &TablePage, snapshot: &Snapshot) {
    let dash = || "—".to_string();

    let system_temp = snapshot
        .system
        .as_ref()
        .and_then(|s| s.temperature_c)
        .map(|t| format!("{t} °C"))
        .unwrap_or_else(dash);
    let temp_warning = snapshot
        .system
        .as_ref()
        .is_some_and(|s| s.temperature_warning);

    // The hottest drive, since that is the one that fails first.
    let hottest = snapshot
        .storage
        .as_ref()
        .and_then(|s| s.disks.iter().filter_map(|d| d.temperature_c).max());
    let hot_count = snapshot
        .storage
        .as_ref()
        .map(|s| s.disks.iter().filter(|d| d.is_hot()).count())
        .unwrap_or(0);

    page.set_stats(&[
        Stat::new(system_temp, "system").warn(temp_warning),
        Stat::new(
            hottest.map(|t| format!("{t} °C")).unwrap_or_else(dash),
            if hot_count > 0 {
                format!("{hot_count} running warm")
            } else {
                "hottest drive".into()
            },
        )
        .warn(hot_count > 0),
        Stat::new(
            snapshot
                .cooling
                .as_ref()
                .map(|c| c.mode_label())
                .unwrap_or_else(dash),
            "fan profile",
        ),
        Stat::new(
            match &snapshot.ups {
                Some(u) if u.enabled => u
                    .charge_percent
                    .map(|c| format!("{c}%"))
                    .unwrap_or_else(|| "On".into()),
                Some(_) => "None".into(),
                None => dash(),
            },
            "battery",
        ),
    ]);

    match &snapshot.cooling {
        Some(cooling) => page.set_rows(
            0,
            vec![
                Row::new("Fan speed mode", cooling.mode_label()),
                Row::new(
                    "Drive temperature sensors",
                    if cooling.disk_temperature_fault {
                        "A drive is not reporting its temperature"
                    } else {
                        "All reporting"
                    },
                )
                .badge(
                    if cooling.disk_temperature_fault {
                        "Fault"
                    } else {
                        "OK"
                    },
                    if cooling.disk_temperature_fault {
                        "warning"
                    } else {
                        "success"
                    },
                ),
            ],
        ),
        None => page.set_unavailable(0),
    }

    match &snapshot.ups {
        // A DiskStation with no UPS configured is not a failure, so it says so
        // rather than showing the section as unavailable.
        Some(ups) if !ups.enabled => {
            page.set_rows(1, vec![Row::new("No UPS", "None is configured in DSM")])
        }
        Some(ups) => page.set_rows(
            1,
            vec![
                Row::new("Device", ups.description()),
                Row::new(
                    "Battery",
                    ups.charge_percent
                        .map(|c| format!("{c}%"))
                        .unwrap_or_else(dash),
                )
                // Says what the charge is, not what the mains are doing.
                // `charge` does not distinguish "full and idle" from
                // "discharging", so a badge reading "On battery power" would
                // claim a power cut that has not happened.
                .badge(
                    if ups.charge_percent.is_some_and(|c| c < 50) {
                        "Low charge"
                    } else {
                        "Charged"
                    },
                    if ups.charge_percent.is_some_and(|c| c < 50) {
                        "warning"
                    } else {
                        "success"
                    },
                ),
                Row::new("Estimated runtime", ups.runtime_label()),
                Row::new("Connection", ups.mode.clone().unwrap_or_else(dash)),
            ],
        ),
        None => page.set_unavailable(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dsm_version_is_shortened_to_fit_a_tile() {
        assert_eq!(short_dsm(Some("DSM 7.2.2-72806 Update 3")), "7.2.2");
        assert_eq!(short_dsm(Some("DSM 7.2-64570")), "7.2");
    }

    #[test]
    fn an_unexpected_version_string_is_shown_rather_than_mangled() {
        assert_eq!(short_dsm(Some("weird")), "weird");
        assert_eq!(short_dsm(None), "—");
    }

    #[test]
    fn a_package_with_an_update_is_badged_as_an_invitation_not_a_fault() {
        let p = Package {
            name: "Synology Photos".into(),
            version: "1.7.0".into(),
            available_version: Some("1.8.0".into()),
            status: "running".into(),
            ..Package::default()
        };
        let (text, class) = package_row(&p).badge.expect("a badge");
        assert!(text.contains("1.8.0"));
        assert_eq!(class, "accent");
    }

    #[test]
    fn a_running_package_without_an_update_reads_as_running() {
        let p = Package {
            name: "Container Manager".into(),
            status: "running".into(),
            ..Package::default()
        };
        assert_eq!(package_row(&p).badge.expect("a badge").1, "success");
    }

    #[test]
    fn only_a_quotaed_share_can_be_flagged_as_nearly_full() {
        // A share with no quota has no fullness to report, and badging it
        // would be inventing a limit DSM does not have.
        let unquotaed = Share {
            name: "Games".into(),
            used_bytes: 13_000_000_000_000,
            quota_bytes: None,
            ..Share::default()
        };
        assert!(share_row(&unquotaed).badge.is_none());

        let full = Share {
            name: "homes".into(),
            used_bytes: 95,
            quota_bytes: Some(100),
            ..Share::default()
        };
        assert_eq!(share_row(&full).badge.expect("a badge").1, "warning");
    }

    #[test]
    fn our_own_session_is_marked_so_it_is_recognisable() {
        let s = Session {
            who: "admin".into(),
            is_current: true,
            ..Session::default()
        };
        assert_eq!(session_row(&s).badge.expect("a badge").0, "This session");
    }
}
