//! UTC stamps: the epoch second as a civil date and back, so a version's name sorts by time
//! without a time zone database.

/// UTC time as (civil year, month, day, hour, minute, second) from Unix seconds — Hinnant's
/// algorithm, so a stamp needs no time zone database and reads the same on every platform.
pub(super) fn civil(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// [`civil`] backwards: days since the epoch of a civil UTC date.
pub(super) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Now, as (a name stamp `YYYYMMDD.HHMMSS`, Unix seconds, milliseconds).
pub(super) fn now() -> (String, i64, u32) {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
    let (y, mo, da, h, mi, sd) = civil(secs);
    (format!("{y:04}{mo:02}{da:02}.{h:02}{mi:02}{sd:02}"), secs, d.subsec_millis())
}

/// A point in time as ISO-8601 UTC with milliseconds.
pub(super) fn iso(secs: i64, ms: u32) -> String {
    let (y, mo, da, h, mi, sd) = civil(secs);
    format!("{y:04}-{mo:02}-{da:02}T{h:02}:{mi:02}:{sd:02}.{ms:03}Z")
}

/// The Unix second a name stamp (`20260819.153012`) stands for.
pub(super) fn stamp_epoch(stamp: &str) -> Option<i64> {
    let (d, t) = stamp.split_once('.')?;
    if d.len() != 8 || t.len() != 6 {
        return None;
    }
    if !d.bytes().all(|b| b.is_ascii_digit()) || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |s: &str, r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, da) = (n(d, 0..4)?, n(d, 4..6)?, n(d, 6..8)?);
    let (h, mi, sd) = (n(t, 0..2)?, n(t, 2..4)?, n(t, 4..6)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || mi > 59 || sd > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + sd)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stamp_round_trips_through_civil_time() {
        for secs in [0i64, 1_760_000_000, 1_766_000_000, 4_102_444_800] {
            let (y, mo, da, h, mi, sd) = civil(secs);
            assert_eq!(days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + sd, secs, "{secs}");
        }
        // A known moment: 2026-10-14T08:35:12Z.
        let (y, mo, da, h, mi, sd) = civil(1_791_966_912);
        assert_eq!((y, mo, da, h, mi, sd), (2026, 10, 14, 8, 35, 12));
        assert_eq!(iso(1_791_966_912, 482), "2026-10-14T08:35:12.482Z");
        assert_eq!(stamp_epoch("20261014.083512"), Some(1_791_966_912));
    }

    #[test]
    fn hostile_stamps_are_refused_not_guessed() {
        for s in [
            "",
            ".",
            "2026",
            "20261014",
            "20261014.",
            "20261014.0835",
            "x0261014.083512",
            "20261014.0835127",
            "20261314.083512",
            "20261014.253512",
            "é0261014.083512",
        ] {
            assert_eq!(stamp_epoch(s), None, "{s}");
        }
    }
}
