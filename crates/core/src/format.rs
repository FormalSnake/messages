//! Port of packages/core/src/format.ts. Times are local; the TS client follows
//! the system locale for 12 or 24 hour clocks.

use crate::model::Millis;

pub fn format_time(ms: Millis) -> String {
    let _ = ms;
    unimplemented!()
}

/// Sidebar timestamp: time today, "Yesterday", a weekday inside the week, else a date.
pub fn format_list_date(ms: Millis, now: Millis) -> String {
    let _ = (ms, now);
    unimplemented!()
}

/// "Today 9:41 AM", "Yesterday 6:02 PM", "Monday 8:15 AM", "Sep 2, 2025 at 4:30 PM".
pub fn format_separator(ms: Millis, now: Millis) -> String {
    let _ = (ms, now);
    unimplemented!()
}

/// Messages.app inserts a separator when more than an hour passed since the previous message.
pub fn needs_separator(previous: Option<Millis>, current: Millis) -> bool {
    let _ = (previous, current);
    unimplemented!()
}

pub fn hhmm(ms: Millis) -> String {
    let _ = ms;
    unimplemented!()
}

/// "just now", "5 min ago", "2 h ago", then "Yesterday 18:02", a weekday, or a date.
pub fn relative_time(ms: Millis, now: Millis) -> String {
    let _ = (ms, now);
    unimplemented!()
}

/// Bare time today, "Tomorrow at 9:00 AM", else a dated one.
pub fn format_scheduled_for(ms: Millis, now: Millis) -> String {
    let _ = (ms, now);
    unimplemented!()
}

/// `HH:MM` (today), `tomorrow HH:MM`, or `YYYY-MM-DD HH:MM`. Err carries the TS error strings verbatim.
pub fn parse_schedule_time(input: &str, now: Millis) -> Result<Millis, String> {
    let _ = (input, now);
    unimplemented!()
}

pub fn format_address(address: &str) -> String {
    let _ = address;
    unimplemented!()
}

pub fn first_grapheme(value: &str) -> &str {
    let _ = value;
    unimplemented!()
}

/// "#" for an empty name or one starting with a digit, "+" or "(".
pub fn initials(name: &str) -> String {
    let _ = name;
    unimplemented!()
}

pub fn format_bytes(bytes: u64) -> String {
    let _ = bytes;
    unimplemented!()
}

pub fn pluralize(count: usize, singular: &str, plural: Option<&str>) -> String {
    let _ = (count, singular, plural);
    unimplemented!()
}

/// At most `max` emoji graphemes and nothing else. A flag counts as one.
pub fn is_emoji_only(text: &str, max: usize) -> bool {
    let _ = (text, max);
    unimplemented!()
}
