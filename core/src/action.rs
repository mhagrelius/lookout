//! The short, fixed list of things this app will change.
//!
//! Everything else is read-only. These live in core rather than in the
//! widgets for the same reason the poll plan does — a second frontend needs
//! the same verbs — and because the quoting rule below is a DSM fact, not a
//! drawing decision.
//!
//! **String parameters must be JSON-quoted.** `name=brain-server` earns "not
//! a json value" from the CGI, and a project id like `8ec29f37…` sent unquoted
//! is parsed as a number in scientific notation. [`quoted`] is not optional
//! politeness; it is the difference between working and not.

use crate::dsm::{quoted, Call, Capabilities, Client, Error, Result};

/// What can be done to a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

impl ContainerAction {
    /// The DSM method name.
    pub fn method(self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
        }
    }

    /// Whether to confirm before doing it.
    ///
    /// Starting something is recoverable by stopping it. Stopping a container
    /// interrupts whatever it was serving, which is not.
    pub fn needs_confirmation(self) -> bool {
        matches!(self, ContainerAction::Stop | ContainerAction::Restart)
    }

    /// The sentence a toast shows afterwards.
    pub fn past_tense(self) -> &'static str {
        match self {
            ContainerAction::Start => "started",
            ContainerAction::Stop => "stopped",
            ContainerAction::Restart => "restarted",
        }
    }

    /// The call it becomes.
    pub fn call(self, version: u32, container: &str) -> Call {
        Call::new("SYNO.Docker.Container", version, self.method()).param("name", quoted(container))
    }
}

/// Run a container action.
pub fn container(
    client: &Client,
    caps: &Capabilities,
    action: ContainerAction,
    name: &str,
) -> Result<()> {
    let Some(version) = caps.version_for("SYNO.Docker.Container", 1) else {
        return Err(Error::Malformed(
            "this DiskStation has no Container Manager".into(),
        ));
    };
    client.call_with(&action.call(version, name))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_action_becomes_the_call_dsm_expects() {
        let call = ContainerAction::Stop.call(1, "brain-server");
        assert_eq!(call.api, "SYNO.Docker.Container");
        assert_eq!(call.method, "stop");
        assert_eq!(call.version, 1);
        assert_eq!(
            call.params,
            vec![("name".to_string(), "\"brain-server\"".to_string())]
        );
    }

    #[test]
    fn the_container_name_is_json_quoted_because_dsm_refuses_it_otherwise() {
        // The single most repeated mistake against this API.
        let call = ContainerAction::Start.call(1, "web");
        assert_eq!(call.params[0].1, "\"web\"");
        assert!(call.params[0].1.starts_with('"'));
    }

    #[test]
    fn a_name_with_a_quote_in_it_is_escaped_rather_than_breaking_the_json() {
        let call = ContainerAction::Start.call(1, r#"od"d"#);
        assert_eq!(call.params[0].1, r#""od\"d""#);
    }

    #[test]
    fn stopping_and_restarting_confirm_but_starting_does_not() {
        // Starting is undone by stopping; stopping interrupts whatever the
        // container was serving.
        assert!(ContainerAction::Stop.needs_confirmation());
        assert!(ContainerAction::Restart.needs_confirmation());
        assert!(!ContainerAction::Start.needs_confirmation());
    }

    #[test]
    fn every_action_has_a_method_and_a_past_tense_for_the_toast() {
        for action in [
            ContainerAction::Start,
            ContainerAction::Stop,
            ContainerAction::Restart,
        ] {
            assert!(!action.method().is_empty());
            assert!(!action.past_tense().is_empty());
        }
    }
}
