//! Schedule parsing logic for the scheduler.
//!
//! Jobs can be scheduled via fixed intervals (`every: 30s`) or cron expressions
//! (`0 9 * * MON-FRI`). Domain types for jobs themselves live in `sober-core`;
//! this module provides only the schedule parsing that the scheduler needs.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::error::SchedulerError;

/// How a job is scheduled to run.
#[derive(Debug, Clone)]
pub enum JobSchedule {
    /// Run at a fixed interval (e.g., every 30 seconds).
    Interval(Duration),
    /// Run according to a cron expression.
    Cron(Box<cron::Schedule>),
}

impl JobSchedule {
    /// Parse a schedule string.
    ///
    /// Interval format: `"every: <duration>"` where duration is like `30s`, `5m`, `1h`.
    /// Cron format: a standard cron expression (5 or 7 fields).
    pub fn parse(s: &str) -> Result<Self, SchedulerError> {
        let trimmed = s.trim();
        if let Some(interval_str) = trimmed.strip_prefix("every:") {
            let dur = parse_duration(interval_str.trim())?;
            Ok(JobSchedule::Interval(dur))
        } else {
            let schedule = cron::Schedule::from_str(trimmed)
                .map_err(|e| SchedulerError::InvalidCron(format!("{trimmed}: {e}")))?;
            Ok(JobSchedule::Cron(Box::new(schedule)))
        }
    }

    /// Calculate the next run time from `now`.
    pub fn next_run_after(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            JobSchedule::Interval(dur) => {
                let dur = chrono::Duration::from_std(*dur).ok()?;
                Some(now + dur)
            }
            JobSchedule::Cron(schedule) => schedule.after(&now).next(),
        }
    }

    /// Serialize back to the string format stored in the database.
    pub fn to_schedule_string(&self) -> String {
        match self {
            JobSchedule::Interval(dur) => {
                let secs = dur.as_secs();
                if secs % 3600 == 0 {
                    format!("every: {}h", secs / 3600)
                } else if secs % 60 == 0 {
                    format!("every: {}m", secs / 60)
                } else {
                    format!("every: {secs}s")
                }
            }
            JobSchedule::Cron(schedule) => schedule.to_string(),
        }
    }
}

impl fmt::Display for JobSchedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_schedule_string())
    }
}

/// Parse a duration string like `30s`, `5m`, `1h`, `2d`.
fn parse_duration(s: &str) -> Result<Duration, SchedulerError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(SchedulerError::InvalidInterval("empty duration".into()));
    }

    let (num_str, suffix) = s.split_at(s.len() - 1);
    let num: u64 = num_str
        .parse()
        .map_err(|_| SchedulerError::InvalidInterval(format!("invalid number in '{s}'")))?;

    let secs = match suffix {
        "s" => num,
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        _ => {
            return Err(SchedulerError::InvalidInterval(format!(
                "unknown suffix '{suffix}' in '{s}', expected s/m/h/d"
            )));
        }
    };

    Ok(Duration::from_secs(secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_seconds() {
        let schedule = JobSchedule::parse("every: 30s").unwrap();
        match &schedule {
            JobSchedule::Interval(d) => assert_eq!(d.as_secs(), 30),
            _ => panic!("expected Interval"),
        }
    }

    #[test]
    fn parse_interval_minutes() {
        let schedule = JobSchedule::parse("every: 5m").unwrap();
        match &schedule {
            JobSchedule::Interval(d) => assert_eq!(d.as_secs(), 300),
            _ => panic!("expected Interval"),
        }
    }

    #[test]
    fn parse_interval_hours() {
        let schedule = JobSchedule::parse("every: 1h").unwrap();
        match &schedule {
            JobSchedule::Interval(d) => assert_eq!(d.as_secs(), 3600),
            _ => panic!("expected Interval"),
        }
    }

    #[test]
    fn parse_cron_expression() {
        let schedule = JobSchedule::parse("0 0 9 * * MON-FRI *").unwrap();
        assert!(matches!(schedule, JobSchedule::Cron(_)));
    }

    #[test]
    fn invalid_cron_rejected() {
        assert!(JobSchedule::parse("not a cron").is_err());
    }

    #[test]
    fn invalid_interval_rejected() {
        assert!(JobSchedule::parse("every: abc").is_err());
        assert!(JobSchedule::parse("every: ").is_err());
    }

    #[test]
    fn interval_next_run() {
        let schedule = JobSchedule::parse("every: 60s").unwrap();
        let now = Utc::now();
        let next = schedule.next_run_after(now).unwrap();
        assert_eq!((next - now).num_seconds(), 60);
    }

    #[test]
    fn cron_next_run() {
        let schedule = JobSchedule::parse("0 0 9 * * * *").unwrap();
        let now = Utc::now();
        let next = schedule.next_run_after(now).unwrap();
        assert!(next > now);
    }

    #[test]
    fn interval_schedule_string_roundtrip() {
        let schedule = JobSchedule::parse("every: 5m").unwrap();
        assert_eq!(schedule.to_schedule_string(), "every: 5m");
    }
}
