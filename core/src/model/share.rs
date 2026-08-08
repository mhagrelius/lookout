//! Shared folders: `SYNO.Core.Share`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::as_bool;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Share {
    pub name: String,
    pub description: String,
    /// The volume it sits on, `/volume1`.
    pub volume_path: String,
    pub hidden: bool,
    pub is_usb: bool,
    /// Space used, in bytes.
    ///
    /// DSM reports this as a float in **mebibytes** under
    /// `share_quota_used`. That unit is not documented and is worth the note:
    /// on a DS-series the five shares sum to 9,994 GiB against a volume
    /// reporting 10,001 GiB used, which is only consistent with MiB. Reading
    /// it as bytes understates a 12 TB share by a factor of a million.
    pub used_bytes: u64,
    /// The quota in bytes, or `None` when unlimited — DSM writes 0 for that,
    /// and a 0-byte quota would otherwise render as a full bar.
    pub quota_bytes: Option<u64>,
}

impl Share {
    /// Fraction of the quota used, or `None` when there is no quota. A share
    /// without a quota has no meaningful bar to draw.
    pub fn used_fraction(&self) -> Option<f64> {
        let quota = self.quota_bytes?;
        if quota == 0 {
            return None;
        }
        Some((self.used_bytes as f64 / quota as f64).min(1.0))
    }

    pub fn list_from_json(data: &Value) -> Vec<Share> {
        data.get("shares")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(share_from).collect())
            .unwrap_or_default()
    }
}

const MIB: f64 = 1024.0 * 1024.0;

fn mib_to_bytes(v: Option<&Value>) -> u64 {
    v.and_then(Value::as_f64)
        .filter(|f| f.is_finite() && *f >= 0.0)
        .map(|mib| (mib * MIB) as u64)
        .unwrap_or(0)
}

fn share_from(v: &Value) -> Share {
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Share {
        name: text("name"),
        description: text("desc"),
        volume_path: text("vol_path"),
        hidden: v.get("hidden").and_then(as_bool).unwrap_or(false),
        is_usb: v.get("is_usb_share").and_then(as_bool).unwrap_or(false),
        used_bytes: mib_to_bytes(v.get("share_quota_used")),
        // `quota_value` is also MiB, and 0 means unlimited rather than none.
        quota_bytes: v
            .get("quota_value")
            .and_then(Value::as_f64)
            .filter(|q| *q > 0.0)
            .map(|q| (q * MIB) as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({"shares": [
            {"name": "docker", "desc": "", "vol_path": "/volume1", "hidden": false,
             "is_usb_share": false, "quota_value": 0, "share_quota_used": 322.18359375},
            {"name": "Games", "desc": "", "vol_path": "/volume1", "hidden": false,
             "is_usb_share": false, "quota_value": 0, "share_quota_used": 12418721.0},
            {"name": "Capped", "desc": "", "vol_path": "/volume1",
             "quota_value": 1024.0, "share_quota_used": 512.0}
        ]})
    }

    #[test]
    fn share_sizes_are_mebibytes_and_convert_to_bytes() {
        // The check that established this: 12,418,721 MiB is 11.8 TiB, which
        // is the size of that share. Read as bytes it would be 12 MB.
        let s = Share::list_from_json(&sample());
        assert_eq!(s[1].used_bytes, (12_418_721.0 * MIB) as u64);
        assert!(s[1].used_bytes > 13_000_000_000_000);
    }

    #[test]
    fn a_fractional_size_survives_the_conversion() {
        let s = Share::list_from_json(&sample());
        assert_eq!(s[0].used_bytes, (322.18359375 * MIB) as u64);
    }

    #[test]
    fn a_quota_of_zero_means_unlimited_not_full() {
        // Treating 0 as a real quota renders every unquotaed share as a
        // completely full bar, which is alarming and wrong.
        let s = Share::list_from_json(&sample());
        assert_eq!(s[0].quota_bytes, None);
        assert_eq!(s[0].used_fraction(), None);
    }

    #[test]
    fn a_real_quota_gives_a_fraction() {
        let s = Share::list_from_json(&sample());
        assert_eq!(s[2].used_fraction(), Some(0.5));
    }

    #[test]
    fn usage_past_the_quota_is_capped_at_full() {
        let s = Share::list_from_json(&json!({"shares": [
            {"name": "over", "quota_value": 100.0, "share_quota_used": 500.0}
        ]}));
        assert_eq!(s[0].used_fraction(), Some(1.0));
    }

    #[test]
    fn a_negative_or_absent_size_reads_as_zero_rather_than_wrapping() {
        // `as u64` on a negative float saturates to 0 in Rust, but relying on
        // that silently is how a -1 sentinel becomes a huge number elsewhere.
        let s = Share::list_from_json(&json!({"shares": [
            {"name": "odd", "share_quota_used": -1.0},
            {"name": "blank"}
        ]}));
        assert_eq!(s[0].used_bytes, 0);
        assert_eq!(s[1].used_bytes, 0);
    }

    #[test]
    fn an_empty_reply_is_an_empty_list() {
        assert!(Share::list_from_json(&json!({})).is_empty());
    }
}
