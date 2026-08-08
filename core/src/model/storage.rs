//! Volumes, pools and physical drives: `SYNO.Storage.CGI.Storage`.
//!
//! Note the namespace. Several published references call this
//! `SYNO.Storage.CS.Storage`; no such API exists on DSM 7 — `SYNO.API.Info`
//! on a DS-series lists `SYNO.Storage.CGI.*` and nothing under `CS`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::{as_i64, as_u64};

/// Everything `load_info` returns, in one shape.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Storage {
    pub volumes: Vec<Volume>,
    pub disks: Vec<Disk>,
    pub pools: Vec<Pool>,
}

/// How healthy something is, collapsed from DSM's many status words.
///
/// DSM spells these differently per object and per version, so the raw word
/// is kept alongside for display and only the severity is interpreted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Health {
    Normal,
    Warning,
    Critical,
    #[default]
    Unknown,
}

impl Health {
    /// Classify a DSM status word.
    ///
    /// Exact matches first, then substrings — because `summary_status` is
    /// compound (`background_scrubbing`, `fs_normal`) while `status` is a
    /// bare word, and the same function has to read both. Matching only
    /// exactly leaves every scrubbing volume classified as Unknown.
    pub fn from_word(word: &str) -> Health {
        let word = word.to_ascii_lowercase();

        match word.as_str() {
            "normal" | "healthy" | "online" | "ok" | "fs_normal" => return Health::Normal,
            "attention" | "degrade" | "degraded" | "background" | "warning" | "expanding"
            | "migrating" => return Health::Warning,
            "crashed" | "critical" | "failed" | "error" | "unrecognized" => {
                return Health::Critical
            }
            _ => {}
        }

        // Critical before warning: `crashed_scrubbing` is crashed, not busy.
        if ["crashed", "critical", "failed", "error", "danger"]
            .iter()
            .any(|w| word.contains(w))
        {
            Health::Critical
        } else if [
            "scrubbing",
            "background",
            "expanding",
            "migrating",
            "degrade",
            "attention",
            "rebuilding",
            "repairing",
            "warning",
        ]
        .iter()
        .any(|w| word.contains(w))
        {
            Health::Warning
        } else if word.contains("normal") {
            Health::Normal
        } else {
            Health::Unknown
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Volume {
    pub id: String,
    /// DSM omits `display_name` on a single-volume box, so this falls back to
    /// the id, which is what the DSM UI shows there too.
    pub name: String,
    pub filesystem: Option<String>,
    pub raid_type: Option<String>,
    pub status: String,
    pub health: Health,
    pub total_bytes: u64,
    pub used_bytes: u64,
    /// The pool this volume sits on, from `pool_path`. This is the link that
    /// makes the storage tree real rather than three flat lists: a volume's
    /// own `disks` array is empty when it is backed by a pool.
    pub pool_id: Option<String>,
    /// DSM's richer verdict — `background_scrubbing`, `fs_normal`. The plain
    /// `status` says "normal" while a scrub is running, so this is the one
    /// that tells you the volume is busy.
    pub summary_status: Option<String>,
    /// Where it is mounted, `/volume1`.
    pub mount_path: Option<String>,
}

impl Volume {
    pub fn free_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes)
    }

    /// Fraction used, 0.0–1.0. Zero-capacity volumes read as empty rather
    /// than dividing by zero.
    pub fn used_fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f64 / self.total_bytes as f64
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Disk {
    /// The kernel name, `sda`.
    pub id: String,
    /// The bay label DSM shows, "Drive 1".
    pub name: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub temperature_c: Option<i64>,
    pub status: String,
    pub health: Health,
    pub smart_status: Option<String>,
    pub smart_health: Health,
    pub size_bytes: u64,
    /// What the drive is allocated to, e.g. `reuse_1`.
    pub used_by: Option<String>,
    pub disk_type: Option<String>,
}

impl Disk {
    /// Whether the drive is running hot enough to say so.
    ///
    /// 45 °C is where the handoff colours the cell; it is a display
    /// threshold, not a manufacturer limit.
    pub fn is_hot(&self) -> bool {
        self.temperature_c.is_some_and(|t| t >= 45)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pool {
    pub id: String,
    pub raid_type: Option<String>,
    pub status: String,
    pub health: Health,
    pub total_bytes: u64,
    pub used_bytes: u64,
    /// The drives that make it up, by kernel name. The pool carries these;
    /// the volume does not.
    pub disk_ids: Vec<String>,
}

impl Storage {
    /// Read the reply to `SYNO.Storage.CGI.Storage`/`load_info`.
    pub fn from_json(data: &Value) -> Self {
        Storage {
            volumes: array(data, "volumes").iter().map(volume_from).collect(),
            disks: array(data, "disks").iter().map(disk_from).collect(),
            pools: array(data, "storagePools").iter().map(pool_from).collect(),
        }
    }

    /// The volumes sitting on a pool.
    pub fn volumes_in(&self, pool: &Pool) -> Vec<&Volume> {
        self.volumes
            .iter()
            .filter(|v| v.pool_id.as_deref() == Some(pool.id.as_str()))
            .collect()
    }

    /// The drives making up a pool.
    ///
    /// Matches on the pool's own `disks` list first, falling back to each
    /// disk's `used_by`. Both are present on DSM 7.3 and they agree; the
    /// fallback covers a DSM that sends only one of them.
    pub fn disks_in(&self, pool: &Pool) -> Vec<&Disk> {
        self.disks
            .iter()
            .filter(|d| {
                pool.disk_ids.contains(&d.id) || d.used_by.as_deref() == Some(pool.id.as_str())
            })
            .collect()
    }

    /// Drives belonging to no pool at all — spares, or a drive DSM has not
    /// allocated. They would otherwise vanish from a pool-grouped view.
    pub fn unassigned_disks(&self) -> Vec<&Disk> {
        self.disks
            .iter()
            .filter(|d| {
                !self
                    .pools
                    .iter()
                    .any(|p| p.disk_ids.contains(&d.id) || d.used_by.as_deref() == Some(&p.id))
            })
            .collect()
    }

    /// The worst health across everything, which is what the banner pill
    /// reports.
    pub fn worst_health(&self) -> Health {
        // Unknown sorts last in the enum but is not worse than Critical, so
        // rank explicitly rather than leaning on Ord.
        let rank = |h: Health| match h {
            Health::Normal => 0,
            Health::Unknown => 1,
            Health::Warning => 2,
            Health::Critical => 3,
        };
        self.volumes
            .iter()
            .map(|v| v.health)
            .chain(self.disks.iter().map(|d| d.health))
            .chain(self.disks.iter().map(|d| d.smart_health))
            .chain(self.pools.iter().map(|p| p.health))
            .max_by_key(|h| rank(*h))
            .unwrap_or(Health::Unknown)
    }
}

fn array<'a>(data: &'a Value, key: &str) -> &'a [Value] {
    data.get(key).and_then(Value::as_array).map_or(&[], |v| v)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn size(v: &Value, key: &str) -> u64 {
    // Byte counts arrive as quoted strings because they run past 2^53.
    v.get("size")
        .and_then(|s| s.get(key))
        .and_then(as_u64)
        .unwrap_or(0)
}

fn volume_from(v: &Value) -> Volume {
    let id = text(v, "id").unwrap_or_default();
    let status = text(v, "status").unwrap_or_default();
    let summary = text(v, "summary_status");
    Volume {
        name: text(v, "display_name").unwrap_or_else(|| id.clone()),
        id,
        filesystem: text(v, "fs_type"),
        raid_type: text(v, "device_type").or_else(|| text(v, "raidType")),
        // A scrubbing volume is healthy but busy; the summary says so and the
        // plain status does not.
        health: Health::from_word(summary.as_deref().unwrap_or(&status)),
        status,
        total_bytes: size(v, "total"),
        used_bytes: size(v, "used"),
        pool_id: text(v, "pool_path"),
        summary_status: summary,
        mount_path: text(v, "vol_path"),
    }
}

fn disk_from(v: &Value) -> Disk {
    let status = text(v, "status").unwrap_or_default();
    let smart = text(v, "smart_status");
    let id = text(v, "id").unwrap_or_default();
    Disk {
        name: text(v, "name").unwrap_or_else(|| id.clone()),
        id,
        model: text(v, "model"),
        serial: text(v, "serial"),
        firmware: text(v, "firm"),
        temperature_c: v.get("temp").and_then(as_i64),
        health: Health::from_word(&status),
        status,
        smart_health: smart.as_deref().map_or(Health::Unknown, Health::from_word),
        smart_status: smart,
        // A disk's capacity is a flat `size_total`, not nested under `size`
        // the way a volume's is.
        size_bytes: v.get("size_total").and_then(as_u64).unwrap_or(0),
        used_by: text(v, "used_by"),
        disk_type: text(v, "disk_type"),
    }
}

fn pool_from(v: &Value) -> Pool {
    let status = text(v, "status").unwrap_or_default();
    Pool {
        id: text(v, "id").unwrap_or_default(),
        raid_type: text(v, "device_type").or_else(|| text(v, "raidType")),
        health: Health::from_word(&status),
        status,
        total_bytes: size(v, "total"),
        used_bytes: size(v, "used"),
        disk_ids: v
            .get("disks")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Verbatim from the DS-series: one btrfs volume, Seagate IronWolf drives.
    fn sample() -> Value {
        json!({
            "volumes": [{
                "id": "volume_1",
                "device_type": "shr_1",
                "fs_type": "btrfs",
                "status": "normal",
                "summary_status": "background_scrubbing",
                "pool_path": "reuse_1",
                "vol_path": "/volume1",
                "disks": [],
                "size": {"total": "28770439729152", "used": "14604487745536",
                         "total_device": "29969208049664"}
            }],
            "disks": [
                {"id": "sda", "name": "Drive 1", "model": "ST10000VN0008-2PJ103",
                 "temp": 31, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"},
                {"id": "sdb", "name": "Drive 2", "model": "ST10000VN0008-2PJ103",
                 "temp": 47, "smart_status": "normal", "status": "normal",
                 "size_total": "10000831348736", "used_by": "reuse_1"}
            ],
            "storagePools": [{
                "id": "reuse_1", "device_type": "shr_1", "status": "normal",
                "disks": ["sda", "sdb"],
                "size": {"total": "29969208049664", "used": "14604487745536"}
            }]
        })
    }

    #[test]
    fn byte_counts_past_two_to_the_fifty_third_survive_as_strings() {
        // 28.7 TB does not fit in an f64 mantissa exactly and does not arrive
        // as a JSON number at all. Parsing it as one loses the volume.
        let s = Storage::from_json(&sample());
        assert_eq!(s.volumes[0].total_bytes, 28_770_439_729_152);
        assert_eq!(s.volumes[0].used_bytes, 14_604_487_745_536);
        assert_eq!(
            s.volumes[0].free_bytes(),
            28_770_439_729_152 - 14_604_487_745_536
        );
    }

    #[test]
    fn a_volume_with_no_display_name_falls_back_to_its_id() {
        // DSM omits display_name on a single-volume box.
        assert_eq!(Storage::from_json(&sample()).volumes[0].name, "volume_1");
    }

    #[test]
    fn a_disks_capacity_is_flat_while_a_volumes_is_nested() {
        // Reading `size.total` on a disk yields 0 and a drive that looks
        // empty; they genuinely differ in shape.
        assert_eq!(
            Storage::from_json(&sample()).disks[0].size_bytes,
            10_000_831_348_736
        );
    }

    #[test]
    fn used_fraction_is_a_ratio_and_survives_an_empty_volume() {
        let s = Storage::from_json(&sample());
        assert!((s.volumes[0].used_fraction() - 0.5076).abs() < 0.001);

        let empty = Storage::from_json(&json!({"volumes": [{"id": "v", "status": "normal"}]}));
        assert_eq!(empty.volumes[0].used_fraction(), 0.0);
    }

    #[test]
    fn a_drive_at_forty_five_is_hot_and_one_below_is_not() {
        let s = Storage::from_json(&sample());
        assert!(!s.disks[0].is_hot());
        assert!(s.disks[1].is_hot());
    }

    #[test]
    fn dsm_status_words_map_onto_a_severity() {
        assert_eq!(Health::from_word("normal"), Health::Normal);
        assert_eq!(Health::from_word("degrade"), Health::Warning);
        assert_eq!(Health::from_word("crashed"), Health::Critical);
        // A word nobody has seen before must not read as healthy.
        assert_eq!(Health::from_word("reshaping-sideways"), Health::Unknown);
    }

    #[test]
    fn a_pool_rebuilding_is_a_warning_not_a_clean_bill() {
        assert_eq!(Health::from_word("background"), Health::Warning);
    }

    #[test]
    fn the_worst_health_is_what_the_banner_reports() {
        let mut s = Storage::from_json(&sample());
        // The sample volume is mid-scrub, so the honest verdict is Warning.
        assert_eq!(s.worst_health(), Health::Warning);

        s.disks[1].smart_health = Health::Critical;
        assert_eq!(s.worst_health(), Health::Critical);
    }

    #[test]
    fn compound_summary_statuses_classify_rather_than_falling_through() {
        assert_eq!(Health::from_word("background_scrubbing"), Health::Warning);
        assert_eq!(Health::from_word("fs_normal"), Health::Normal);
        assert_eq!(Health::from_word("volume_rebuilding"), Health::Warning);
        // Critical wins over the busy words when both appear.
        assert_eq!(Health::from_word("crashed_scrubbing"), Health::Critical);
    }

    #[test]
    fn unknown_does_not_outrank_a_real_failure() {
        // Ord on the enum puts Unknown last; the ranking must not.
        let s = Storage {
            volumes: vec![Volume {
                health: Health::Critical,
                ..Volume::default()
            }],
            disks: vec![Disk {
                health: Health::Unknown,
                ..Disk::default()
            }],
            pools: vec![],
        };
        assert_eq!(s.worst_health(), Health::Critical);
    }

    #[test]
    fn a_volume_knows_which_pool_it_sits_on() {
        // Its own `disks` array is empty; `pool_path` is the only link, and
        // without it the storage tree is three unrelated lists.
        let s = Storage::from_json(&sample());
        assert_eq!(s.volumes[0].pool_id.as_deref(), Some("reuse_1"));
        assert_eq!(s.volumes[0].mount_path.as_deref(), Some("/volume1"));
    }

    #[test]
    fn a_pool_gathers_its_volumes_and_its_drives() {
        let s = Storage::from_json(&sample());
        let pool = &s.pools[0];
        assert_eq!(s.volumes_in(pool).len(), 1);
        assert_eq!(s.disks_in(pool).len(), 2);
    }

    #[test]
    fn drives_are_matched_by_used_by_when_the_pool_lists_none() {
        // Belt and braces: DSM 7.3 sends both, but only one is needed.
        let s = Storage::from_json(&json!({
            "disks": [{"id": "sdz", "name": "Drive 9", "used_by": "reuse_1"}],
            "storagePools": [{"id": "reuse_1", "status": "normal"}]
        }));
        assert_eq!(s.disks_in(&s.pools[0]).len(), 1);
    }

    #[test]
    fn a_drive_in_no_pool_is_still_reachable() {
        // Otherwise a spare vanishes from a pool-grouped view entirely.
        let s = Storage::from_json(&json!({
            "disks": [
                {"id": "sda", "used_by": "reuse_1"},
                {"id": "sdz", "used_by": ""}
            ],
            "storagePools": [{"id": "reuse_1", "status": "normal", "disks": ["sda"]}]
        }));
        let loose = s.unassigned_disks();
        assert_eq!(loose.len(), 1);
        assert_eq!(loose[0].id, "sdz");
    }

    #[test]
    fn a_scrubbing_volume_is_busy_rather_than_plainly_normal() {
        // `status` says "normal" throughout a scrub; `summary_status` is the
        // field that says the volume is working.
        let s = Storage::from_json(&sample());
        assert_eq!(
            s.volumes[0].summary_status.as_deref(),
            Some("background_scrubbing")
        );
        assert_eq!(s.volumes[0].health, Health::Warning);
    }

    #[test]
    fn an_empty_reply_yields_nothing_rather_than_panicking() {
        let s = Storage::from_json(&json!({}));
        assert!(s.volumes.is_empty() && s.disks.is_empty() && s.pools.is_empty());
        assert_eq!(s.worst_health(), Health::Unknown);
    }
}
