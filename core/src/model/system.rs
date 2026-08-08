//! Box identity and health: `SYNO.Core.System`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::{as_bool, as_i64, as_u64};

/// What the DiskStation says about itself.
///
/// Every field is optional because DSM versions disagree about which they
/// send, and a missing serial number is not a reason to fail the page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemInfo {
    pub model: Option<String>,
    pub serial: Option<String>,
    pub firmware_version: Option<String>,
    pub firmware_date: Option<String>,
    pub uptime: Option<Duration>,
    /// System temperature in °C.
    pub temperature_c: Option<i64>,
    /// Whether DSM considers that temperature too high. Taken from whichever
    /// of its three spellings this version uses.
    pub temperature_warning: bool,
    pub cpu_vendor: Option<String>,
    pub cpu_series: Option<String>,
    pub cpu_cores: Option<u64>,
    /// Clock speed in MHz.
    pub cpu_clock_mhz: Option<u64>,
    /// Installed memory in MB.
    pub ram_mb: Option<u64>,
    pub ntp_enabled: bool,
    pub ntp_server: Option<String>,
    pub time_zone: Option<String>,
}

impl SystemInfo {
    /// Read the reply to `SYNO.Core.System`/`info`.
    pub fn from_json(data: &Value) -> Self {
        let s = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_owned);

        SystemInfo {
            model: s("model"),
            serial: s("serial"),
            firmware_version: s("firmware_ver"),
            firmware_date: s("firmware_date"),
            uptime: data.get("up_time").and_then(parse_uptime),
            // DSM 7.3 sends `sys_temp`. Older builds and the mobile API send
            // `temperature`. Neither is reliably present, so try both.
            temperature_c: data
                .get("sys_temp")
                .or_else(|| data.get("temperature"))
                .and_then(as_i64),
            // Three spellings of the same flag ship in the same reply. Any
            // one of them being true is a warning.
            temperature_warning: ["sys_tempwarn", "systempwarn", "temperature_warning"]
                .iter()
                .filter_map(|k| data.get(*k))
                .filter_map(as_bool)
                .any(|b| b),
            cpu_vendor: s("cpu_vendor"),
            cpu_series: s("cpu_series"),
            // `cpu_cores` arrives as the string "4".
            cpu_cores: data.get("cpu_cores").and_then(as_u64),
            cpu_clock_mhz: data.get("cpu_clock_speed").and_then(as_u64),
            ram_mb: data.get("ram_size").and_then(as_u64),
            ntp_enabled: data.get("enabled_ntp").and_then(as_bool).unwrap_or(false),
            ntp_server: s("ntp_server"),
            time_zone: s("time_zone_desc").or_else(|| s("time_zone")),
        }
    }

    /// The CPU as one phrase, for the banner.
    pub fn cpu_description(&self) -> Option<String> {
        let series = self.cpu_series.as_deref()?;
        let vendor = self.cpu_vendor.as_deref().unwrap_or_default();
        let mut out = if vendor.is_empty() {
            series.to_owned()
        } else {
            // DSM shouts the vendor: "INTEL".
            format!("{} {}", titlecase(vendor), series)
        };
        if let Some(cores) = self.cpu_cores {
            out.push_str(&format!(" · {cores} cores"));
        }
        if let Some(mhz) = self.cpu_clock_mhz {
            out.push_str(&format!(" @ {:.2} GHz", mhz as f64 / 1000.0));
        }
        Some(out)
    }
}

fn titlecase(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

/// Parse DSM's uptime string.
///
/// On DSM 7.3 this is `H:MM:SS` with the hours running past 24 — a box up for
/// two and a half days reports `64:48:7`. Community type definitions describe
/// it as `DD:HH:MM:SS`, which is where a "64 days" display comes from; that
/// four-field form is accepted too, since it costs one arm of a match and
/// some DSM version may well produce it.
fn parse_uptime(value: &Value) -> Option<Duration> {
    let text = value.as_str()?;
    let parts: Vec<u64> = text
        .split(':')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;

    let seconds = match parts.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [d, h, m, s] => d * 86_400 + h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(Duration::from_secs(seconds))
}

/// Render a duration the way the banner wants it: "2 days, 16 hours".
pub fn format_uptime(d: Duration) -> String {
    let total = d.as_secs();
    let days = total / 86_400;
    let hours = (total % 86_400) / 3600;
    let minutes = (total % 3600) / 60;

    let plural = |n: u64, unit: &str| format!("{n} {unit}{}", if n == 1 { "" } else { "s" });

    if days > 0 {
        format!("{}, {}", plural(days, "day"), plural(hours, "hour"))
    } else if hours > 0 {
        format!("{}, {}", plural(hours, "hour"), plural(minutes, "minute"))
    } else {
        plural(minutes, "minute")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The reply this was written against, trimmed. Field names and spellings
    /// are verbatim from a DS-series on DSM 7.2.2-72806.
    fn nas() -> Value {
        json!({
            "cpu_clock_speed": 2200,
            "cpu_cores": "4",
            "cpu_family": "Xeon",
            "cpu_series": "D-1527",
            "cpu_vendor": "INTEL",
            "enabled_ntp": true,
            "firmware_date": "2026/06/18",
            "firmware_ver": "DSM 7.2.2-72806 Update 3",
            "model": "DS-series",
            "ntp_server": "pool.ntp.org",
            "ram_size": 32768,
            "serial": "REDACTED",
            "sys_temp": 42,
            "sys_tempwarn": false,
            "systempwarn": false,
            "temperature_warning": false,
            "time_zone_desc": "(GMT-05:00) Eastern Time",
            "up_time": "64:48:7"
        })
    }

    #[test]
    fn the_real_reply_reads_end_to_end() {
        let info = SystemInfo::from_json(&nas());
        assert_eq!(info.model.as_deref(), Some("DS-series"));
        assert_eq!(
            info.firmware_version.as_deref(),
            Some("DSM 7.2.2-72806 Update 3")
        );
        assert_eq!(info.ram_mb, Some(32768));
        assert_eq!(info.cpu_cores, Some(4));
        assert!(!info.temperature_warning);
    }

    #[test]
    fn temperature_comes_from_sys_temp_which_is_what_dsm_7_3_sends() {
        // Reading `temperature` alone yields None here, and a banner with no
        // temperature at all. That is the bug this test exists to hold shut.
        assert_eq!(SystemInfo::from_json(&nas()).temperature_c, Some(42));
    }

    #[test]
    fn temperature_falls_back_to_the_older_spelling() {
        let info = SystemInfo::from_json(&json!({"temperature": 43}));
        assert_eq!(info.temperature_c, Some(43));
    }

    #[test]
    fn any_of_the_three_warning_spellings_raises_the_warning() {
        for key in ["sys_tempwarn", "systempwarn", "temperature_warning"] {
            let info = SystemInfo::from_json(&json!({ key: true }));
            assert!(info.temperature_warning, "{key} should raise it");
        }
    }

    #[test]
    fn uptime_is_hours_minutes_seconds_not_days_hours_minutes_seconds() {
        // 64:48:7 is two and a half days, not sixty-four days. Getting this
        // wrong is invisible in review and glaring in the banner.
        let d = parse_uptime(&json!("64:48:7")).expect("should parse");
        assert_eq!(d.as_secs(), 64 * 3600 + 48 * 60 + 7);
        assert_eq!(format_uptime(d), "2 days, 16 hours");
    }

    #[test]
    fn the_four_field_form_is_read_as_days_first() {
        let d = parse_uptime(&json!("3:04:05:06")).expect("should parse");
        assert_eq!(d.as_secs(), 3 * 86_400 + 4 * 3600 + 5 * 60 + 6);
    }

    #[test]
    fn a_nonsense_uptime_is_absent_rather_than_zero() {
        // Zero would render "0 minutes", which reads as a box that just
        // rebooted — a worse lie than showing nothing.
        assert_eq!(parse_uptime(&json!("not a time")), None);
        assert_eq!(parse_uptime(&json!("1:2")), None);
        assert_eq!(parse_uptime(&json!(12345)), None);
    }

    #[test]
    fn uptime_renders_at_each_scale() {
        assert_eq!(format_uptime(Duration::from_secs(90)), "1 minute");
        assert_eq!(
            format_uptime(Duration::from_secs(3 * 3600 + 120)),
            "3 hours, 2 minutes"
        );
        assert_eq!(
            format_uptime(Duration::from_secs(86_400 + 3600)),
            "1 day, 1 hour"
        );
    }

    #[test]
    fn the_cpu_reads_as_one_phrase_with_the_vendor_no_longer_shouting() {
        let info = SystemInfo::from_json(&nas());
        assert_eq!(
            info.cpu_description().as_deref(),
            Some("Intel D-1527 · 4 cores @ 2.20 GHz")
        );
    }

    #[test]
    fn an_empty_reply_produces_an_empty_record_rather_than_a_panic() {
        let info = SystemInfo::from_json(&json!({}));
        assert_eq!(info, SystemInfo::default());
        assert_eq!(info.cpu_description(), None);
    }
}
