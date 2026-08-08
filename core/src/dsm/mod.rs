//! Talking to a DiskStation.
//!
//! Everything here is DSM's vocabulary — sessions, `entry.cgi`, error codes,
//! compound requests. The typed records that the rest of the app works in
//! terms of live in [`crate::model`], which reads the JSON this produces.

pub mod capabilities;
pub mod client;
pub mod envelope;
pub mod error;

pub use capabilities::Capabilities;
pub use client::{Client, Credentials, Host, Session};
pub use envelope::{quoted, Call};
pub use error::{DsmError, Error, Result};

use serde_json::Value;

/// Fetch the API catalogue.
///
/// Needs no session, which is what makes it usable as a reachability check
/// before anyone has typed a password: if this answers, the address and port
/// are right and the thing on the end is a DiskStation.
pub fn discover(client: &Client) -> Result<Capabilities> {
    let call = Call::new("SYNO.API.Info", 1, "query").param("query", "all");
    let data = client.call_with(&call)?;
    Capabilities::parse(&data)
}

/// Read a field that DSM sends as a number in some versions and a numeric
/// string in others.
///
/// Both happen, in the same reply: `SYNO.Storage.CGI.Storage` sends byte
/// counts as strings because they run past 2^53, while `temp` on the same
/// disk is a number. A helper is cheaper than being wrong once.
pub fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// The same, for the byte counts that only ever arrive as strings.
pub fn as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Read a field DSM sends as a bool in some versions and `"yes"`/`"no"` or
/// `0`/`1` in others.
pub fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        Value::String(s) => match s.as_str() {
            "yes" | "true" | "1" => Some(true),
            "no" | "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Seconds since the Unix epoch, from an RFC 3339 timestamp.
///
/// Hand-rolled rather than adding a date crate for one field: the only
/// timestamps DSM sends in this form are Docker's, always UTC or with a
/// numeric offset, and the whole job is a civil-date-to-days conversion.
///
/// `0001-01-01T00:00:00Z` — Docker's "never" — is before the epoch and so
/// reads as `None`, which is what the callers want anyway.
pub fn as_unix_seconds(value: &Value) -> Option<u64> {
    let text = value.as_str()?;
    let (date, rest) = text.split_once('T')?;

    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;

    // Split the zone off before touching the clock: the fractional seconds
    // run right up against it (`32.78343004Z`, `13.53714687-04:00`).
    let (clock, offset_seconds) = split_zone(rest)?;

    let mut clock_parts = clock.split(':');
    let hour: i64 = clock_parts.next()?.parse().ok()?;
    let minute: i64 = clock_parts.next()?.parse().ok()?;
    let second: i64 = clock_parts
        .next()?
        .split_once('.')
        .map_or(clock, |(whole, _)| whole)
        .rsplit(':')
        .next()?
        .parse()
        .ok()?;

    let days = days_from_civil(year, month, day)?;
    let total = days * 86_400 + hour * 3600 + minute * 60 + second - offset_seconds;
    u64::try_from(total).ok()
}

/// Split `12:34:56.789Z` or `12:34:56-04:00` into the clock and the offset in
/// seconds east of UTC.
fn split_zone(rest: &str) -> Option<(&str, i64)> {
    if let Some(clock) = rest.strip_suffix('Z') {
        return Some((clock, 0));
    }
    // Look for the sign after the clock, not inside it — an offset is always
    // the last `+`/`-` in the string.
    let sign_at = rest.rfind(['+', '-'])?;
    let (clock, zone) = rest.split_at(sign_at);
    let sign = if zone.starts_with('-') { -1 } else { 1 };
    let (h, m) = zone[1..].split_once(':')?;
    let seconds: i64 = h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60;
    Some((clock, sign * seconds))
}

/// Days from 1970-01-01 to a civil date, by Howard Hinnant's algorithm.
///
/// Correct for every proleptic Gregorian date, which matters because Docker's
/// "never" sentinel is year 1.
fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_docker_start_time_reads_as_a_unix_timestamp() {
        // Verbatim from SYNO.Docker.Container/list on the NAS.
        assert_eq!(
            as_unix_seconds(&json!("2026-08-05T13:26:32.78343004Z")),
            Some(1_785_936_392)
        );
        // The epoch itself, as the fixed point everything else is measured
        // against.
        assert_eq!(as_unix_seconds(&json!("1970-01-01T00:00:00Z")), Some(0));
    }

    #[test]
    fn a_numeric_offset_is_applied_rather_than_ignored() {
        // Docker's health-check log entries carry the box's local offset.
        // Treating -04:00 as UTC would put the reading four hours early.
        let utc = as_unix_seconds(&json!("2026-08-08T12:57:13Z")).expect("utc");
        let east = as_unix_seconds(&json!("2026-08-08T08:57:13-04:00")).expect("offset");
        assert_eq!(utc, east);
    }

    #[test]
    fn dockers_never_sentinel_is_absent_rather_than_a_huge_uptime() {
        // `FinishedAt` on a container that has never stopped is year 1.
        // Read as a u64 it would underflow into a container up for millennia.
        assert_eq!(as_unix_seconds(&json!("0001-01-01T00:00:00Z")), None);
        assert_eq!(as_unix_seconds(&json!(null)), None);
        assert_eq!(as_unix_seconds(&json!("not a timestamp")), None);
    }

    #[test]
    fn leap_years_and_century_rules_land_on_the_right_day() {
        // 2000 is a leap year, 1900 is not; an algorithm that gets the
        // century rule wrong is off by a day for decades either side.
        let feb29 = as_unix_seconds(&json!("2000-02-29T00:00:00Z")).expect("2000-02-29");
        let mar01 = as_unix_seconds(&json!("2000-03-01T00:00:00Z")).expect("2000-03-01");
        assert_eq!(mar01 - feb29, 86_400);
    }

    #[test]
    fn byte_counts_read_the_same_whether_they_arrive_as_strings_or_numbers() {
        // This is not hypothetical: volume sizes exceed 2^53 and arrive
        // quoted, while disk temperatures on the same record do not.
        assert_eq!(as_u64(&json!("28770439729152")), Some(28_770_439_729_152));
        assert_eq!(as_u64(&json!(31)), Some(31));
        assert_eq!(as_u64(&json!(null)), None);
        assert_eq!(as_u64(&json!("not a number")), None);
    }

    #[test]
    fn signed_reads_cope_with_both_spellings_too() {
        assert_eq!(as_i64(&json!("-5")), Some(-5));
        assert_eq!(as_i64(&json!(42)), Some(42));
    }

    #[test]
    fn dsm_spells_true_several_ways_and_all_of_them_read_as_true() {
        for v in [json!(true), json!("yes"), json!("true"), json!(1)] {
            assert_eq!(as_bool(&v), Some(true), "{v} should be true");
        }
        for v in [json!(false), json!("no"), json!("false"), json!(0)] {
            assert_eq!(as_bool(&v), Some(false), "{v} should be false");
        }
        assert_eq!(as_bool(&json!("maybe")), None);
    }
}
