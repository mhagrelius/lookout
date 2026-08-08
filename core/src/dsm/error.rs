//! What can go wrong talking to a DiskStation, as values.
//!
//! DSM does not use HTTP status codes to report failure. Every call that
//! reaches the CGI answers `200 OK` with `{"success": false, "error": {...}}`
//! in the body, so the transport's job is to turn that back into a `Result`
//! at the one place it crosses into this crate. Above that seam nothing
//! inspects a status code and nothing catches anything.

use std::fmt;

/// Why a call did not produce an answer.
#[derive(Debug)]
pub enum Error {
    /// The request never completed: DNS, connection refused, TLS rejected,
    /// timeout. The string is the underlying client's description, which is
    /// the only thing that is going to help someone diagnose it.
    Transport(String),
    /// The body was not the JSON envelope every DSM endpoint answers with.
    /// A reverse proxy in front of DSM serving its own error page is the
    /// usual cause, and it is worth distinguishing from a DSM refusal.
    Malformed(String),
    /// DSM answered, and said no.
    Dsm(DsmError),
}

impl Error {
    /// Whether re-running the same call unchanged could plausibly succeed.
    ///
    /// The poller backs off on these rather than giving up on the host; on
    /// anything else it stops and shows the reason, because retrying a
    /// permission failure five seconds later just produces it again.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Transport(_) => true,
            Error::Malformed(_) => false,
            Error::Dsm(e) => e.is_transient(),
        }
    }

    /// Whether the session is gone and a fresh login is the remedy.
    pub fn needs_login(&self) -> bool {
        matches!(
            self,
            Error::Dsm(DsmError {
                code: 105 | 106 | 107 | 119,
                ..
            })
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(m) => write!(f, "could not reach the DiskStation: {m}"),
            Error::Malformed(m) => write!(f, "unexpected reply from the DiskStation: {m}"),
            Error::Dsm(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// A refusal from DSM, with the numeric code it came with.
///
/// The code is kept rather than collapsed into an enum because the set is
/// open: each API adds its own codes above 400 and they are not documented
/// anywhere complete. Callers that care match on the number; everything else
/// shows [`DsmError::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsmError {
    pub code: i64,
    /// The API that produced it, so a compound request can say which of its
    /// parts failed rather than reporting one anonymous number.
    pub api: String,
    /// Which parameter DSM objected to, when it says. Error 120 carries
    /// `{"name": "id", "reason": "required"}` and that is far more use than
    /// "invalid parameter".
    pub parameter: Option<String>,
}

impl DsmError {
    pub fn new(code: i64, api: impl Into<String>) -> Self {
        DsmError {
            code,
            api: api.into(),
            parameter: None,
        }
    }

    /// Whether retrying unchanged could work.
    ///
    /// 117 and 118 are DSM's own "the system is busy" and are the whole
    /// reason this distinction exists — a DiskStation waking its disks
    /// answers them for a few seconds and then works fine.
    pub fn is_transient(&self) -> bool {
        matches!(self.code, 117 | 118)
    }

    /// Whether this means the account needs a second factor to get in.
    ///
    /// Login returns 403 before an OTP has been supplied and 404 when the one
    /// supplied was wrong, which the connection dialog needs to tell apart:
    /// one asks for a code, the other says the code was rejected.
    pub fn needs_otp(&self) -> bool {
        matches!(self.code, 403 | 406)
    }

    /// A sentence to show a person.
    ///
    /// Only the codes worth explaining are spelled out. The rest render as
    /// the bare number, which is honest — inventing prose for an
    /// undocumented code would be worse than showing what DSM said.
    pub fn message(&self) -> String {
        let known = match self.code {
            100 => "Unknown error.",
            101 => "The request was missing an API, method or version.",
            102 => "This DiskStation does not have that API.",
            103 => "That API does not support this method.",
            104 => "That API version does not support this call.",
            105 => "This account does not have permission for that.",
            106 => "The session timed out.",
            107 => "The session was ended by a login from somewhere else.",
            114 => "The request was missing a required parameter.",
            117 | 118 => "The DiskStation is busy.",
            119 => "The session is no longer valid.",
            120 => return self.invalid_parameter_message(),
            400 => "Wrong account name or password.",
            401 => "That account is disabled.",
            402 => "Permission denied.",
            403 | 406 => "This account needs a two-factor code.",
            404 => "That two-factor code was not accepted.",
            407 => "This address is blocked by the DiskStation.",
            408..=410 => "The account password has expired and must be changed in DSM.",
            _ => return format!("{} refused the request (error {}).", self.api, self.code),
        };
        known.to_string()
    }

    fn invalid_parameter_message(&self) -> String {
        match &self.parameter {
            Some(p) => format!("{} rejected the parameter `{}`.", self.api, p),
            None => format!("{} rejected one of the parameters.", self.api),
        }
    }
}

impl fmt::Display for DsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for DsmError {}

/// The result of anything that talks to a DiskStation.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_codes_ask_for_a_fresh_login() {
        for code in [105, 106, 107, 119] {
            let e = Error::Dsm(DsmError::new(code, "SYNO.Core.System"));
            assert!(e.needs_login(), "code {code} should send us back to login");
        }
    }

    #[test]
    fn a_missing_method_is_not_a_session_problem() {
        let e = Error::Dsm(DsmError::new(103, "SYNO.Core.System"));
        assert!(!e.needs_login());
        assert!(!e.is_transient());
    }

    #[test]
    fn a_busy_diskstation_is_worth_retrying_but_a_refusal_is_not() {
        assert!(DsmError::new(117, "x").is_transient());
        assert!(DsmError::new(118, "x").is_transient());
        assert!(!DsmError::new(402, "x").is_transient());
    }

    #[test]
    fn unreachable_hosts_are_transient_and_malformed_replies_are_not() {
        assert!(Error::Transport("connection refused".into()).is_transient());
        assert!(!Error::Malformed("expected object".into()).is_transient());
    }

    #[test]
    fn only_the_pre_code_states_ask_for_an_otp() {
        assert!(DsmError::new(403, "SYNO.API.Auth").needs_otp());
        assert!(DsmError::new(406, "SYNO.API.Auth").needs_otp());
        // 404 means a code was given and rejected, which is a different
        // sentence to the dialog: ask again, do not ask for the first time.
        assert!(!DsmError::new(404, "SYNO.API.Auth").needs_otp());
    }

    #[test]
    fn an_invalid_parameter_names_the_parameter_when_dsm_named_it() {
        let mut e = DsmError::new(120, "SYNO.Foto.BackgroundTask.Info");
        e.parameter = Some("id".into());
        assert!(e.message().contains("`id`"));
    }

    #[test]
    fn an_undocumented_code_reports_the_number_rather_than_inventing_prose() {
        let m = DsmError::new(4711, "SYNO.Docker.Project").message();
        assert!(m.contains("4711"));
        assert!(m.contains("SYNO.Docker.Project"));
    }
}
