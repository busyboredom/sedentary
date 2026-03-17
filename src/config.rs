use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 1]
pub(crate) struct Config {
    pub(crate) work_minutes: u32,
    pub(crate) break_minutes: u32,
    pub(crate) todos: Vec<TodoData>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            work_minutes: 30,
            break_minutes: 5,
            todos: Vec::new(),
        }
    }
}

/// Serializable snapshot of a to-do item (for persistence only).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TodoData {
    pub(crate) id: Uuid,
    pub(crate) title: String,
    pub(crate) notes: String,
    pub(crate) subtasks: Vec<TodoData>,
    pub(crate) complete: bool,
    #[serde(default)]
    pub(crate) deadline: Option<jiff::Timestamp>,
    #[serde(default)]
    pub(crate) recurrence: Option<RecurrenceRule>,
}

/// Needed because `jiff::civil::Weekday` does not implement `serde`.
#[derive(Debug, Clone, Hash, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum WeekdayConfig {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<jiff::civil::Weekday> for WeekdayConfig {
    fn from(w: jiff::civil::Weekday) -> Self {
        match w {
            jiff::civil::Weekday::Monday => Self::Monday,
            jiff::civil::Weekday::Tuesday => Self::Tuesday,
            jiff::civil::Weekday::Wednesday => Self::Wednesday,
            jiff::civil::Weekday::Thursday => Self::Thursday,
            jiff::civil::Weekday::Friday => Self::Friday,
            jiff::civil::Weekday::Saturday => Self::Saturday,
            jiff::civil::Weekday::Sunday => Self::Sunday,
        }
    }
}

impl From<WeekdayConfig> for jiff::civil::Weekday {
    fn from(w: WeekdayConfig) -> Self {
        match w {
            WeekdayConfig::Monday => Self::Monday,
            WeekdayConfig::Tuesday => Self::Tuesday,
            WeekdayConfig::Wednesday => Self::Wednesday,
            WeekdayConfig::Thursday => Self::Thursday,
            WeekdayConfig::Friday => Self::Friday,
            WeekdayConfig::Saturday => Self::Saturday,
            WeekdayConfig::Sunday => Self::Sunday,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum RecurrenceRule {
    Interval(jiff::Span),
    Weekly(indexmap::IndexSet<WeekdayConfig>),
    Yearly { month: u8, day: u8 },
}

impl RecurrenceRule {
    /// Calculates the next occurrence strictly after `base_time`.
    pub(crate) fn next_occurrence(&self, base_time: jiff::Timestamp) -> Option<jiff::Timestamp> {
        let zdt = base_time.to_zoned(jiff::tz::TimeZone::system());
        match self {
            Self::Interval(span) => zdt.checked_add(*span).ok().map(|z| z.timestamp()),
            Self::Weekly(days) => {
                if days.is_empty() {
                    return None;
                }
                let mut current = zdt;
                for _ in 1..=8 {
                    current = current.checked_add(jiff::Span::new().days(1)).unwrap();
                    let wk: WeekdayConfig = current.weekday().into();
                    if days.contains(&wk) {
                        return Some(current.timestamp());
                    }
                }
                None
            }
            Self::Yearly { month, day } => {
                let mut candidate_year = zdt.year();
                for _ in 0..5 {
                    if let Ok(c) = zdt
                        .clone()
                        .with()
                        .year(candidate_year)
                        .month((*month).cast_signed())
                        .day((*day).cast_signed())
                        .build()
                        && c.timestamp() > base_time
                    {
                        return Some(c.timestamp());
                    }
                    candidate_year += 1;
                }
                None
            }
        }
    }

    /// Returns true if the given timestamp is a valid occurrence of this rule.
    pub(crate) fn is_valid_occurrence(&self, timestamp: jiff::Timestamp) -> bool {
        let zdt = timestamp.to_zoned(jiff::tz::TimeZone::system());
        match self {
            Self::Interval(_) => true, // Any point is a valid start of an interval
            Self::Weekly(days) => {
                let wk: WeekdayConfig = zdt.weekday().into();
                days.contains(&wk)
            }
            Self::Yearly { month, day } => {
                zdt.month() == (*month).cast_signed() && zdt.day() == (*day).cast_signed()
            }
        }
    }

    /// Finds the first occurrence of this rule that is at or after `reference`.
    pub(crate) fn first_occurrence_at_or_after(
        &self,
        reference: jiff::Timestamp,
    ) -> Option<jiff::Timestamp> {
        let zdt = reference.to_zoned(jiff::tz::TimeZone::system());
        match self {
            Self::Interval(_) => Some(reference), // Interval starts immediately
            Self::Weekly(days) => {
                if days.is_empty() {
                    return None;
                }
                let mut current = zdt;
                for _ in 0..=7 {
                    let wk: WeekdayConfig = current.weekday().into();
                    if days.contains(&wk) {
                        return Some(current.timestamp());
                    }
                    current = current.checked_add(jiff::Span::new().days(1)).unwrap();
                }
                None
            }
            Self::Yearly { month, day } => {
                let mut candidate_year = zdt.year();
                for _ in 0..5 {
                    if let Ok(c) = zdt
                        .clone()
                        .with()
                        .year(candidate_year)
                        .month((*month).cast_signed())
                        .day((*day).cast_signed())
                        .build()
                        && c.timestamp() >= reference
                    {
                        return Some(c.timestamp());
                    }
                    candidate_year += 1;
                }
                None
            }
        }
    }
}

impl PartialEq for RecurrenceRule {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Interval(a), Self::Interval(b)) => a
                .compare(*b)
                .is_ok_and(|cmp| cmp == std::cmp::Ordering::Equal),
            (Self::Weekly(a), Self::Weekly(b)) => a == b,
            (Self::Yearly { month: m1, day: d1 }, Self::Yearly { month: m2, day: d2 }) => {
                m1 == m2 && d1 == d2
            }
            _ => false,
        }
    }
}
impl Eq for RecurrenceRule {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tododata_nested_roundtrip() {
        let child = TodoData {
            id: Uuid::new_v4(),
            title: "Child".to_string(),
            notes: String::new(),
            subtasks: Vec::new(),
            complete: false,
            deadline: None,
            recurrence: None,
        };
        let parent = TodoData {
            id: Uuid::new_v4(),
            title: "Parent".to_string(),
            notes: "Parent notes".to_string(),
            subtasks: vec![child],
            complete: false,
            deadline: None,
            recurrence: None,
        };
        let json = serde_json::to_string(&parent).unwrap();
        let restored: TodoData = serde_json::from_str(&json).unwrap();
        assert_eq!(parent, restored);
        assert_eq!(restored.subtasks.len(), 1);
        assert_eq!(restored.subtasks[0].title, "Child");
    }

    #[test]
    fn weekly_first_occurrence_includes_today() {
        let mon = WeekdayConfig::Monday;
        let rule = RecurrenceRule::Weekly(indexmap::indexset! { mon });

        // Let's pick a Monday
        let base = jiff::civil::date(2024, 1, 1)
            .at(12, 0, 0, 0)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .unwrap()
            .timestamp();

        let first = rule.first_occurrence_at_or_after(base).unwrap();
        assert_eq!(first, base); // Should be the same Monday

        // Check Tuesday
        let tue_base = base
            .to_zoned(jiff::tz::TimeZone::UTC)
            .checked_add(jiff::Span::new().days(1))
            .unwrap()
            .timestamp();
        let next_mon = rule.first_occurrence_at_or_after(tue_base).unwrap();
        assert_eq!(
            next_mon,
            base.to_zoned(jiff::tz::TimeZone::UTC)
                .checked_add(jiff::Span::new().days(7))
                .unwrap()
                .timestamp()
        );
    }
}
