//! A point-in-time resource sample: `SYNO.Core.System.Utilization`.
//!
//! This is a snapshot, not an average, and DSM keeps no history of it — see
//! [`crate::trend`] for why the app records its own.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::{as_i64, as_u64};

/// One reading of everything the box is doing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Utilization {
    pub cpu: Cpu,
    pub memory: Memory,
    /// Per-interface throughput, plus the aggregate under the device name
    /// `total`.
    pub network: Vec<Interface>,
    /// Aggregate disk activity.
    pub disk: Option<Io>,
    /// Aggregate volume activity.
    pub space: Option<Io>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Cpu {
    /// Percentages, 0–100.
    pub user: u8,
    pub system: u8,
    pub other: u8,
    /// Load averages, already divided down from the hundredths DSM sends.
    pub load_1: f32,
    pub load_5: f32,
    pub load_15: f32,
}

impl Cpu {
    /// Total busy percentage, which is what the tile and the chart show.
    pub fn total(&self) -> u8 {
        // Saturating, then capped at 100. DSM normalises these across all
        // cores so they should sum to at most 100, but a busy box does
        // occasionally answer 101 — and a wrapped `u8` would render 0% at
        // exactly the moment the number mattered. The cap keeps the chart's
        // fixed 0–100 axis honest.
        self.user
            .saturating_add(self.system)
            .saturating_add(self.other)
            .min(100)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Percentage of real memory in use, as DSM computes it.
    pub usage_percent: u8,
    pub swap_percent: u8,
    /// All in kilobytes, which is the unit DSM uses throughout this reply.
    pub total_kb: u64,
    pub available_kb: u64,
    pub cached_kb: u64,
    pub buffer_kb: u64,
    pub total_swap_kb: u64,
}

impl Memory {
    pub fn used_kb(&self) -> u64 {
        self.total_kb.saturating_sub(self.available_kb)
    }
}

/// Throughput on one interface, in bytes per second.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Interface {
    pub device: String,
    pub rx: u64,
    pub tx: u64,
}

/// Read/write activity, for disks or for volumes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Io {
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    /// Percentage of time the device was busy.
    pub utilization: u8,
}

impl Utilization {
    /// Read the reply to `SYNO.Core.System.Utilization`/`get`.
    pub fn from_json(data: &Value) -> Self {
        Utilization {
            cpu: cpu_from(data.get("cpu")),
            memory: memory_from(data.get("memory")),
            network: data
                .get("network")
                .and_then(Value::as_array)
                .map(|list| list.iter().map(interface_from).collect())
                .unwrap_or_default(),
            disk: data.get("disk").and_then(|d| d.get("total")).map(io_from),
            space: data.get("space").and_then(|d| d.get("total")).map(io_from),
        }
    }

    /// The aggregate interface DSM reports under the name `total`, if it did.
    pub fn network_total(&self) -> Option<&Interface> {
        self.network.iter().find(|i| i.device == "total")
    }
}

fn pct(v: Option<&Value>) -> u8 {
    v.and_then(as_i64).unwrap_or(0).clamp(0, 100) as u8
}

fn cpu_from(v: Option<&Value>) -> Cpu {
    let Some(v) = v else { return Cpu::default() };
    // DSM sends load averages multiplied by 100: an idle box reporting
    // `"1min_load": 27` is at 0.27, not 27. Showing the raw number turns a
    // quiet NAS into an alarming one.
    let load = |k: &str| v.get(k).and_then(as_i64).unwrap_or(0) as f32 / 100.0;

    Cpu {
        user: pct(v.get("user_load")),
        system: pct(v.get("system_load")),
        other: pct(v.get("other_load")),
        load_1: load("1min_load"),
        load_5: load("5min_load"),
        load_15: load("15min_load"),
    }
}

fn memory_from(v: Option<&Value>) -> Memory {
    let Some(v) = v else { return Memory::default() };
    let kb = |k: &str| v.get(k).and_then(as_u64).unwrap_or(0);

    Memory {
        usage_percent: pct(v.get("real_usage")),
        swap_percent: pct(v.get("swap_usage")),
        total_kb: kb("total_real"),
        available_kb: kb("avail_real"),
        cached_kb: kb("cached"),
        buffer_kb: kb("buffer"),
        total_swap_kb: kb("total_swap"),
    }
}

fn interface_from(v: &Value) -> Interface {
    Interface {
        device: v
            .get("device")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        rx: v.get("rx").and_then(as_u64).unwrap_or(0),
        tx: v.get("tx").and_then(as_u64).unwrap_or(0),
    }
}

fn io_from(v: &Value) -> Io {
    let n = |k: &str| v.get(k).and_then(as_u64).unwrap_or(0);
    Io {
        read_bytes: n("read_byte"),
        write_bytes: n("write_byte"),
        read_ops: n("read_access"),
        write_ops: n("write_access"),
        utilization: pct(v.get("utilization")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Verbatim shape from the DS-series, values trimmed.
    fn sample() -> Value {
        json!({
            "cpu": {
                "15min_load": 2, "1min_load": 27, "5min_load": 10,
                "device": "System", "other_load": 1, "system_load": 0, "user_load": 0
            },
            "memory": {
                "avail_real": 24333872, "avail_swap": 21684140, "buffer": 85012,
                "cached": 5707596, "device": "Memory", "memory_size": 33554432,
                "real_usage": 7, "si_disk": 0, "so_disk": 0,
                "total_real": 32641796, "total_swap": 21684140, "swap_usage": 0
            },
            "network": [
                {"device": "total", "rx": 1024, "tx": 2048},
                {"device": "eth0", "rx": 1024, "tx": 2048}
            ],
            "disk": {"total": {"device": "total", "read_access": 0, "read_byte": 9478,
                               "utilization": 2, "write_access": 12, "write_byte": 4096}},
            "space": {"total": {"device": "total", "read_access": 0, "read_byte": 0,
                                "utilization": 3, "write_access": 35, "write_byte": 578560}}
        })
    }

    #[test]
    fn load_averages_are_hundredths_and_come_back_as_fractions() {
        // The whole point: 27 means 0.27. Rendering it raw would show a
        // load of 27 on an idle six-bay NAS.
        let u = Utilization::from_json(&sample());
        assert!((u.cpu.load_1 - 0.27).abs() < f32::EPSILON);
        assert!((u.cpu.load_5 - 0.10).abs() < f32::EPSILON);
        assert!((u.cpu.load_15 - 0.02).abs() < f32::EPSILON);
    }

    #[test]
    fn cpu_percentages_sum_to_the_total() {
        let u = Utilization::from_json(&sample());
        assert_eq!(u.cpu.total(), 1);
    }

    #[test]
    fn a_cpu_summing_past_one_hundred_is_capped_rather_than_wrapping() {
        // 240 would be a nonsense percentage; a wrapped u8 would be 0, which
        // is worse — it reads as idle at the moment the box is busiest.
        let u = Utilization::from_json(&json!({
            "cpu": {"user_load": 80, "system_load": 80, "other_load": 80}
        }));
        assert_eq!(u.cpu.total(), 100);
    }

    #[test]
    fn memory_is_kilobytes_and_used_is_total_less_available() {
        let u = Utilization::from_json(&sample());
        assert_eq!(u.memory.total_kb, 32_641_796);
        assert_eq!(u.memory.used_kb(), 32_641_796 - 24_333_872);
        assert_eq!(u.memory.usage_percent, 7);
    }

    #[test]
    fn available_memory_above_total_does_not_underflow_used() {
        // Never seen, but `used_kb` subtracting u64s is one bad reply away
        // from a number near 18 quintillion in the tile.
        let u = Utilization::from_json(&json!({
            "memory": {"total_real": 100, "avail_real": 200}
        }));
        assert_eq!(u.memory.used_kb(), 0);
    }

    #[test]
    fn the_aggregate_interface_is_found_by_name() {
        let u = Utilization::from_json(&sample());
        let total = u.network_total().expect("total should be present");
        assert_eq!(total.rx, 1024);
        assert_eq!(u.network.len(), 2);
    }

    #[test]
    fn disk_and_volume_activity_are_read_separately() {
        let u = Utilization::from_json(&sample());
        assert_eq!(u.disk.expect("disk").read_bytes, 9478);
        assert_eq!(u.space.expect("space").write_bytes, 578_560);
        assert_eq!(u.space.expect("space").utilization, 3);
    }

    #[test]
    fn an_empty_reply_is_all_zeroes_and_no_panic() {
        let u = Utilization::from_json(&json!({}));
        assert_eq!(u, Utilization::default());
        assert!(u.network_total().is_none());
    }

    #[test]
    fn out_of_range_percentages_are_clamped() {
        let u = Utilization::from_json(&json!({"cpu": {"user_load": 250, "system_load": -5}}));
        assert_eq!(u.cpu.user, 100);
        assert_eq!(u.cpu.system, 0);
    }
}
