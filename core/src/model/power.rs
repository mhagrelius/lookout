//! Cooling and power: `SYNO.Core.Hardware.FanSpeed` and
//! `SYNO.Core.ExternalDevice.UPS`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::{as_bool, as_i64};

/// The fan policy DSM is running.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cooling {
    /// `coolfan`, `quietfan`, `lowtempfan` — DSM's own words for the profile.
    pub mode: Option<String>,
    /// Whether any disk's temperature reading has failed.
    pub disk_temperature_fault: bool,
}

impl Cooling {
    /// A phrase for the row. DSM's `dual_fan_speed` is the setting the Control
    /// Panel calls "Fan Speed Mode".
    pub fn mode_label(&self) -> String {
        match self.mode.as_deref() {
            Some("coolfan") => "Cool mode".into(),
            Some("quietfan") => "Quiet mode".into(),
            Some("lowtempfan") => "Low-temperature mode".into(),
            Some("fullfan") => "Full-speed mode".into(),
            Some(other) if !other.is_empty() => other.to_owned(),
            _ => "—".into(),
        }
    }

    pub fn from_json(data: &Value) -> Self {
        Cooling {
            mode: data
                .get("dual_fan_speed")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            // Sent as the string "yes"/"no", not a bool.
            disk_temperature_fault: data
                .get("all_disk_temp_fail")
                .and_then(as_bool)
                .unwrap_or(false),
        }
    }
}

/// An attached UPS, if there is one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ups {
    pub enabled: bool,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    /// Battery charge, 0–100.
    pub charge_percent: Option<i64>,
    /// Estimated runtime in seconds.
    pub runtime_seconds: Option<i64>,
    /// `usb`, `snmp`, `network`.
    pub mode: Option<String>,
}

impl Ups {
    pub fn from_json(data: &Value) -> Self {
        let text = |k: &str| {
            data.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        Ups {
            enabled: data.get("enable").and_then(as_bool).unwrap_or(false),
            manufacturer: text("manufacture"),
            model: text("model"),
            charge_percent: data.get("charge").and_then(as_i64),
            runtime_seconds: data.get("runtime").and_then(as_i64),
            mode: text("mode"),
        }
    }

    /// Runtime as a phrase.
    pub fn runtime_label(&self) -> String {
        match self.runtime_seconds {
            Some(s) if s > 0 => {
                let minutes = s / 60;
                if minutes >= 60 {
                    format!("{} h {} min", minutes / 60, minutes % 60)
                } else {
                    format!("{minutes} min")
                }
            }
            _ => "—".into(),
        }
    }

    /// Manufacturer and model as one phrase.
    pub fn description(&self) -> String {
        match (&self.manufacturer, &self.model) {
            (Some(m), Some(model)) => format!("{m} {model}"),
            (Some(m), None) => m.clone(),
            (None, Some(model)) => model.clone(),
            (None, None) => "Unknown UPS".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_fan_reply_reads_its_yes_no_strings_as_flags() {
        // Verbatim from the DS-series: these are strings, not bools.
        let c = Cooling::from_json(&json!({
            "all_disk_temp_fail": "no", "cool_fan": "yes",
            "dual_fan_speed": "coolfan", "fan_type": 11
        }));
        assert!(!c.disk_temperature_fault);
        assert_eq!(c.mode_label(), "Cool mode");
    }

    #[test]
    fn a_disk_temperature_fault_is_picked_up() {
        let c = Cooling::from_json(&json!({"all_disk_temp_fail": "yes"}));
        assert!(c.disk_temperature_fault);
    }

    #[test]
    fn an_unrecognised_fan_mode_is_shown_rather_than_hidden() {
        let c = Cooling::from_json(&json!({"dual_fan_speed": "somenewmode"}));
        assert_eq!(c.mode_label(), "somenewmode");
        assert_eq!(Cooling::from_json(&json!({})).mode_label(), "—");
    }

    #[test]
    fn a_ups_reads_end_to_end() {
        let u = Ups::from_json(&json!({
            "enable": true, "manufacture": "APC", "model": "Back-UPS 1500",
            "charge": 100, "runtime": 3900, "mode": "usb"
        }));
        assert!(u.enabled);
        assert_eq!(u.description(), "APC Back-UPS 1500");
        assert_eq!(u.charge_percent, Some(100));
        assert_eq!(u.runtime_label(), "1 h 5 min");
    }

    #[test]
    fn a_short_runtime_stays_in_minutes() {
        let u = Ups::from_json(&json!({"runtime": 900}));
        assert_eq!(u.runtime_label(), "15 min");
    }

    #[test]
    fn no_ups_is_absent_rather_than_a_zero_charge_battery() {
        // Rendering "0%" for a NAS with no UPS would look like a dead battery.
        let u = Ups::from_json(&json!({"enable": false}));
        assert!(!u.enabled);
        assert_eq!(u.runtime_label(), "—");
        assert_eq!(u.description(), "Unknown UPS");
    }
}
