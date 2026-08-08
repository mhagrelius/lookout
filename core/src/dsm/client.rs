//! The one thing in this crate that opens a socket.
//!
//! Everything above it works in terms of [`Result`] and typed records; the
//! conversion from "a TLS connection did something" to "this is the answer or
//! this is why there isn't one" happens here and nowhere else.

use std::time::Duration;

use serde_json::Value;

use super::envelope::{self, Call};
use super::error::{Error, Result};

/// Where a DiskStation is and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub address: String,
    pub port: u16,
    pub https: bool,
    /// Whether to insist on a certificate that chains to a real root.
    ///
    /// Off by default would be wrong, but so would refusing to offer it: a
    /// DiskStation out of the box serves a self-signed certificate, and the
    /// alternative for most people is not "get a real certificate", it is
    /// "use this over plain HTTP instead". A reachable-over-Tailscale
    /// `ts.net` name gets a real Let's Encrypt certificate and should keep
    /// this on.
    pub verify_tls: bool,
}

impl Host {
    /// The conventional DSM endpoint: HTTPS on 5001.
    pub fn new(address: impl Into<String>) -> Self {
        Host {
            address: address.into(),
            port: 5001,
            https: true,
            verify_tls: true,
        }
    }

    /// The base URL, with no trailing slash.
    pub fn base_url(&self) -> String {
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://{}:{}", self.address, self.port)
    }

    /// Where every API call goes on DSM 7.
    pub fn entry_url(&self) -> String {
        format!("{}/webapi/entry.cgi", self.base_url())
    }
}

/// Proof that we are logged in.
///
/// `device_id` is the thing worth persisting: with it, a later login skips
/// the two-factor prompt entirely, so the code is typed once at setup rather
/// than every time the app starts. The `sid` belongs in the platform secret
/// store and never in settings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Session {
    pub sid: String,
    /// DSM's CSRF token. Present when the login asked for one, and required
    /// on every subsequent call once the DiskStation has CSRF protection on.
    pub synotoken: Option<String>,
    /// Returned only by a login that passed `enable_device_token=yes`.
    pub device_id: Option<String>,
}

/// What to log in with.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub account: String,
    pub password: String,
    /// A code from the authenticator, when DSM has asked for one.
    pub otp_code: Option<String>,
    /// A device token from a previous login, which replaces the code.
    pub device_id: Option<String>,
    /// How this machine names itself when asking for a device token.
    pub device_name: String,
}

impl Credentials {
    pub fn new(account: impl Into<String>, password: impl Into<String>) -> Self {
        Credentials {
            account: account.into(),
            password: password.into(),
            otp_code: None,
            device_id: None,
            device_name: "lookout".into(),
        }
    }
}

/// A connection to one DiskStation.
pub struct Client {
    agent: ureq::Agent,
    host: Host,
    session: Option<Session>,
}

impl Client {
    /// Build a client. Opens nothing until something is called.
    pub fn new(host: Host) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .disable_verification(!host.verify_tls)
            .build();
        let config = ureq::Agent::config_builder()
            // A DiskStation with sleeping disks can take a few seconds to
            // answer the first call; anything past ten is a dead host as far
            // as a five-second poll is concerned.
            .timeout_global(Some(Duration::from_secs(10)))
            .tls_config(tls)
            .build();

        Client {
            agent: config.new_agent(),
            host,
            session: None,
        }
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Reuse a session obtained earlier, skipping the login round trip.
    pub fn restore(&mut self, session: Session) {
        self.session = Some(session);
    }

    /// Log in.
    ///
    /// Uses `SYNO.API.Auth` version 7, which is what DSM 7 speaks; versions
    /// below 6 have no device-token support and DSM 7.2 onwards answers 103
    /// to some of them anyway.
    pub fn login(&mut self, creds: &Credentials) -> Result<Session> {
        let mut call = Call::new("SYNO.API.Auth", 7, "login")
            .param("account", &creds.account)
            .param("passwd", &creds.password)
            .param("format", "sid")
            .param("enable_syno_token", "yes");

        // Ask for a device token whenever a code is being supplied, so this
        // is the last time a code is needed on this machine.
        match (&creds.otp_code, &creds.device_id) {
            (Some(code), _) => {
                call = call
                    .param("otp_code", code)
                    .param("enable_device_token", "yes")
                    .param("device_name", &creds.device_name);
            }
            (None, Some(did)) => {
                call = call
                    .param("device_id", did)
                    .param("device_name", &creds.device_name);
            }
            (None, None) => {}
        }

        let data = self.send(&call)?;

        let sid = data
            .get("sid")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Malformed("login succeeded but returned no sid".into()))?
            .to_owned();

        let session = Session {
            sid,
            synotoken: data
                .get("synotoken")
                .and_then(Value::as_str)
                .map(str::to_owned),
            // DSM has called this `did` and `device_id` in different
            // versions; take whichever is there, and keep the one we were
            // given if this login used it and got neither back.
            device_id: data
                .get("device_id")
                .or_else(|| data.get("did"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| creds.device_id.clone()),
        };

        self.session = Some(session.clone());
        Ok(session)
    }

    /// End the session. Best effort: a failure here is not worth reporting,
    /// because the session is being discarded either way.
    pub fn logout(&mut self) {
        if self.session.is_some() {
            let _ = self.send(&Call::new("SYNO.API.Auth", 7, "logout"));
            self.session = None;
        }
    }

    /// Make one call.
    pub fn call(&self, api: &str, version: u32, method: &str) -> Result<Value> {
        self.send(&Call::new(api, version, method))
    }

    /// Make one call with parameters.
    pub fn call_with(&self, call: &Call) -> Result<Value> {
        self.send(call)
    }

    /// Make several calls in one round trip.
    ///
    /// Each element of the answer is that call's own result, so one failing
    /// endpoint costs only its own card. Falls back to nothing clever if the
    /// DiskStation lacks `SYNO.Entry.Request` — the caller checks for it via
    /// [`super::capabilities`] first.
    pub fn compound(&self, calls: &[Call]) -> Result<Vec<Result<Value>>> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }

        let compound: Vec<Value> = calls
            .iter()
            .map(|c| {
                let mut obj = serde_json::Map::new();
                obj.insert("api".into(), Value::String(c.api.clone()));
                obj.insert("method".into(), Value::String(c.method.clone()));
                obj.insert("version".into(), Value::Number(c.version.into()));
                for (k, v) in &c.params {
                    // A parameter inside a compound call is a JSON value, not
                    // the string form the flat CGI takes. Anything that does
                    // not parse is sent as a string, which is what it was.
                    let parsed =
                        serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.clone()));
                    obj.insert(k.clone(), parsed);
                }
                Value::Object(obj)
            })
            .collect();

        let request = Call::new("SYNO.Entry.Request", 1, "request")
            .param("stop_when_error", "false")
            .param("mode", "\"parallel\"")
            .param("compound", Value::Array(compound).to_string());

        let data = self.send(&request)?;
        envelope::split_compound(calls, data)
    }

    /// Post one call and parse the reply.
    fn send(&self, call: &Call) -> Result<Value> {
        let mut form: Vec<(String, String)> = vec![
            ("api".into(), call.api.clone()),
            ("version".into(), call.version.to_string()),
            ("method".into(), call.method.clone()),
        ];
        form.extend(call.params.iter().cloned());

        // The session travels in the body rather than a cookie jar: it is one
        // value, it has to be attached to every call anyway, and a cookie
        // store would be a second place for it to live.
        if let Some(session) = &self.session {
            form.push(("_sid".into(), session.sid.clone()));
        }

        let mut request = self.agent.post(self.host.entry_url());
        if let Some(token) = self.session.as_ref().and_then(|s| s.synotoken.as_deref()) {
            request = request.header("X-SYNO-TOKEN", token);
        }

        let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

        let body = request
            .send_form(pairs)
            .map_err(|e| Error::Transport(e.to_string()))?
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Transport(e.to_string()))?;

        envelope::parse(&call.api, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_host_is_dsm_over_https() {
        let h = Host::new("nas.local");
        assert_eq!(h.base_url(), "https://nas.local:5001");
        assert_eq!(h.entry_url(), "https://nas.local:5001/webapi/entry.cgi");
        assert!(h.verify_tls);
    }

    #[test]
    fn plain_http_is_expressible_for_a_diskstation_on_a_trusted_network() {
        let h = Host {
            address: "10.0.0.4".into(),
            port: 5000,
            https: false,
            verify_tls: true,
        };
        assert_eq!(h.base_url(), "http://10.0.0.4:5000");
    }

    #[test]
    fn a_client_opens_nothing_until_it_is_asked_to() {
        // Constructing against an address that cannot resolve must not fail
        // or block: the preferences dialog builds one to test with.
        let c = Client::new(Host::new("no-such-host.invalid"));
        assert!(c.session().is_none());
    }

    #[test]
    fn a_restored_session_is_used_without_logging_in_again() {
        let mut c = Client::new(Host::new("nas.local"));
        c.restore(Session {
            sid: "abc".into(),
            synotoken: Some("tok".into()),
            device_id: None,
        });
        assert_eq!(c.session().expect("restored").sid, "abc");
    }
}
