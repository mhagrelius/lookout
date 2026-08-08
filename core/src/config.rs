//! What the app remembers between runs.
//!
//! Deliberately holds **no secret**. The password is typed at connect time and
//! the session id lives only in memory; what is persisted is the address, the
//! account name, and the two-factor device token — which is not a credential
//! on its own, since it only skips the OTP step for someone who already knows
//! the password.
//!
//! A plain JSON file rather than GSettings, because GSettings needs a compiled
//! schema installed on the machine and this crate has to work unchanged on
//! Windows and macOS.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dsm::Host;
use crate::trend::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub address: String,
    pub port: u16,
    pub https: bool,
    pub verify_tls: bool,
    pub account: String,
    /// From a previous two-factor login. Lets a later login skip the code.
    pub device_id: Option<String>,
    /// Seconds between polls, 1–60.
    pub poll_interval: u64,
    pub range: Range,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            address: String::new(),
            port: 5001,
            https: true,
            verify_tls: true,
            account: String::new(),
            device_id: None,
            poll_interval: 5,
            range: Range::Hour,
        }
    }
}

impl Config {
    /// Whether there is enough here to try connecting without asking.
    pub fn is_configured(&self) -> bool {
        !self.address.is_empty() && !self.account.is_empty()
    }

    pub fn host(&self) -> Host {
        Host {
            address: self.address.clone(),
            port: self.port,
            https: self.https,
            verify_tls: self.verify_tls,
        }
    }

    /// Clamp anything a hand-edited file could get wrong.
    pub fn sanitised(mut self) -> Self {
        self.poll_interval = self.poll_interval.clamp(1, 60);
        if self.port == 0 {
            self.port = if self.https { 5001 } else { 5000 };
        }
        self
    }

    /// Where the config lives, honouring `XDG_CONFIG_HOME` so tests can
    /// redirect it.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("lookout").join("config.json")
    }

    /// A missing or unreadable file is the default config — a first run, not
    /// a failure.
    pub fn load(path: &Path) -> Config {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<Config>(&t).ok())
            .unwrap_or_default()
            .sanitised()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_config_is_not_configured_and_does_not_connect_to_anything() {
        assert!(!Config::default().is_configured());
    }

    #[test]
    fn an_address_and_account_are_enough_to_try() {
        let c = Config {
            address: "nas.ts.net".into(),
            account: "monitor".into(),
            ..Config::default()
        };
        assert!(c.is_configured());
        assert_eq!(c.host().base_url(), "https://nas.ts.net:5001");
    }

    #[test]
    fn a_hand_edited_interval_is_clamped_into_range() {
        // The file is editable by design; a 0 there would spin the poll loop.
        let c = Config {
            poll_interval: 0,
            ..Config::default()
        }
        .sanitised();
        assert_eq!(c.poll_interval, 1);
        let c = Config {
            poll_interval: 9999,
            ..Config::default()
        }
        .sanitised();
        assert_eq!(c.poll_interval, 60);
    }

    #[test]
    fn a_zero_port_falls_back_to_the_conventional_one_for_the_scheme() {
        let https = Config {
            port: 0,
            https: true,
            ..Config::default()
        }
        .sanitised();
        assert_eq!(https.port, 5001);
        let http = Config {
            port: 0,
            https: false,
            ..Config::default()
        }
        .sanitised();
        assert_eq!(http.port, 5000);
    }

    #[test]
    fn no_password_field_exists_to_be_written_to_disk() {
        // Guards the property rather than the implementation: if someone adds
        // one, the serialised form changes and this fails.
        let json = serde_json::to_string(&Config::default()).expect("serialises");
        assert!(
            !json.contains("passw"),
            "a password reached the config file: {json}"
        );
        assert!(!json.contains("sid"));
    }

    #[test]
    fn a_config_round_trips_through_a_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");

        let c = Config {
            address: "nas.ts.net".into(),
            port: 5002,
            account: "monitor".into(),
            device_id: Some("token".into()),
            ..Config::default()
        };
        c.save(&path).expect("saves");
        assert_eq!(Config::load(&path), c);
    }

    #[test]
    fn a_missing_config_is_the_default_rather_than_an_error() {
        assert_eq!(
            Config::load(Path::new("/nonexistent/config.json")),
            Config::default()
        );
    }

    #[test]
    fn a_corrupt_config_does_not_stop_the_app_starting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json").expect("write");
        assert_eq!(Config::load(&path), Config::default());
    }
}
