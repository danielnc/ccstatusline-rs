//! Time-zone conversion for the reset-timestamp formatter, standing in for
//! `Intl.DateTimeFormat('en-US', { timeZone, timeZoneName: 'short', … })`.
//!
//! Offsets come from the system tz database (TZif files under
//! /usr/share/zoneinfo, with the POSIX footer rule past the last transition).
//! Zone names follow ICU's en-US short names: `UTC` for the UTC aliases, the
//! US/Canada/Caribbean metazone abbreviations (EST, CDT, AKST, HST, ADT, …),
//! `GMT` for the zones on the GMT metazone at offset zero, and the localized
//! GMT format (`GMT+2`, `GMT+5:30`, `GMT-3`) for everything else. Locales other
//! than en-US are formatted as en-US.

use std::path::PathBuf;

pub struct Parts {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 = Sunday
    pub weekday: u32,
    pub zone_name: String,
}

const UTC_ALIASES: [&str; 16] = [
    "utc",
    "etc/utc",
    "etc/uct",
    "uct",
    "etc/universal",
    "universal",
    "zulu",
    "etc/zulu",
    "gmt",
    "etc/gmt",
    "etc/gmt0",
    "gmt0",
    "etc/gmt+0",
    "etc/gmt-0",
    "greenwich",
    "etc/greenwich",
];

fn zoneinfo_dir() -> PathBuf {
    PathBuf::from(std::env::var("TZDIR").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "/usr/share/zoneinfo".into()))
}

fn valid_zone_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
}

/// The zone `Intl` resolves when no `timeZone` is given: `TZ`, else the
/// system zone.
pub fn default_zone_name() -> Option<String> {
    if let Ok(tz) = std::env::var("TZ") {
        let tz = tz.strip_prefix(':').unwrap_or(&tz).to_string();
        if !tz.is_empty() {
            return Some(tz);
        }
    }
    let target = std::fs::read_link("/etc/localtime").ok()?;
    let s = target.to_string_lossy();
    let i = s.find("zoneinfo/")? + "zoneinfo/".len();
    Some(s[i..].to_string())
}

#[derive(Clone)]
struct TType {
    offset: i64,
    is_dst: bool,
    abbr: String,
}

struct TzData {
    transitions: Vec<i64>,
    idx: Vec<u8>,
    types: Vec<TType>,
    footer: Option<String>,
}

fn be32(b: &[u8]) -> i64 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64
}

fn be64(b: &[u8]) -> i64 {
    i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn parse_tzif(data: &[u8]) -> Option<TzData> {
    fn header(d: &[u8]) -> Option<(u8, [usize; 6])> {
        if d.len() < 44 || &d[..4] != b"TZif" {
            return None;
        }
        let mut c = [0usize; 6];
        for (i, v) in c.iter_mut().enumerate() {
            *v = u32::from_be_bytes([d[20 + i * 4], d[21 + i * 4], d[22 + i * 4], d[23 + i * 4]]) as usize;
        }
        Some((d[4], c))
    }
    let (version, c) = header(data)?;
    let v1_len = c[3] * 4 + c[3] + c[4] * 6 + c[5] + c[2] * 8 + c[1] + c[0];
    let (body, time_size, counts) = if version >= b'2' {
        let second = &data.get(44 + v1_len..)?;
        let (_, c2) = header(second)?;
        (&second[44..], 8usize, c2)
    } else {
        (&data[44..], 4usize, c)
    };
    let [isut, isstd, leap, timecnt, typecnt, charcnt] = counts;
    let mut p = 0;
    let take = |p: &mut usize, n: usize| -> Option<&[u8]> {
        let s = body.get(*p..*p + n)?;
        *p += n;
        Some(s)
    };
    let times = take(&mut p, timecnt * time_size)?;
    let transitions: Vec<i64> =
        times.chunks_exact(time_size).map(|t| if time_size == 8 { be64(t) } else { be32(t) }).collect();
    let idx = take(&mut p, timecnt)?.to_vec();
    let tt = take(&mut p, typecnt * 6)?;
    let chars = take(&mut p, charcnt)?;
    take(&mut p, leap * (time_size + 4) + isstd + isut)?;
    let types = tt
        .as_chunks::<6>()
        .0
        .iter()
        .map(|t| {
            let ai = t[5] as usize;
            let abbr = chars.get(ai..).map(|s| s.split(|b| *b == 0).next().unwrap_or(&[])).unwrap_or(&[]);
            TType { offset: be32(t), is_dst: t[4] != 0, abbr: String::from_utf8_lossy(abbr).into_owned() }
        })
        .collect::<Vec<_>>();
    if types.is_empty() || idx.iter().any(|i| *i as usize >= types.len()) {
        return None;
    }
    let footer = if version >= b'2' {
        let rest = &body[p..];
        let s = String::from_utf8_lossy(rest);
        let s = s.trim_matches('\n');
        (!s.is_empty() && !s.contains('\n')).then(|| s.to_string())
    } else {
        None
    };
    Some(TzData { transitions, idx, types, footer })
}

// ------------------------------------------------------------ POSIX TZ rules

struct Rule {
    std_abbr: String,
    std_offset: i64,
    dst: Option<(String, i64, DateRule, i64, DateRule, i64)>,
}

#[derive(Clone, Copy)]
enum DateRule {
    /// Mm.w.d
    M(u32, u32, u32),
    /// Jn: 1..=365, Feb 29 never counted
    J(u32),
    /// n: 0..=365, zero-based day of year
    N(u32),
}

fn parse_abbr(s: &str) -> Option<(String, &str)> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        return Some((rest[..end].to_string(), &rest[end + 1..]));
    }
    let n = s.bytes().take_while(|b| b.is_ascii_alphabetic()).count();
    (n >= 3).then(|| (s[..n].to_string(), &s[n..]))
}

/// `[+-]hh[:mm[:ss]]` in seconds.
fn parse_hms(s: &str) -> Option<(i64, &str)> {
    let (sign, s) = match s.as_bytes().first() {
        Some(b'+') => (1, &s[1..]),
        Some(b'-') => (-1, &s[1..]),
        _ => (1, s),
    };
    let n = s.bytes().take_while(|b| b.is_ascii_digit() || *b == b':').count();
    if n == 0 {
        return None;
    }
    let mut total = 0i64;
    for (i, part) in s[..n].split(':').enumerate() {
        let v: i64 = part.parse().ok()?;
        total += v * [3600, 60, 1].get(i)?;
    }
    Some((sign * total, &s[n..]))
}

fn parse_date_rule(s: &str) -> Option<(DateRule, i64, &str)> {
    let (rule, rest) = if let Some(m) = s.strip_prefix('M') {
        let n = m.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
        let parts: Vec<u32> = m[..n].split('.').map(|x| x.parse().ok()).collect::<Option<_>>()?;
        if parts.len() != 3 {
            return None;
        }
        (DateRule::M(parts[0], parts[1], parts[2]), &m[n..])
    } else if let Some(j) = s.strip_prefix('J') {
        let n = j.bytes().take_while(|b| b.is_ascii_digit()).count();
        (DateRule::J(j[..n].parse().ok()?), &j[n..])
    } else {
        let n = s.bytes().take_while(|b| b.is_ascii_digit()).count();
        (DateRule::N(s[..n].parse().ok()?), &s[n..])
    };
    let (time, rest) = match rest.strip_prefix('/') {
        Some(t) => parse_hms(t)?,
        None => (7200, rest),
    };
    Some((rule, time, rest))
}

fn parse_rule(s: &str) -> Option<Rule> {
    let (std_abbr, rest) = parse_abbr(s)?;
    let (off, rest) = parse_hms(rest)?;
    let std_offset = -off;
    if rest.is_empty() {
        return Some(Rule { std_abbr, std_offset, dst: None });
    }
    let (dst_abbr, rest) = parse_abbr(rest)?;
    let (dst_offset, rest) = match rest.as_bytes().first() {
        Some(b',') | None => (std_offset + 3600, rest),
        _ => {
            let (o, r) = parse_hms(rest)?;
            (-o, r)
        }
    };
    let rest = rest.strip_prefix(',')?;
    let (r1, t1, rest) = parse_date_rule(rest)?;
    let rest = rest.strip_prefix(',')?;
    let (r2, t2, rest) = parse_date_rule(rest)?;
    if !rest.is_empty() {
        return None;
    }
    Some(Rule { std_abbr, std_offset, dst: Some((dst_abbr, dst_offset, r1, t1, r2, t2)) })
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// Day number (days since epoch) of a rule's date in `year`.
fn rule_day(r: DateRule, year: i64) -> i64 {
    let jan1 = crate::timefmt::days_from_civil(year, 1, 1);
    match r {
        DateRule::J(n) => {
            let mut d = n as i64 - 1;
            if is_leap(year) && n >= 60 {
                d += 1;
            }
            jan1 + d
        }
        DateRule::N(n) => jan1 + n as i64,
        DateRule::M(m, w, d) => {
            let first = crate::timefmt::days_from_civil(year, m, 1);
            let wd_first = (first + 4).rem_euclid(7);
            let mut day = first + (d as i64 - wd_first).rem_euclid(7) + (w as i64 - 1) * 7;
            let next_month = if m == 12 {
                crate::timefmt::days_from_civil(year + 1, 1, 1)
            } else {
                crate::timefmt::days_from_civil(year, m + 1, 1)
            };
            while day >= next_month {
                day -= 7;
            }
            day
        }
    }
}

fn rule_type(rule: &Rule, t: i64) -> TType {
    let std = TType { offset: rule.std_offset, is_dst: false, abbr: rule.std_abbr.clone() };
    let Some((dst_abbr, dst_offset, r1, t1, r2, t2)) = &rule.dst else { return std };
    let year = crate::timefmt::utc((t * 1000) as f64).year;
    let start = rule_day(*r1, year) * 86400 + t1 - rule.std_offset;
    let end = rule_day(*r2, year) * 86400 + t2 - dst_offset;
    let in_dst = if start < end { t >= start && t < end } else { t >= start || t < end };
    if in_dst {
        TType { offset: *dst_offset, is_dst: true, abbr: dst_abbr.clone() }
    } else {
        std
    }
}

impl TzData {
    fn type_at(&self, t: i64) -> TType {
        if self.transitions.is_empty() || t < self.transitions[0] {
            if self.transitions.is_empty() {
                if let Some(r) = self.footer.as_deref().and_then(parse_rule) {
                    return rule_type(&r, t);
                }
            }
            return self.types.iter().find(|x| !x.is_dst).unwrap_or(&self.types[0]).clone();
        }
        let i = match self.transitions.binary_search(&t) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        if i == self.transitions.len() - 1 {
            if let Some(r) = self.footer.as_deref().and_then(parse_rule) {
                return rule_type(&r, t);
            }
        }
        self.types[self.idx[i] as usize].clone()
    }
}

fn load_zone(name: &str) -> Option<TzData> {
    if !valid_zone_name(name) {
        return None;
    }
    let data = std::fs::read(zoneinfo_dir().join(name)).ok()?;
    parse_tzif(&data)
}

/// A zone the tz database (or the UTC aliases) knows by this name.
pub fn zone_exists(name: &str) -> bool {
    UTC_ALIASES.contains(&name.to_ascii_lowercase().as_str()) || load_zone(name).is_some()
}

fn uses_us_metazone(zone: &str) -> bool {
    let z = zone.to_ascii_lowercase();
    if z == "america/havana" || z == "cuba" {
        return false;
    }
    z.starts_with("america/")
        || z.starts_with("us/")
        || z.starts_with("canada/")
        || matches!(
            z.as_str(),
            "atlantic/bermuda"
                | "pacific/honolulu"
                | "pacific/johnston"
                | "est5edt"
                | "cst6cdt"
                | "mst7mdt"
                | "pst8pdt"
                | "est"
                | "mst"
                | "hst"
                | "jamaica"
                | "navajo"
                | "mexico/general"
        )
}

const US_ABBRS: [&str; 14] =
    ["EST", "EDT", "CST", "CDT", "MST", "MDT", "PST", "PDT", "AKST", "AKDT", "HST", "HDT", "AST", "ADT"];

fn gmt_format(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let a = offset.abs();
    let (h, m) = (a / 3600, (a % 3600) / 60);
    if m == 0 {
        format!("GMT{sign}{h}")
    } else {
        format!("GMT{sign}{h}:{m:02}")
    }
}

fn zone_display_name(zone: &str, t: &TType) -> String {
    if US_ABBRS.contains(&t.abbr.as_str()) && uses_us_metazone(zone) {
        return t.abbr.clone();
    }
    if t.offset == 0 && t.abbr == "GMT" {
        return "GMT".into();
    }
    gmt_format(t.offset)
}

/// Civil date/time of `ms` in `zone` (None: the default zone). None when the
/// zone is unknown, which upstream sees as a RangeError.
pub fn date_parts(ms: f64, zone: Option<&str>) -> Option<Parts> {
    let local_offset = |ms| crate::timefmt::local(ms).offset_seconds;
    match zone {
        Some(z) => parts_in(ms, Some(z), false, local_offset),
        None => parts_in(ms, default_zone_name().as_deref(), true, local_offset),
    }
}

/// `name` None: a system zone without a name (`/etc/localtime` a plain file).
/// `local_offset` is the C library's UTC offset in seconds at `ms`.
fn parts_in(ms: f64, name: Option<&str>, system: bool, local_offset: fn(f64) -> i64) -> Option<Parts> {
    // unnamed or unreadable system zone: the C library's view
    let c_local = || {
        let offset = local_offset(ms);
        (offset, gmt_format(offset))
    };
    let (offset, zone_name) = match name {
        None => match c_local() {
            (0, _) => (0, "UTC".to_string()),
            local => local,
        },
        Some(name) if UTC_ALIASES.contains(&name.to_ascii_lowercase().as_str()) => (0, "UTC".to_string()),
        Some(name) => match load_zone(name) {
            Some(data) => {
                let t = data.type_at((ms / 1000.0).floor() as i64);
                (t.offset, zone_display_name(name, &t))
            }
            None if system => c_local(),
            None => return None,
        },
    };
    let c = crate::timefmt::utc(ms + (offset * 1000) as f64);
    Some(Parts {
        year: c.year,
        month: c.month,
        day: c.day,
        hour: c.hour,
        minute: c.minute,
        weekday: c.weekday,
        zone_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(iso: &str, zone: &str) -> String {
        let ms = crate::timefmt::parse_iso(iso).unwrap();
        let p = date_parts(ms, Some(zone)).unwrap();
        format!("{}-{:02}-{:02} {:02}:{:02} {}", p.year, p.month, p.day, p.hour, p.minute, p.zone_name)
    }

    #[test]
    fn zones_match_node_intl() {
        if !std::path::Path::new("/usr/share/zoneinfo/America/New_York").exists() {
            return;
        }
        assert_eq!(fmt("2026-10-21T00:05:00Z", "America/New_York"), "2026-10-20 20:05 EDT");
        assert_eq!(fmt("2026-01-15T13:45:00Z", "America/New_York"), "2026-01-15 08:45 EST");
        assert_eq!(fmt("2026-01-15T13:45:00Z", "America/Mexico_City"), "2026-01-15 07:45 CST");
        assert_eq!(fmt("2026-01-15T13:45:00Z", "America/Havana"), "2026-01-15 08:45 GMT-5");
        assert_eq!(fmt("2026-07-04T23:59:00Z", "Europe/London"), "2026-07-05 00:59 GMT+1");
        assert_eq!(fmt("2026-01-15T13:45:00Z", "Europe/London"), "2026-01-15 13:45 GMT");
        assert_eq!(fmt("2026-10-21T00:05:00Z", "Asia/Kolkata"), "2026-10-21 05:35 GMT+5:30");
        assert_eq!(fmt("2026-01-15T13:45:00Z", "Australia/Sydney"), "2026-01-16 00:45 GMT+11");
        assert_eq!(fmt("2026-01-15T13:45:00Z", "Etc/GMT"), "2026-01-15 13:45 UTC");
        assert_eq!(fmt("2040-07-04T12:00:00Z", "America/Chicago"), "2040-07-04 07:00 CDT");
        assert_eq!(fmt("2040-01-04T12:00:00Z", "Europe/Berlin"), "2040-01-04 13:00 GMT+1");
        assert!(date_parts(0.0, Some("Bogus/Zone")).is_none());
        assert!(date_parts(0.0, Some("../etc/passwd")).is_none());
    }

    #[test]
    fn unnamed_system_zone_is_the_c_library_local_zone() {
        let ms = crate::timefmt::parse_iso("2026-01-15T13:45:00Z").unwrap();
        let at = |offset: fn(f64) -> i64| {
            let p = parts_in(ms, None, true, offset).unwrap();
            format!("{}-{:02}-{:02} {:02}:{:02} {}", p.year, p.month, p.day, p.hour, p.minute, p.zone_name)
        };
        assert_eq!(at(|_| -5 * 3600), "2026-01-15 08:45 GMT-5");
        assert_eq!(at(|_| 5 * 3600 + 1800), "2026-01-15 19:15 GMT+5:30");
        assert_eq!(at(|_| 0), "2026-01-15 13:45 UTC");
    }

    #[test]
    fn posix_rules() {
        let r = parse_rule("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        let summer = crate::timefmt::parse_iso("2041-07-01T00:00:00Z").unwrap() / 1000.0;
        assert_eq!(rule_type(&r, summer as i64).offset, 7200);
        let r = parse_rule("<+0530>-5:30").unwrap();
        assert_eq!(rule_type(&r, 0).offset, 19800);
        let r = parse_rule("<-03>3").unwrap();
        assert_eq!(rule_type(&r, 0).offset, -10800);
    }
}
