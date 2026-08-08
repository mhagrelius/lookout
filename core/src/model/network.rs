//! Network interfaces: `SYNO.Core.Network.Interface`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::{as_bool, as_u64};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// `ovs_eth0`, `bond0`, `eth1`.
    pub name: String,
    pub ip: Option<String>,
    pub netmask: Option<String>,
    /// Link speed in Mbit/s. `0` when the link is down.
    pub speed_mbit: u64,
    /// `connected`, `disconnected`.
    pub status: String,
    /// `ovseth`, `bond`, `eth`.
    pub kind: String,
    pub dhcp: bool,
}

impl NetworkInterface {
    pub fn is_connected(&self) -> bool {
        self.status.eq_ignore_ascii_case("connected")
    }

    /// The link speed as a phrase: DSM reports 10 GbE as `10000`.
    pub fn speed_label(&self) -> String {
        match self.speed_mbit {
            0 => "—".into(),
            s if s >= 1000 && s % 1000 == 0 => format!("{} GbE", s / 1000),
            s => format!("{s} Mbit/s"),
        }
    }

    /// Read `SYNO.Core.Network.Interface`/`list`.
    ///
    /// The reply is an **object keyed by index** — `{"0": {...}, "1": {...}}`
    /// — not an array, so a straightforward `as_array` yields nothing.
    pub fn list_from_json(data: &Value) -> Vec<NetworkInterface> {
        let Some(obj) = data.as_object() else {
            return Vec::new();
        };

        // Sorted numerically by the key so the order matches DSM's, rather
        // than the string order a map iteration gives ("10" before "2").
        let mut entries: Vec<(u64, &Value)> = obj
            .iter()
            .filter(|(_, v)| v.is_object())
            .map(|(k, v)| (k.parse::<u64>().unwrap_or(u64::MAX), v))
            .collect();
        entries.sort_by_key(|(index, _)| *index);

        entries
            .into_iter()
            .map(|(_, v)| interface_from(v))
            .collect()
    }
}

fn interface_from(v: &Value) -> NetworkInterface {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
    NetworkInterface {
        name: text("ifname").unwrap_or_default(),
        ip: text("ip").filter(|s| !s.is_empty()),
        netmask: text("mask").filter(|s| !s.is_empty()),
        speed_mbit: v.get("speed").and_then(as_u64).unwrap_or(0),
        status: text("status").unwrap_or_default(),
        kind: text("type").unwrap_or_default(),
        dhcp: v.get("use_dhcp").and_then(as_bool).unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Verbatim from the DS-series.
    fn sample() -> Value {
        json!({
            "0": {"ifname": "ovs_eth0", "ip": "192.0.2.4", "mask": "255.255.255.0",
                  "speed": 10000, "status": "connected", "type": "ovseth", "use_dhcp": false},
            "1": {"ifname": "eth1", "ip": "", "mask": "",
                  "speed": 0, "status": "disconnected", "type": "eth", "use_dhcp": true}
        })
    }

    #[test]
    fn interfaces_arrive_keyed_by_index_not_as_an_array() {
        // `as_array` on this yields nothing and an empty Network page.
        let ns = NetworkInterface::list_from_json(&sample());
        assert_eq!(ns.len(), 2);
        assert_eq!(ns[0].name, "ovs_eth0");
        assert_eq!(ns[0].ip.as_deref(), Some("192.0.2.4"));
        assert!(ns[0].is_connected());
        assert!(!ns[1].is_connected());
    }

    #[test]
    fn a_disconnected_interface_reports_no_address_rather_than_an_empty_string() {
        let ns = NetworkInterface::list_from_json(&sample());
        assert_eq!(ns[1].ip, None);
        assert_eq!(ns[1].netmask, None);
    }

    #[test]
    fn ten_gigabit_reads_as_gbe_rather_than_ten_thousand() {
        let ns = NetworkInterface::list_from_json(&sample());
        assert_eq!(ns[0].speed_label(), "10 GbE");
        assert_eq!(ns[1].speed_label(), "—");
    }

    #[test]
    fn an_odd_speed_keeps_its_own_unit() {
        let ns = NetworkInterface::list_from_json(&json!({"0": {"speed": 100}}));
        assert_eq!(ns[0].speed_label(), "100 Mbit/s");
    }

    #[test]
    fn interfaces_come_back_in_numeric_key_order() {
        // String ordering would put "10" before "2".
        let ns = NetworkInterface::list_from_json(&json!({
            "10": {"ifname": "eth10"},
            "2":  {"ifname": "eth2"},
            "1":  {"ifname": "eth1"}
        }));
        let names: Vec<&str> = ns.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["eth1", "eth2", "eth10"]);
    }

    #[test]
    fn an_empty_reply_is_an_empty_list() {
        assert!(NetworkInterface::list_from_json(&json!({})).is_empty());
        assert!(NetworkInterface::list_from_json(&json!(null)).is_empty());
    }
}
