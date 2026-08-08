//! Who is connected: `SYNO.Core.CurrentConnection`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::dsm::as_bool;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub who: String,
    /// The address it came from.
    pub from: String,
    /// The service: `DSM`, `SMB`, `FTP`, …
    pub service: String,
    /// DSM's own wording of when it started. Kept verbatim for the same
    /// reason log timestamps are — it arrives with no offset.
    pub since: String,
    /// Whether DSM will let this session be ended.
    ///
    /// The app's own session reports `false`, which is the case that matters:
    /// offering an End button that logs you out of the thing you are using is
    /// a trap.
    pub can_be_ended: bool,
    pub is_current: bool,
}

impl Session {
    pub fn list_from_json(data: &Value) -> Vec<Session> {
        data.get("items")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(session_from).collect())
            .unwrap_or_default()
    }
}

fn session_from(v: &Value) -> Session {
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let is_current = v
        .get("is_current_connected")
        .and_then(as_bool)
        .unwrap_or(false);
    Session {
        who: text("who"),
        from: text("from"),
        service: {
            let descr = text("descr");
            if descr.is_empty() {
                text("type")
            } else {
                descr
            }
        },
        since: text("first_login_time"),
        // Never offer to end the session doing the asking, whatever DSM says.
        can_be_ended: v.get("can_be_kicked").and_then(as_bool).unwrap_or(false) && !is_current,
        is_current,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({"items": [
            {"who": "admin", "from": "100.101.102.103", "descr": "DSM",
             "first_login_time": "2026/08/04 09:58:03",
             "can_be_kicked": true, "is_current_connected": true},
            {"who": "someone", "from": "192.0.2.9", "descr": "SMB",
             "first_login_time": "2026/08/04 08:00:00",
             "can_be_kicked": true, "is_current_connected": false}
        ], "total": 2})
    }

    #[test]
    fn sessions_read_end_to_end() {
        let ss = Session::list_from_json(&sample());
        assert_eq!(ss.len(), 2);
        assert_eq!(ss[1].who, "someone");
        assert_eq!(ss[1].service, "SMB");
        assert_eq!(ss[1].from, "192.0.2.9");
    }

    #[test]
    fn the_session_doing_the_asking_cannot_be_ended_even_though_dsm_allows_it() {
        // DSM says can_be_kicked: true for our own connection. Offering the
        // button would let someone log themselves out mid-poll.
        let ss = Session::list_from_json(&sample());
        assert!(ss[0].is_current);
        assert!(!ss[0].can_be_ended, "our own session must not be endable");
        assert!(ss[1].can_be_ended);
    }

    #[test]
    fn a_session_dsm_refuses_to_end_stays_that_way() {
        let ss = Session::list_from_json(&json!({"items": [
            {"who": "system", "can_be_kicked": false, "is_current_connected": false}
        ]}));
        assert!(!ss[0].can_be_ended);
    }

    #[test]
    fn the_service_falls_back_to_the_type_when_there_is_no_description() {
        let ss = Session::list_from_json(&json!({"items": [{"who": "x", "type": "FTP"}]}));
        assert_eq!(ss[0].service, "FTP");
    }

    #[test]
    fn an_empty_reply_is_an_empty_list() {
        assert!(Session::list_from_json(&json!({})).is_empty());
    }
}
