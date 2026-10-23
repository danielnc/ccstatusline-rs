//! Date helpers: `Date#toISOString`, local civil time via libc (honours TZ).

pub struct Civil {
    pub year: i64,
    /// 1..=12
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
    /// 0 = Sunday
    pub weekday: u32,
    /// seconds east of UTC
    pub offset_seconds: i64,
}

fn days_to_civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn utc(ms: f64) -> Civil {
    let total_ms = ms.floor() as i64;
    let secs = total_ms.div_euclid(1000);
    let millis = total_ms.rem_euclid(1000) as u32;
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let (y, m, d) = days_to_civil(days);
    Civil {
        year: y,
        month: m,
        day: d,
        hour: (sod / 3600) as u32,
        minute: ((sod % 3600) / 60) as u32,
        second: (sod % 60) as u32,
        millis,
        weekday: (days + 4).rem_euclid(7) as u32,
        offset_seconds: 0,
    }
}

/// Local time per the process TZ (like JS `getHours()` etc.).
pub fn local(ms: f64) -> Civil {
    let total_ms = ms.floor() as i64;
    let secs = total_ms.div_euclid(1000);
    let millis = total_ms.rem_euclid(1000) as u32;
    unsafe {
        let t: libc::time_t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return utc(ms);
        }
        Civil {
            year: tm.tm_year as i64 + 1900,
            month: tm.tm_mon as u32 + 1,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            millis,
            weekday: tm.tm_wday as u32,
            offset_seconds: tm.tm_gmtoff as i64,
        }
    }
}

pub fn iso_string(ms: f64) -> String {
    let c = utc(ms);
    let year = if (0..=9999).contains(&c.year) {
        format!("{:04}", c.year)
    } else if c.year < 0 {
        format!("-{:06}", -c.year)
    } else {
        format!("+{:06}", c.year)
    };
    format!("{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", c.month, c.day, c.hour, c.minute, c.second, c.millis)
}

/// `Date.parse(s)` for the ISO-8601 forms timestamps use; None means NaN.
pub fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = s.get(r)?;
        if t.bytes().all(|c| c.is_ascii_digit()) && !t.is_empty() {
            t.parse().ok()
        } else {
            None
        }
    };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = num(0..4)?;
    let mo = num(5..7)?;
    let d = num(8..10)?;
    let (mut h, mut mi, mut sec, mut ms) = (0i64, 0i64, 0i64, 0f64);
    let mut i;
    let mut offset_min: Option<i64> = None;
    let date_only = b.len() == 10;
    if !date_only {
        if b[10] != b'T' && b[10] != b't' && b[10] != b' ' {
            return None;
        }
        h = num(11..13)?;
        if b.get(13) != Some(&b':') {
            return None;
        }
        mi = num(14..16)?;
        i = 16;
        if b.get(16) == Some(&b':') {
            sec = num(17..19)?;
            i = 19;
            if b.get(19) == Some(&b'.') {
                let start = 20;
                let mut end = start;
                while end < b.len() && b[end].is_ascii_digit() {
                    end += 1;
                }
                if end == start {
                    return None;
                }
                let frac = &s[start..end];
                let padded: String = frac.chars().chain(std::iter::repeat('0')).take(3).collect();
                ms = padded.parse::<f64>().ok()?;
                i = end;
            }
        }
        match b.get(i) {
            None => {}
            Some(b'Z') | Some(b'z') if i + 1 == b.len() => offset_min = Some(0),
            Some(&c) if (c == b'+' || c == b'-') && b.len() == i + 6 && b[i + 3] == b':' => {
                let oh = num(i + 1..i + 3)?;
                let om = num(i + 4..i + 6)?;
                let sign = if c == b'+' { 1 } else { -1 };
                offset_min = Some(sign * (oh * 60 + om));
            }
            _ => return None,
        }
    } else {
        offset_min = Some(0);
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 24 || mi > 59 || sec > 59 {
        return None;
    }
    let days = days_from_civil(y, mo as u32, d as u32);
    let local_ms = ((days * 86400 + h * 3600 + mi * 60 + sec) * 1000) as f64 + ms;
    match offset_min {
        Some(off) => Some(local_ms - (off * 60_000) as f64),
        None => {
            // no offset on a date-time form means local time
            let guess = local_ms;
            let off = local(guess).offset_seconds;
            let first = guess - (off * 1000) as f64;
            let off2 = local(first).offset_seconds;
            Some(guess - (off2 * 1000) as f64)
        }
    }
}

pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64;
    let doy = (153 * mp + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso() {
        assert_eq!(iso_string(0.0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_string(1792584000000.0), "2026-10-21T12:00:00.000Z");
        assert_eq!(parse_iso("2026-10-21T12:00:00.000Z"), Some(1792584000000.0));
        assert_eq!(parse_iso("2026-10-21T12:00:00+01:00"), Some(1792584000000.0 - 3_600_000.0));
    }
}
