//! The system log: `SYNO.Core.SyslogClient.Log`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::as_u64;

/// How loud an entry is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    #[default]
    Info,
    Warning,
    Error,
}

impl Severity {
    /// DSM writes these as `info`, `warn`, `err`.
    pub fn from_word(word: &str) -> Severity {
        match word.to_ascii_lowercase().as_str() {
            "err" | "error" | "crit" | "critical" => Severity::Error,
            "warn" | "warning" => Severity::Warning,
            _ => Severity::Info,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "Info",
            Severity::Warning => "Warning",
            Severity::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub severity: Severity,
    /// DSM's own formatting, `2026/08/04 10:09:51`. Kept as text: it arrives
    /// in the DiskStation's local zone with no offset, so parsing it to an
    /// instant would require guessing that zone and would be wrong for
    /// anyone whose NAS is not where they are.
    pub time: String,
    pub message: String,
    /// `System`, `Connection`, and so on.
    pub category: String,
    /// The account or `SYSTEM`.
    pub who: String,
}

/// A page of log entries, with the counts DSM reports alongside.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LogPage {
    pub entries: Vec<LogEntry>,
    pub total: u64,
    pub error_count: u64,
    pub warning_count: u64,
    pub info_count: u64,
}

impl LogPage {
    pub fn from_json(data: &Value) -> Self {
        let n = |k: &str| data.get(k).and_then(as_u64).unwrap_or(0);
        LogPage {
            entries: data
                .get("items")
                .and_then(Value::as_array)
                .map(|list| list.iter().map(entry_from).collect())
                .unwrap_or_default(),
            total: n("total"),
            error_count: n("errorCount"),
            warning_count: n("warnCount"),
            info_count: n("infoCount"),
        }
    }

    /// Entries at or above a severity, for the filter buttons.
    pub fn at_least(&self, floor: Severity) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| e.severity >= floor)
            .collect()
    }
}

fn entry_from(v: &Value) -> LogEntry {
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    LogEntry {
        severity: Severity::from_word(&text("level")),
        time: text("time"),
        message: text("descr"),
        category: text("logtype"),
        who: text("who"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "errorCount": 0, "warnCount": 1, "infoCount": 1, "total": 3206,
            "items": [
                {"descr": "Server back online.", "level": "info", "logtype": "System",
                 "orginalLogType": "system", "time": "2026/08/04 10:09:51", "who": "SYSTEM"},
                {"descr": "Volume degraded.", "level": "warn", "logtype": "System",
                 "time": "2026/08/04 10:10:00", "who": "SYSTEM"}
            ]
        })
    }

    #[test]
    fn a_page_reads_its_entries_and_its_counts() {
        let p = LogPage::from_json(&sample());
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.total, 3206);
        assert_eq!(p.warning_count, 1);
        assert_eq!(p.entries[0].message, "Server back online.");
        assert_eq!(p.entries[0].who, "SYSTEM");
    }

    #[test]
    fn dsm_abbreviates_severities_and_they_still_classify() {
        assert_eq!(Severity::from_word("err"), Severity::Error);
        assert_eq!(Severity::from_word("warn"), Severity::Warning);
        assert_eq!(Severity::from_word("info"), Severity::Info);
    }

    #[test]
    fn an_unknown_severity_reads_as_info_rather_than_error() {
        // Erring the other way would paint the log red over a word DSM
        // simply spells differently in some locale.
        assert_eq!(Severity::from_word("notice"), Severity::Info);
    }

    #[test]
    fn severities_order_so_the_filter_can_ask_for_at_least() {
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);

        let p = LogPage::from_json(&sample());
        assert_eq!(p.at_least(Severity::Warning).len(), 1);
        assert_eq!(p.at_least(Severity::Info).len(), 2);
    }

    #[test]
    fn the_timestamp_is_kept_verbatim_rather_than_guessed_into_a_zone() {
        // DSM sends no offset. Parsing it would mean assuming the NAS shares
        // the reader's timezone, which for a remote box is simply wrong.
        assert_eq!(
            LogPage::from_json(&sample()).entries[0].time,
            "2026/08/04 10:09:51"
        );
    }

    #[test]
    fn an_empty_reply_is_an_empty_page() {
        let p = LogPage::from_json(&json!({}));
        assert!(p.entries.is_empty());
        assert_eq!(p.total, 0);
    }
}
