//! What this particular DiskStation can do.
//!
//! `SYNO.API.Info` answers without a session and lists every API the box
//! exposes with the version range it supports. Two things depend on it: a
//! section whose namespace is absent is hidden rather than shown broken (no
//! Container Manager installed means no Containers card at all), and a call
//! is made at a version the host actually has rather than the newest the
//! documentation mentions.

use std::collections::BTreeMap;

use serde_json::Value;

use super::error::{Error, Result};

/// The version range one API supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionRange {
    pub min: u32,
    pub max: u32,
}

/// Every API a DiskStation exposes, by name.
#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    apis: BTreeMap<String, VersionRange>,
}

impl Capabilities {
    /// Parse the reply to `SYNO.API.Info`/`query`.
    pub fn parse(data: &Value) -> Result<Self> {
        let obj = data
            .as_object()
            .ok_or_else(|| Error::Malformed("SYNO.API.Info: reply was not an object".into()))?;

        let mut apis = BTreeMap::new();
        for (name, entry) in obj {
            // An entry without versions is not usable, and skipping it is
            // better than failing the whole discovery over one odd row.
            let (Some(min), Some(max)) = (
                entry.get("minVersion").and_then(Value::as_u64),
                entry.get("maxVersion").and_then(Value::as_u64),
            ) else {
                continue;
            };
            apis.insert(
                name.clone(),
                VersionRange {
                    min: min as u32,
                    max: max as u32,
                },
            );
        }

        if apis.is_empty() {
            return Err(Error::Malformed(
                "SYNO.API.Info: listed no usable APIs".into(),
            ));
        }
        Ok(Capabilities { apis })
    }

    /// Whether the host has this API at all.
    pub fn has(&self, api: &str) -> bool {
        self.apis.contains_key(api)
    }

    /// The version to call an API at: the highest the host supports, capped
    /// at the highest this client knows how to read.
    ///
    /// Returns `None` when the API is absent, or when the host's *minimum*
    /// is above what we understand — which is the case worth being careful
    /// about, since calling it anyway earns a 104 and a broken card.
    pub fn version_for(&self, api: &str, understood: u32) -> Option<u32> {
        let range = self.apis.get(api)?;
        if range.min > understood {
            return None;
        }
        Some(range.max.min(understood))
    }

    /// How many APIs were discovered. Useful for a diagnostics line, and for
    /// telling "discovery has not run" from "this host is very bare".
    pub fn len(&self) -> usize {
        self.apis.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apis.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Capabilities {
        Capabilities::parse(&json!({
            "SYNO.API.Auth":            {"minVersion": 1, "maxVersion": 7, "path": "entry.cgi"},
            "SYNO.Core.System":         {"minVersion": 1, "maxVersion": 3, "path": "entry.cgi"},
            "SYNO.Docker.Container":    {"minVersion": 1, "maxVersion": 1, "path": "entry.cgi"},
            "SYNO.Storage.CGI.Storage": {"minVersion": 1, "maxVersion": 1, "path": "entry.cgi"},
        }))
        .expect("sample should parse")
    }

    #[test]
    fn a_present_api_is_found() {
        assert!(sample().has("SYNO.Docker.Container"));
    }

    #[test]
    fn an_absent_api_is_absent_so_its_section_can_be_hidden() {
        // No Container Manager installed is the real case this exists for.
        assert!(!sample().has("SYNO.SurveillanceStation"));
        assert_eq!(sample().version_for("SYNO.SurveillanceStation", 1), None);
    }

    #[test]
    fn we_call_at_the_highest_version_we_understand_not_the_highest_offered() {
        // The host offers SYNO.Core.System up to 3; this client reads 1.
        assert_eq!(sample().version_for("SYNO.Core.System", 1), Some(1));
    }

    #[test]
    fn we_call_at_the_hosts_maximum_when_it_is_older_than_ours() {
        // We understand Auth 7; a host offering only up to 3 gets 3.
        let caps = Capabilities::parse(&json!({
            "SYNO.API.Auth": {"minVersion": 1, "maxVersion": 3}
        }))
        .expect("should parse");
        assert_eq!(caps.version_for("SYNO.API.Auth", 7), Some(3));
    }

    #[test]
    fn an_api_whose_minimum_is_beyond_us_is_refused_rather_than_called_wrongly() {
        // Calling this anyway earns error 104 and a card that fails for a
        // reason nobody can read.
        let caps = Capabilities::parse(&json!({
            "SYNO.Future.Thing": {"minVersion": 9, "maxVersion": 9}
        }))
        .expect("should parse");
        assert_eq!(caps.version_for("SYNO.Future.Thing", 2), None);
    }

    #[test]
    fn entries_without_versions_are_skipped_rather_than_failing_discovery() {
        let caps = Capabilities::parse(&json!({
            "SYNO.Good": {"minVersion": 1, "maxVersion": 2},
            "SYNO.Odd":  {"path": "entry.cgi"}
        }))
        .expect("should parse");
        assert!(caps.has("SYNO.Good"));
        assert!(!caps.has("SYNO.Odd"));
    }

    #[test]
    fn a_reply_listing_nothing_usable_is_an_error() {
        assert!(Capabilities::parse(&json!({})).is_err());
        assert!(Capabilities::parse(&json!("nope")).is_err());
    }
}
