//! Timestamp formatting for API payloads.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Unix milliseconds as an RFC 3339 UTC timestamp; the epoch when out of range.
pub(crate) fn rfc3339_from_unix_ms(ms: i64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_default()
}

/// The current time as an RFC 3339 UTC timestamp.
pub(crate) fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
