//! Port of packages/core/src/format.ts. Times are local; weekday and month
//! names are fixed to English (chrono has no locale table without pulling in
//! ICU, unlike the TS client's `Intl` calls, which follow the system locale).

use chrono::{DateTime, Datelike, Local, NaiveTime};
use unicode_segmentation::UnicodeSegmentation;

use crate::model::Millis;

const DAY_MS: Millis = 24 * 60 * 60 * 1000;

fn to_local(ms: Millis) -> DateTime<Local> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default().with_timezone(&Local)
}

fn start_of_day(ms: Millis) -> Millis {
    let dt = to_local(ms);
    dt.with_time(NaiveTime::MIN).single().unwrap_or(dt).timestamp_millis()
}

pub fn format_time(ms: Millis) -> String {
    to_local(ms).format("%-I:%M %p").to_string()
}

fn weekday(ms: Millis) -> String {
    to_local(ms).format("%A").to_string()
}

fn short_date(ms: Millis, now: Millis) -> String {
    let same_year = to_local(ms).year() == to_local(now).year();
    let fmt = if same_year { "%b %-d" } else { "%b %-d, %Y" };
    to_local(ms).format(fmt).to_string()
}

/// Sidebar timestamp: time today, "Yesterday", a weekday inside the week, else a date.
pub fn format_list_date(ms: Millis, now: Millis) -> String {
    let today = start_of_day(now);
    let day = start_of_day(ms);
    if day == today {
        format_time(ms)
    } else if day == today - DAY_MS {
        "Yesterday".to_owned()
    } else if day > today - 6 * DAY_MS {
        weekday(ms)
    } else {
        short_date(ms, now)
    }
}

/// "Today 9:41 AM", "Yesterday 6:02 PM", "Monday 8:15 AM", "Sep 2, 2025 at 4:30 PM".
pub fn format_separator(ms: Millis, now: Millis) -> String {
    let today = start_of_day(now);
    let day = start_of_day(ms);
    let time = format_time(ms);
    if day == today {
        format!("Today {time}")
    } else if day == today - DAY_MS {
        format!("Yesterday {time}")
    } else if day > today - 6 * DAY_MS {
        format!("{} {time}", weekday(ms))
    } else {
        format!("{} at {time}", short_date(ms, now))
    }
}

/// Messages.app inserts a separator when more than an hour passed since the previous message.
pub fn needs_separator(previous: Option<Millis>, current: Millis) -> bool {
    match previous {
        None => true,
        Some(previous) => current - previous > 60 * 60 * 1000,
    }
}

pub fn hhmm(ms: Millis) -> String {
    to_local(ms).format("%H:%M").to_string()
}

/// "just now", "5 min ago", "2 h ago", then "Yesterday 18:02", a weekday, or a date.
pub fn relative_time(ms: Millis, now: Millis) -> String {
    let diff = now - ms;
    if diff < 60_000 {
        return "just now".to_owned();
    }
    if diff < 60 * 60_000 {
        return format!("{} min ago", diff / 60_000);
    }
    if diff < DAY_MS {
        return format!("{} h ago", diff / (60 * 60_000));
    }
    let today = start_of_day(now);
    let day = start_of_day(ms);
    if day == today - DAY_MS {
        format!("Yesterday {}", hhmm(ms))
    } else if day > today - 6 * DAY_MS {
        format!("{} {}", weekday(ms), hhmm(ms))
    } else {
        format!("{} at {}", short_date(ms, now), hhmm(ms))
    }
}

/// Bare time today, "Tomorrow at 9:00 AM", else a dated one.
pub fn format_scheduled_for(ms: Millis, now: Millis) -> String {
    let today = start_of_day(now);
    let day = start_of_day(ms);
    let time = format_time(ms);
    if day == today {
        time
    } else if day == today + DAY_MS {
        format!("Tomorrow at {time}")
    } else {
        format!("{} at {time}", short_date(ms, now))
    }
}

fn valid_clock(hour: u32, minute: u32) -> bool {
    hour <= 23 && minute <= 59
}

const BAD_TIME: &str = "Use a 24-hour time between 00:00 and 23:59";

/// `HH:MM` (today), `tomorrow HH:MM`, or `YYYY-MM-DD HH:MM`. Rejects anything unparsable or already past.
pub fn parse_schedule_time(input: &str, now: Millis) -> Result<Millis, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter a time".to_owned());
    }

    let now_local = to_local(now);
    let date = if let Some(caps) = regex_date_time().captures(trimmed) {
        let year: i32 = caps[1].parse().unwrap();
        let month: u32 = caps[2].parse().unwrap();
        let day: u32 = caps[3].parse().unwrap();
        let hour: u32 = caps[4].parse().unwrap();
        let minute: u32 = caps[5].parse().unwrap();
        if !valid_clock(hour, minute) {
            return Err(BAD_TIME.to_owned());
        }
        let naive_date = chrono::NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| "That date does not exist".to_owned())?;
        let naive_time = NaiveTime::from_hms_opt(hour, minute, 0).ok_or_else(|| BAD_TIME.to_owned())?;
        naive_date
            .and_time(naive_time)
            .and_local_timezone(Local)
            .single()
            .unwrap_or_else(|| naive_date.and_time(naive_time).and_utc().with_timezone(&Local))
    } else if let Some(caps) = regex_tomorrow().captures(trimmed) {
        let hour: u32 = caps[1].parse().unwrap();
        let minute: u32 = caps[2].parse().unwrap();
        if !valid_clock(hour, minute) {
            return Err(BAD_TIME.to_owned());
        }
        let naive_date = (now_local.date_naive() + chrono::Duration::days(1)).and_hms_opt(hour, minute, 0).unwrap();
        naive_date.and_local_timezone(Local).single().unwrap_or_else(|| naive_date.and_utc().with_timezone(&Local))
    } else if let Some(caps) = regex_time_only().captures(trimmed) {
        let hour: u32 = caps[1].parse().unwrap();
        let minute: u32 = caps[2].parse().unwrap();
        if !valid_clock(hour, minute) {
            return Err(BAD_TIME.to_owned());
        }
        let naive_date = now_local.date_naive().and_hms_opt(hour, minute, 0).unwrap();
        naive_date.and_local_timezone(Local).single().unwrap_or_else(|| naive_date.and_utc().with_timezone(&Local))
    } else {
        return Err("Use HH:MM, \"tomorrow HH:MM\" or YYYY-MM-DD HH:MM".to_owned());
    };

    if date.timestamp_millis() <= now {
        return Err("That time has already passed".to_owned());
    }
    Ok(date.timestamp_millis())
}

fn regex_time_only() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^(\d{1,2}):(\d{2})$").unwrap())
}

fn regex_tomorrow() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?i)^tomorrow\s+(\d{1,2}):(\d{2})$").unwrap())
}

fn regex_date_time() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^(\d{4})-(\d{2})-(\d{2})\s+(\d{1,2}):(\d{2})$").unwrap())
}

pub fn format_address(address: &str) -> String {
    if address.contains('@') {
        return address.to_owned();
    }
    let digits: String = address.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    if let Some(caps) = regex_us_phone().captures(&digits) {
        return format!("+1 ({}) {}-{}", &caps[1], &caps[2], &caps[3]);
    }
    if let Some(body) = digits.strip_prefix('+') {
        let country_len = body.len().saturating_sub(9);
        let country = &body[..country_len];
        let rest = &body[country_len..];
        let groups: Vec<String> = rest.chars().collect::<Vec<_>>().chunks(3).map(|chunk| chunk.iter().collect()).collect();
        let groups = if groups.is_empty() { vec![rest.to_owned()] } else { groups };
        return format!("+{country} {}", groups.join(" ")).trim().to_owned();
    }
    address.to_owned()
}

fn regex_us_phone() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^\+1(\d{3})(\d{3})(\d{4})$").unwrap())
}

/// First user-perceived character. Indexing a string would split a surrogate pair, which the native JSON parser rejects.
pub fn first_grapheme(value: &str) -> &str {
    value.graphemes(true).next().unwrap_or("")
}

/// "#" for an empty name or one starting with a digit, "+" or "(".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.trim().split_whitespace().filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return "#".to_owned();
    }
    if name.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '(') {
        return "#".to_owned();
    }
    let first = first_grapheme(words[0]);
    let last = if words.len() > 1 { first_grapheme(words[words.len() - 1]) } else { "" };
    format!("{first}{last}").to_uppercase()
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.0} KB", bytes as f64 / 1024.0);
    }
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

pub fn pluralize(count: usize, singular: &str, plural: Option<&str>) -> String {
    let owned;
    let word = if count == 1 {
        singular
    } else {
        match plural {
            Some(p) => p,
            None => {
                owned = format!("{singular}s");
                &owned
            }
        }
    };
    format!("{count} {word}")
}

fn is_extended_pictographic(c: char) -> bool {
    matches!(c,
        '\u{00A9}' | '\u{00AE}' |
        '\u{203C}' | '\u{2049}' |
        '\u{2122}' | '\u{2139}' |
        '\u{2194}'..='\u{21AA}' |
        '\u{231A}'..='\u{231B}' |
        '\u{2328}' | '\u{23CF}' |
        '\u{23E9}'..='\u{23FA}' |
        '\u{24C2}' |
        '\u{25AA}'..='\u{25FE}' |
        '\u{2600}'..='\u{27BF}' |
        '\u{2934}'..='\u{2935}' |
        '\u{2B00}'..='\u{2BFF}' |
        '\u{3030}' | '\u{303D}' |
        '\u{3297}' | '\u{3299}' |
        '\u{1F000}'..='\u{1FFFF}'
    )
}

fn is_emoji_modifier(c: char) -> bool {
    matches!(c, '\u{1F3FB}'..='\u{1F3FF}')
}

fn is_regional_indicator(c: char) -> bool {
    matches!(c, '\u{1F1E6}'..='\u{1F1FF}')
}

fn is_tag_char(c: char) -> bool {
    matches!(c, '\u{E0020}'..='\u{E007F}')
}

/// One grapheme cluster that is a single emoji: a country flag (two regional
/// indicators), a keycap, or a pictograph with its modifiers, variation
/// selectors, ZWJ parts and tag characters.
fn is_emoji_cluster(segment: &str) -> bool {
    let chars: Vec<char> = segment.chars().collect();
    if chars.len() == 2 && is_regional_indicator(chars[0]) && is_regional_indicator(chars[1]) {
        return true;
    }
    if chars.is_empty() {
        return false;
    }
    let mut i;
    if is_extended_pictographic(chars[0]) {
        i = 1;
    } else if matches!(chars[0], '0'..='9' | '#' | '*') {
        i = 1;
        if chars.get(i) == Some(&'\u{FE0F}') {
            i += 1;
        }
        if chars.get(i) == Some(&'\u{20E3}') {
            i += 1;
        } else {
            return false;
        }
    } else {
        return false;
    }
    while i < chars.len() {
        let c = chars[i];
        if is_extended_pictographic(c) || is_emoji_modifier(c) || c == '\u{200D}' || c == '\u{FE0F}' || c == '\u{20E3}' || is_tag_char(c) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

/// At most `max` emoji graphemes and nothing else. A flag counts as one.
pub fn is_emoji_only(text: &str, max: usize) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut count = 0;
    for segment in trimmed.graphemes(true) {
        if segment.trim().is_empty() {
            continue;
        }
        if !is_emoji_cluster(segment) {
            return false;
        }
        count += 1;
        if count > max {
            return false;
        }
    }
    count > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local_ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Millis {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap().timestamp_millis()
    }

    fn now() -> Millis {
        // Thu Sep 3 2026, 14:30 local
        local_ms(2026, 9, 3, 14, 30)
    }

    #[test]
    fn parses_a_bare_time_as_today() {
        assert_eq!(parse_schedule_time("20:00", now()), Ok(local_ms(2026, 9, 3, 20, 0)));
    }

    #[test]
    fn parses_tomorrow_hh_mm() {
        assert_eq!(parse_schedule_time("tomorrow 09:00", now()), Ok(local_ms(2026, 9, 4, 9, 0)));
    }

    #[test]
    fn parses_tomorrow_case_insensitively_with_a_single_digit_hour() {
        assert_eq!(parse_schedule_time("Tomorrow 9:05", now()), Ok(local_ms(2026, 9, 4, 9, 5)));
    }

    #[test]
    fn parses_a_full_date_and_time() {
        assert_eq!(parse_schedule_time("2026-12-25 08:00", now()), Ok(local_ms(2026, 12, 25, 8, 0)));
    }

    #[test]
    fn rejects_a_bare_time_already_in_the_past_today() {
        assert_eq!(parse_schedule_time("09:00", now()), Err("That time has already passed".to_owned()));
    }

    #[test]
    fn rejects_a_full_date_and_time_already_in_the_past() {
        assert_eq!(parse_schedule_time("2026-09-03 09:00", now()), Err("That time has already passed".to_owned()));
    }

    #[test]
    fn rejects_an_unparsable_string() {
        assert!(parse_schedule_time("whenever", now()).is_err());
    }

    #[test]
    fn rejects_an_out_of_range_hour() {
        assert!(parse_schedule_time("25:00", now()).is_err());
    }

    #[test]
    fn rejects_an_out_of_range_minute() {
        assert!(parse_schedule_time("12:75", now()).is_err());
    }

    #[test]
    fn rejects_a_date_that_does_not_exist() {
        assert!(parse_schedule_time("2026-02-30 08:00", now()).is_err());
    }

    #[test]
    fn rejects_empty_input() {
        assert!(parse_schedule_time("   ", now()).is_err());
    }

    #[test]
    fn accepts_a_country_flag() {
        assert!(is_emoji_only("\u{1F1F9}\u{1F1F7}", 3));
        assert!(is_emoji_only("\u{1F1F9}\u{1F1F7} \u{1F1EA}\u{1F1F8}", 3));
    }

    #[test]
    fn accepts_zwj_sequences_skin_tones_keycaps_and_tag_flags() {
        assert!(is_emoji_only("\u{1F3F3}\u{FE0F}\u{200D}\u{1F308}", 3));
        assert!(is_emoji_only("\u{1F44B}\u{1F3FD}", 3));
        assert!(is_emoji_only("1\u{FE0F}\u{20E3}", 3));
        assert!(is_emoji_only("\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}", 3));
    }

    #[test]
    fn rejects_text_empty_strings_and_more_than_the_allowed_count() {
        assert!(!is_emoji_only("", 3));
        assert!(!is_emoji_only("ok \u{1F44D}", 3));
        assert!(!is_emoji_only("5", 3));
        assert!(!is_emoji_only("\u{1F600}\u{1F600}\u{1F600}\u{1F600}", 3));
        assert!(!is_emoji_only("\u{1F1F9}\u{1F1F7}\u{1F1EA}\u{1F1F8}\u{1F1EB}\u{1F1F7}\u{1F1F3}\u{1F1F1}", 3));
    }
}
