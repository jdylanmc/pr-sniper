//! Repository schedules: one five-field cron per repository, evaluated in the
//! single saved time zone. The builder is a closed set of cron shapes that
//! encode to, and decode from, that cron. Anything else is Advanced (raw cron).

use chrono::TimeZone;
use serde::{Deserialize, Serialize};

/// Cron for a repository created after per-repository schedules.
pub const DEFAULT_CRON: &str = "*/30 * * * *";
/// Effective global cron of files that predate saved schedules entirely.
pub const LEGACY_DEFAULT_CRON: &str = "*/15 * * * *";
/// Builder intervals, in minutes.
pub const EVERY_MINUTES: [u32; 7] = [5, 10, 15, 30, 60, 120, 240];
pub const PREVIEW_RUNS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cadence {
    /// One of `EVERY_MINUTES`.
    Every { minutes: u32 },
    /// Every hour at this minute.
    Hourly { minute: u32 },
    /// Every listed `HH:MM`. One cron holds a minute x hour grid only.
    Daily { times: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Builder {
    pub cadence: Cadence,
    /// Weekdays, 0 = Sunday. All seven = every day.
    pub days: Vec<u32>,
}

const ALL_DAYS: [u32; 7] = [0, 1, 2, 3, 4, 5, 6];

impl Builder {
    pub fn encode(&self) -> Result<String, String> {
        let mut days = self.days.clone();
        days.sort_unstable();
        days.dedup();
        if days.is_empty() || days.iter().any(|day| *day > 6) {
            return Err("Choose at least one day.".into());
        }
        let weekdays = if days == ALL_DAYS {
            "*".to_string()
        } else {
            compress(&days)
        };
        let (minute, hour) = match &self.cadence {
            Cadence::Every { minutes } if *minutes < 60 && EVERY_MINUTES.contains(minutes) => {
                (format!("*/{minutes}"), "*".to_string())
            }
            Cadence::Every { minutes } if EVERY_MINUTES.contains(minutes) => {
                ("0".to_string(), format!("*/{}", minutes / 60))
            }
            Cadence::Every { .. } => return Err("Choose a listed interval.".into()),
            Cadence::Hourly { minute } if *minute < 60 => (minute.to_string(), "*".to_string()),
            Cadence::Hourly { .. } => return Err("Minute must be 0-59.".into()),
            Cadence::Daily { times } => {
                let (minutes, hours) = grid(times)?;
                (join(&minutes), join(&hours))
            }
        };
        Ok(format!("{minute} {hour} * * {weekdays}"))
    }

    /// The builder for a cron it can represent, else None (Advanced).
    pub fn decode(expression: &str) -> Option<Self> {
        let fields: Vec<_> = expression.split_whitespace().collect();
        let [minute, hour, "*", "*", weekdays] = fields[..] else {
            return None;
        };
        let days = weekday_set(weekdays)?;
        let cadence = if let Some(step) = minute.strip_prefix("*/") {
            let minutes = number(step, 1, 59)?;
            if hour != "*" || !EVERY_MINUTES.contains(&minutes) {
                return None;
            }
            Cadence::Every { minutes }
        } else if let (Some(0), Some(step)) = (number(minute, 0, 59), hour.strip_prefix("*/")) {
            let minutes = number(step, 1, 23)? * 60;
            if !EVERY_MINUTES.contains(&minutes) {
                return None;
            }
            Cadence::Every { minutes }
        } else if let (Some(at), "*") = (number(minute, 0, 59), hour) {
            Cadence::Hourly { minute: at }
        } else {
            let minutes = number_list(minute, 59)?;
            let hours = number_list(hour, 23)?;
            let times = hours
                .iter()
                .flat_map(|h| minutes.iter().map(move |m| format!("{h:02}:{m:02}")))
                .collect();
            Cadence::Daily { times }
        };
        Some(Self { cadence, days })
    }
}

/// `HH:MM` times as the minute and hour sets of one cron. Every combination
/// of those sets must be listed, or the cron would run at unlisted times.
fn grid(times: &[String]) -> Result<(Vec<u32>, Vec<u32>), String> {
    let mut parsed = Vec::new();
    for time in times {
        let (hour, minute) = time
            .split_once(':')
            .and_then(|(h, m)| {
                (h.len() == 2 && m.len() == 2)
                    .then(|| Some((number(h, 0, 23)?, number(m, 0, 59)?)))
                    .flatten()
            })
            .ok_or("Enter times as HH:MM.")?;
        parsed.push((hour, minute));
    }
    parsed.sort_unstable();
    parsed.dedup();
    if parsed.is_empty() {
        return Err("Add at least one time.".into());
    }
    let mut hours: Vec<_> = parsed.iter().map(|(h, _)| *h).collect();
    let mut minutes: Vec<_> = parsed.iter().map(|(_, m)| *m).collect();
    hours.dedup();
    minutes.sort_unstable();
    minutes.dedup();
    if hours.len() * minutes.len() != parsed.len() {
        return Err("Daily times must share the same minutes in every hour.".into());
    }
    Ok((minutes, hours))
}

fn join(values: &[u32]) -> String {
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Sorted unique days as cron ranges, e.g. `1-5` or `1,3,5`.
fn compress(days: &[u32]) -> String {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < days.len() {
        let start = days[index];
        let mut end = start;
        while index + 1 < days.len() && days[index + 1] == end + 1 {
            index += 1;
            end = days[index];
        }
        parts.push(if end >= start + 2 {
            format!("{start}-{end}")
        } else if end == start + 1 {
            format!("{start},{end}")
        } else {
            start.to_string()
        });
        index += 1;
    }
    parts.join(",")
}

fn number(text: &str, min: u32, max: u32) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) || text.len() > 2 {
        return None;
    }
    text.parse().ok().filter(|n| (min..=max).contains(n))
}

/// Plain comma-separated numbers (no ranges or steps), sorted and unique.
fn number_list(text: &str, max: u32) -> Option<Vec<u32>> {
    let mut values = text
        .split(',')
        .map(|part| number(part, 0, max))
        .collect::<Option<Vec<_>>>()?;
    values.sort_unstable();
    values.dedup();
    Some(values)
}

fn weekday(text: &str) -> Option<u32> {
    const NAMES: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];
    let upper = text.to_ascii_uppercase();
    NAMES
        .iter()
        .position(|name| *name == upper)
        .map(|day| day as u32)
        .or_else(|| number(text, 0, 7))
}

/// Weekday field without steps; `7` is Sunday.
fn weekday_set(text: &str) -> Option<Vec<u32>> {
    if text == "*" {
        return Some(ALL_DAYS.to_vec());
    }
    let mut days = Vec::new();
    for part in text.split(',') {
        if let Some((start, end)) = part.split_once('-') {
            let (start, end) = (weekday(start)?, weekday(end)?);
            if start > end {
                return None;
            }
            days.extend(start..=end);
        } else {
            days.push(weekday(part)?);
        }
    }
    for day in &mut days {
        *day %= 7;
    }
    days.sort_unstable();
    days.dedup();
    Some(days)
}

/// Five-field cron accepted for a repository: parses and has an occurrence.
pub fn validate_cron(expression: &str) -> Result<(), String> {
    next_runs(expression, "UTC", chrono::Utc::now().timestamp(), 1).map(|_| ())
}

/// The next `count` occurrences strictly after `now`, as Unix seconds.
pub fn next_runs(
    expression: &str,
    time_zone: &str,
    now: i64,
    count: usize,
) -> Result<Vec<i64>, String> {
    let invalid = || "Enter a valid five-field cron expression.".to_string();
    if expression.split_whitespace().count() != 5 {
        return Err(invalid());
    }
    let cron = expression.parse::<croner::Cron>().map_err(|_| invalid())?;
    let zone = time_zone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| "Choose a valid time zone.".to_string())?;
    let mut current = zone.timestamp_opt(now, 0).single().ok_or_else(invalid)?;
    let mut runs = Vec::with_capacity(count);
    for _ in 0..count {
        current = cron
            .find_next_occurrence(&current, false)
            .map_err(|_| "This cron never runs.".to_string())?;
        runs.push(current.timestamp());
    }
    Ok(runs)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewRequest {
    Builder { builder: Builder },
    Cron { cron: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preview {
    pub cron: String,
    /// None: Advanced only.
    pub builder: Option<Builder>,
    pub next_runs: Vec<i64>,
}

pub fn preview(request: PreviewRequest, time_zone: &str, now: i64) -> Result<Preview, String> {
    let cron = match request {
        PreviewRequest::Builder { builder } => builder.encode()?,
        PreviewRequest::Cron { cron } => cron.trim().to_string(),
    };
    let next_runs = next_runs(&cron, time_zone, now, PREVIEW_RUNS)?;
    Ok(Preview {
        builder: Builder::decode(&cron),
        cron,
        next_runs,
    })
}

/// `preview` in the saved time zone, the one every repository schedule uses.
pub fn preview_saved(
    store: &crate::storage::Store,
    request: PreviewRequest,
    now: i64,
) -> Result<Preview, String> {
    preview(request, &store.load_settings()?.time_zone, now)
}

/// Builders for saved crons, None where only Advanced represents one.
pub fn describe(crons: &[String]) -> Vec<Option<Builder>> {
    crons.iter().map(|cron| Builder::decode(cron)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder(cadence: Cadence, days: &[u32]) -> Builder {
        Builder {
            cadence,
            days: days.to_vec(),
        }
    }

    const WEEK: [u32; 7] = ALL_DAYS;
    const WEEKDAYS: [u32; 5] = [1, 2, 3, 4, 5];

    #[test]
    fn every_mode_round_trips_through_its_cron() {
        let daily = |times: &[&str]| Cadence::Daily {
            times: times.iter().map(|t| t.to_string()).collect(),
        };
        let cases = [
            (builder(Cadence::Every { minutes: 5 }, &WEEK), "*/5 * * * *"),
            (
                builder(Cadence::Every { minutes: 10 }, &WEEK),
                "*/10 * * * *",
            ),
            (
                builder(Cadence::Every { minutes: 15 }, &WEEK),
                "*/15 * * * *",
            ),
            (
                builder(Cadence::Every { minutes: 30 }, &WEEK),
                "*/30 * * * *",
            ),
            (
                builder(Cadence::Every { minutes: 60 }, &WEEK),
                "0 */1 * * *",
            ),
            (
                builder(Cadence::Every { minutes: 120 }, &WEEK),
                "0 */2 * * *",
            ),
            (
                builder(Cadence::Every { minutes: 240 }, &WEEK),
                "0 */4 * * *",
            ),
            (builder(Cadence::Hourly { minute: 0 }, &WEEK), "0 * * * *"),
            (builder(Cadence::Hourly { minute: 45 }, &WEEK), "45 * * * *"),
            (builder(daily(&["09:00"]), &WEEK), "0 9 * * *"),
            (
                builder(daily(&["09:00", "09:30", "17:00", "17:30"]), &WEEK),
                "0,30 9,17 * * *",
            ),
            (builder(daily(&["08:15"]), &[1, 3, 5]), "15 8 * * 1,3,5"),
            (
                builder(Cadence::Every { minutes: 15 }, &WEEKDAYS),
                "*/15 * * * 1-5",
            ),
            (
                builder(Cadence::Every { minutes: 120 }, &WEEKDAYS),
                "0 */2 * * 1-5",
            ),
            (
                builder(Cadence::Hourly { minute: 5 }, &[0, 6]),
                "5 * * * 0,6",
            ),
            (
                builder(Cadence::Every { minutes: 30 }, &[0, 1, 2, 4, 5, 6]),
                "*/30 * * * 0-2,4-6",
            ),
        ];
        for (builder, cron) in cases {
            assert_eq!(builder.encode().as_deref(), Ok(cron), "{builder:?}");
            assert_eq!(Builder::decode(cron), Some(builder), "{cron}");
            next_runs(cron, "UTC", 1_800_000_000, 3).unwrap();
        }
    }

    #[test]
    fn equivalent_crons_decode_into_the_builder() {
        let cases = [
            (
                "0 9 * * MON-FRI",
                builder(
                    Cadence::Daily {
                        times: vec!["09:00".into()],
                    },
                    &WEEKDAYS,
                ),
            ),
            (
                "*/15 * * * 0-6",
                builder(Cadence::Every { minutes: 15 }, &WEEK),
            ),
            (
                "30 13,9 * * 7",
                builder(
                    Cadence::Daily {
                        times: vec!["09:30".into(), "13:30".into()],
                    },
                    &[0],
                ),
            ),
        ];
        for (cron, expected) in cases {
            assert_eq!(Builder::decode(cron), Some(expected), "{cron}");
        }
    }

    #[test]
    fn crons_outside_the_builder_stay_advanced() {
        for cron in [
            "*/7 * * * *",
            "0 */3 * * *",
            "0 0 1 * *",
            "0 0 * 1 *",
            "* * * * *",
            "1-5 * * * *",
            "0 9 * * */2",
            "*/15 9-17/2 * * *",
            "15 */2 * * *",
            "0 9-17 * * 5-1",
            // Hour windows are not a builder concept: Advanced.
            "*/15 9-17 * * 1-5",
            "0 9-17/2 * * 1-5",
            "5 10-10 * * 0,6",
            "45 9-17 * * *",
            "0 0 * * * *",
            "nonsense",
        ] {
            assert_eq!(Builder::decode(cron), None, "{cron}");
        }
    }

    #[test]
    fn invalid_builders_are_rejected() {
        let daily = |times: &[&str]| Cadence::Daily {
            times: times.iter().map(|t| t.to_string()).collect(),
        };
        for invalid in [
            builder(Cadence::Every { minutes: 7 }, &WEEK),
            builder(Cadence::Every { minutes: 15 }, &[]),
            builder(Cadence::Every { minutes: 15 }, &[7]),
            builder(Cadence::Hourly { minute: 60 }, &WEEK),
            builder(daily(&[]), &WEEK),
            builder(daily(&["9:00"]), &WEEK),
            builder(daily(&["24:00"]), &WEEK),
            builder(daily(&["09:00", "13:30"]), &WEEK),
        ] {
            assert!(invalid.encode().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn next_runs_follow_the_time_zone() {
        // 2027-01-15 12:00:00 UTC = 07:00 in New York (EST).
        let now = 1_799_928_000;
        let utc = next_runs("0 9 * * *", "UTC", now, 3).unwrap();
        let new_york = next_runs("0 9 * * *", "America/New_York", now, 3).unwrap();
        assert_eq!(utc[0] - now, 21 * 3600);
        assert_eq!(new_york[0] - now, 2 * 3600);
        assert!(utc.windows(2).all(|pair| pair[1] - pair[0] == 86_400));
        assert!(new_york.windows(2).all(|pair| pair[1] - pair[0] == 86_400));
    }

    #[test]
    fn next_runs_skip_and_repeat_dst_hours_once() {
        // 2027-03-14 02:30 does not exist in New York; it runs at the first
        // valid time instead of being dropped.
        let before_spring = chrono_tz::America::New_York
            .with_ymd_and_hms(2027, 3, 14, 0, 0, 0)
            .unwrap()
            .timestamp();
        let spring = next_runs("30 2 * * *", "America/New_York", before_spring, 2).unwrap();
        let first = chrono_tz::America::New_York
            .timestamp_opt(spring[0], 0)
            .unwrap();
        assert_eq!(first.date_naive().to_string(), "2027-03-14");
        assert!(spring[1] - spring[0] >= 23 * 3600);
        // 2027-11-07 01:30 happens twice; it runs once that day.
        let before_fall = chrono_tz::America::New_York
            .with_ymd_and_hms(2027, 11, 7, 0, 0, 0)
            .unwrap()
            .timestamp();
        let fall = next_runs("30 1 * * *", "America/New_York", before_fall, 2).unwrap();
        assert!(fall[1] - fall[0] > 23 * 3600, "{fall:?}");
    }

    #[test]
    fn preview_encodes_builders_and_rejects_invalid_crons() {
        let preview = preview(
            PreviewRequest::Builder {
                builder: builder(Cadence::Every { minutes: 30 }, &WEEK),
            },
            "UTC",
            1_800_000_000,
        )
        .unwrap();
        assert_eq!(preview.cron, DEFAULT_CRON);
        assert_eq!(preview.next_runs.len(), PREVIEW_RUNS);
        assert!(preview.next_runs.iter().all(|run| run % 1800 == 0));
        for cron in ["", "* * * *", "61 * * * *", "0 0 30 2 *"] {
            assert!(
                super::preview(PreviewRequest::Cron { cron: cron.into() }, "UTC", 0).is_err(),
                "{cron}"
            );
        }
        assert!(super::preview(
            PreviewRequest::Cron {
                cron: DEFAULT_CRON.into()
            },
            "Mars/Olympus_Mons",
            0
        )
        .is_err());
    }
}
