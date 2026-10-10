//! RFC 822/850/asctime dates for cookie expiry, like Synapse's `DecodeRfcDateTime`
//! and `Rfc822DateTime` (baseunits/synapse/synautil.pas:491-498, 711-790).

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

fn is_zone(token: &str) -> bool {
    token.len() == 5 && (token.starts_with('+') || token.starts_with('-'))
}

/// Parses an HTTP date into Unix seconds, accepting the RFC 1123, RFC 850, Netscape
/// cookie (`Wed, 21-Oct-15 ...`) and asctime forms. `None` when no full date is found;
/// Synapse returns 0 (1899) then, which makes a cookie expire at once.
pub(crate) fn parse(value: &str) -> Option<i64> {
    let (mut day, mut month, mut year, mut time, mut zone) = (None, None, None, None, 0i64);
    let tokens = value
        .split([' ', ',', '\t'])
        .flat_map(|t| {
            // `21-Oct-15` is a date; a leading sign is a zone offset.
            let is_zone = t.len() == 5 && (t.starts_with('+') || t.starts_with('-'));
            let parts: Vec<&str> = if is_zone {
                vec![t]
            } else {
                t.split('-').collect()
            };
            parts
        })
        .filter(|t| !t.is_empty());
    for token in tokens {
        if let Some(m) = MONTHS
            .iter()
            .position(|m| token.to_ascii_lowercase().starts_with(m))
        {
            month = Some(m as i64 + 1);
        } else if token.contains(':') {
            let mut parts = token.split(':').map(|p| p.parse::<i64>().ok());
            let (h, m) = (parts.next()??, parts.next()??);
            let s = parts.next().flatten().unwrap_or(0);
            time = Some(h * 3600 + m * 60 + s);
        } else if is_zone(token) {
            let n: i64 = token.get(1..)?.parse().ok()?;
            let offset = (n / 100) * 3600 + (n % 100) * 60;
            zone = if token.starts_with('-') {
                -offset
            } else {
                offset
            };
        } else if let Ok(n) = token.parse::<i64>() {
            if day.is_none() && token.len() <= 2 && (1..=31).contains(&n) {
                day = Some(n);
            } else {
                // Two-digit years pivot at 50 (`TwoDigitYearCenturyWindow`).
                year = Some(match token.len() {
                    1 | 2 if n < 50 => 2000 + n,
                    1 | 2 => 1900 + n,
                    _ => n,
                });
            }
        }
    }
    let days = days_from_civil(year?, month?, day?);
    Some(days * 86400 + time.unwrap_or(0) - zone)
}

/// Formats Unix seconds as `ddd, d mmm yyyy hh:nn:ss +0000`.
pub(crate) fn format(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    let month = MONTHS[(m - 1) as usize];
    let month = format!("{}{}", month[..1].to_ascii_uppercase(), &month[1..]);
    format!(
        "{}, {d} {month} {y:04} {:02}:{:02}:{:02} +0000",
        DAYS[days.rem_euclid(7) as usize],
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
