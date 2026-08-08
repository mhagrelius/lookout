//! Turning DSM's reply envelope into a `Result`.
//!
//! Every endpoint on a DiskStation answers with the same two shapes:
//!
//! ```json
//! {"success": true,  "data": { ... }}
//! {"success": false, "error": {"code": 119}}
//! ```
//!
//! This module is the only place that knows that, and it is pure — it takes a
//! string and returns a value or an error, touches no socket, and is
//! therefore where the tests for this behaviour live.

use serde_json::Value;

use super::error::{DsmError, Error, Result};

/// Parse one reply body.
///
/// `api` is carried in only so a failure can say which call produced it; the
/// envelope itself does not name the API.
pub fn parse(api: &str, body: &str) -> Result<Value> {
    let root: Value =
        serde_json::from_str(body).map_err(|e| Error::Malformed(format!("{api}: {e}")))?;
    from_value(api, root)
}

/// Parse one already-deserialised envelope.
///
/// Split out from [`parse`] because a compound reply arrives as a list of
/// these nested inside an outer envelope that has already been parsed once.
pub fn from_value(api: &str, root: Value) -> Result<Value> {
    let obj = root
        .as_object()
        .ok_or_else(|| Error::Malformed(format!("{api}: reply was not a JSON object")))?;

    match obj.get("success").and_then(Value::as_bool) {
        Some(true) => {
            // A call with nothing to report omits `data` entirely rather than
            // sending an empty object — `logout` and most of the action verbs
            // do. That is a success, not a malformed reply.
            Ok(obj.get("data").cloned().unwrap_or(Value::Null))
        }
        Some(false) => Err(Error::Dsm(error_from(api, obj.get("error")))),
        None => Err(Error::Malformed(format!(
            "{api}: reply had no `success` field"
        ))),
    }
}

/// Read the `error` object, which is missing more often than it should be.
fn error_from(api: &str, error: Option<&Value>) -> DsmError {
    let Some(error) = error.and_then(Value::as_object) else {
        // `success: false` with no error object at all. DSM does this
        // occasionally and there is nothing to report but the fact of it.
        return DsmError::new(-1, api);
    };

    let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
    let mut out = DsmError::new(code, api);

    // Error 120 nests the offending parameter, which is the single most
    // useful thing DSM ever puts in an error body.
    out.parameter = error
        .get("errors")
        .and_then(|e| e.get("name"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    out
}

/// One call inside a compound request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub api: String,
    pub version: u32,
    pub method: String,
    /// Extra parameters, already stringified the way DSM wants them.
    pub params: Vec<(String, String)>,
}

impl Call {
    pub fn new(api: impl Into<String>, version: u32, method: impl Into<String>) -> Self {
        Call {
            api: api.into(),
            version,
            method: method.into(),
            params: Vec::new(),
        }
    }

    /// Add a parameter.
    ///
    /// Note that DSM wants JSON values here, not bare words: a string
    /// parameter has to arrive quoted (`id="abc"`) or the CGI reports "not a
    /// json value", and an unquoted id that looks like a number in scientific
    /// notation is parsed as one. [`quoted`] exists for that.
    pub fn param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push((key.into(), value.into()));
        self
    }
}

/// Wrap a string parameter in the quotes DSM's parameter parser requires.
pub fn quoted(value: &str) -> String {
    // serde_json does the escaping, which matters for share names and
    // container names with quotes or backslashes in them.
    Value::String(value.to_owned()).to_string()
}

/// Split a compound reply into one result per call.
///
/// The outer envelope succeeds even when individual calls inside it failed —
/// `has_fail` says whether any did — so each element is turned into its own
/// `Result`. That is what lets one dead endpoint grey out one card instead of
/// blanking the page.
pub fn split_compound(calls: &[Call], data: Value) -> Result<Vec<Result<Value>>> {
    let results = data
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Malformed("compound reply had no `result` array".into()))?;

    if results.len() != calls.len() {
        return Err(Error::Malformed(format!(
            "compound reply had {} results for {} calls",
            results.len(),
            calls.len()
        )));
    }

    Ok(calls
        .iter()
        .zip(results)
        .map(|(call, result)| from_value(&call.api, result.clone()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_successful_reply_yields_its_data() {
        let v = parse(
            "SYNO.Core.System",
            r#"{"success":true,"data":{"model":"DS-series"}}"#,
        )
        .expect("should parse");
        assert_eq!(v["model"], "DS-series");
    }

    #[test]
    fn a_success_with_no_data_is_still_a_success() {
        let v = parse("SYNO.API.Auth", r#"{"success":true}"#).expect("should parse");
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn a_failure_carries_its_code_and_the_api_that_produced_it() {
        let e = parse(
            "SYNO.Core.System",
            r#"{"success":false,"error":{"code":119}}"#,
        )
        .expect_err("should fail");
        match e {
            Error::Dsm(d) => {
                assert_eq!(d.code, 119);
                assert_eq!(d.api, "SYNO.Core.System");
            }
            other => panic!("expected a DSM error, got {other:?}"),
        }
    }

    #[test]
    fn error_120_keeps_the_parameter_dsm_objected_to() {
        let body =
            r#"{"success":false,"error":{"code":120,"errors":{"name":"id","reason":"required"}}}"#;
        let e = parse("SYNO.Foto.BackgroundTask.Info", body).expect_err("should fail");
        match e {
            Error::Dsm(d) => assert_eq!(d.parameter.as_deref(), Some("id")),
            other => panic!("expected a DSM error, got {other:?}"),
        }
    }

    #[test]
    fn a_failure_with_no_error_object_still_fails() {
        let e = parse("SYNO.Core.System", r#"{"success":false}"#).expect_err("should fail");
        assert!(matches!(e, Error::Dsm(_)));
    }

    #[test]
    fn a_proxy_error_page_is_malformed_rather_than_a_dsm_refusal() {
        // The distinction matters: this means "something is in front of your
        // DiskStation", not "your DiskStation said no".
        let e = parse("SYNO.Core.System", "<html>502 Bad Gateway</html>")
            .expect_err("html is not an envelope");
        assert!(matches!(e, Error::Malformed(_)));
    }

    #[test]
    fn a_reply_without_success_is_malformed() {
        let e = parse("SYNO.Core.System", r#"{"data":{}}"#).expect_err("should fail");
        assert!(matches!(e, Error::Malformed(_)));
    }

    #[test]
    fn string_parameters_are_quoted_and_escaped() {
        assert_eq!(quoted("brain-server"), "\"brain-server\"");
        assert_eq!(quoted(r#"od"d"#), r#""od\"d""#);
    }

    #[test]
    fn a_compound_reply_is_split_into_one_result_per_call() {
        let calls = vec![
            Call::new("SYNO.Core.System", 1, "info"),
            Call::new("SYNO.Docker.Container", 1, "list"),
        ];
        let data = json!({
            "has_fail": true,
            "result": [
                {"api": "SYNO.Core.System", "success": true, "data": {"model": "DS-series"}},
                {"api": "SYNO.Docker.Container", "success": false, "error": {"code": 102}},
            ]
        });

        let out = split_compound(&calls, data).expect("should split");
        assert_eq!(
            out[0].as_ref().expect("first call succeeded")["model"],
            "DS-series"
        );
        // The second failing does not cost us the first, which is the entire
        // reason the Overview batches this way.
        let err = out[1].as_ref().expect_err("second call failed");
        assert!(matches!(err, Error::Dsm(d) if d.code == 102));
    }

    #[test]
    fn a_compound_reply_of_the_wrong_length_is_refused_rather_than_misaligned() {
        // Silently zipping a short list would attribute one API's data to
        // another API's name, which is worse than failing.
        let calls = vec![
            Call::new("SYNO.Core.System", 1, "info"),
            Call::new("SYNO.Docker.Container", 1, "list"),
        ];
        let data = json!({"result": [{"success": true, "data": {}}]});
        assert!(matches!(
            split_compound(&calls, data),
            Err(Error::Malformed(_))
        ));
    }
}
