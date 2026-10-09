//! The zone FMD2's zone-less timestamps are read in.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use thiserror::Error;

/// The time zone FMD2 ran in. FMD2 stores `Now`, the local wall-clock time, without a zone
/// (`PrepSQLValue`, baseunits/SQLiteData.pas:159-167); cookie expiry dates are local too
/// (`DecodeRfcDateTime` converts to local time, baseunits/synapse/synautil.pas:798-805).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeZone(jiff::tz::TimeZone);

#[derive(Debug, Error)]
#[error("unknown time zone {name:?}: {reason}")]
pub struct UnknownTimeZone {
    name: String,
    reason: String,
}

impl TimeZone {
    pub const UTC: Self = Self(jiff::tz::TimeZone::UTC);

    /// This machine's zone (`TZ`, else the system's), or UTC when it cannot be found.
    pub fn system() -> Self {
        Self(jiff::tz::TimeZone::system())
    }

    /// An IANA zone, e.g. `Europe/Amsterdam`.
    pub fn named(name: &str) -> Result<Self, UnknownTimeZone> {
        jiff::tz::TimeZone::get(name)
            .map(Self)
            .map_err(|e| UnknownTimeZone {
                name: name.to_string(),
                reason: e.to_string(),
            })
    }

    /// Unix milliseconds of the wall-clock time `local` (milliseconds since 1970-01-01 00:00 on
    /// the local clock). A time skipped by a DST change is read with the offset before the change,
    /// a repeated one as its first occurrence.
    pub(crate) fn to_utc(&self, local: i64) -> Option<i64> {
        let civil = Timestamp::from_millisecond(local)
            .ok()?
            .to_zoned(jiff::tz::TimeZone::UTC)
            .datetime();
        let utc = self.0.to_ambiguous_timestamp(civil).compatible().ok()?;
        Some(utc.as_millisecond())
    }
}

impl Default for TimeZone {
    fn default() -> Self {
        Self::system()
    }
}

impl FromStr for TimeZone {
    type Err = UnknownTimeZone;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::named(s)
    }
}

impl fmt::Display for TimeZone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.iana_name().unwrap_or("local"))
    }
}
