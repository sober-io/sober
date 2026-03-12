//! Job types and scheduling logic.
//!
//! Jobs can be scheduled via fixed intervals (`every: 30s`) or cron expressions
//! (`0 9 * * MON-FRI`). Each job has an owner (system, user, or agent) and a
//! payload that gets passed to the agent when executed.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use uuid::Uuid;

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

/// Who owns a scheduled job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOwner {
    /// Built-in system task (memory pruning, health checks, etc.).
    System,
    /// Created by a user.
    User(Uuid),
    /// Created by the agent.
    Agent(Uuid),
}

impl JobOwner {
    /// Parse from the `owner_type` and `owner_id` pair stored in the database.
    pub fn from_parts(owner_type: &str, owner_id: Option<Uuid>) -> Result<Self, SchedulerError> {
        match owner_type {
            "system" => Ok(JobOwner::System),
            "user" => {
                let id = owner_id.ok_or_else(|| {
                    SchedulerError::Internal("user owner requires owner_id".into())
                })?;
                Ok(JobOwner::User(id))
            }
            "agent" => {
                let id = owner_id.ok_or_else(|| {
                    SchedulerError::Internal("agent owner requires owner_id".into())
                })?;
                Ok(JobOwner::Agent(id))
            }
            other => Err(SchedulerError::Internal(format!(
                "unknown owner_type: {other}"
            ))),
        }
    }

    /// The owner type string for database storage.
    pub fn owner_type(&self) -> &'static str {
        match self {
            JobOwner::System => "system",
            JobOwner::User(_) => "user",
            JobOwner::Agent(_) => "agent",
        }
    }

    /// The owner ID (None for system jobs).
    pub fn owner_id(&self) -> Option<Uuid> {
        match self {
            JobOwner::System => None,
            JobOwner::User(id) | JobOwner::Agent(id) => Some(*id),
        }
    }
}

/// Status of a scheduled job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Job is active and will run on schedule.
    Active,
    /// Job is temporarily paused.
    Paused,
    /// Job has been cancelled.
    Cancelled,
    /// Job is currently executing.
    Running,
}

impl JobStatus {
    /// Parse from the string stored in the database.
    pub fn from_str_value(s: &str) -> Result<Self, SchedulerError> {
        match s {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "cancelled" => Ok(Self::Cancelled),
            "running" => Ok(Self::Running),
            other => Err(SchedulerError::Internal(format!(
                "unknown job status: {other}"
            ))),
        }
    }

    /// String representation for database storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
            Self::Running => "running",
        }
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A scheduled job.
#[derive(Debug, Clone)]
pub struct Job {
    /// Unique job ID.
    pub id: Uuid,
    /// Human-readable job name.
    pub name: String,
    /// Who owns this job.
    pub owner: JobOwner,
    /// Schedule (interval or cron).
    pub schedule: JobSchedule,
    /// Opaque payload passed to the agent when executed.
    pub payload: Vec<u8>,
    /// Current status.
    pub status: JobStatus,
    /// When the job should next run.
    pub next_run_at: DateTime<Utc>,
    /// When the job last ran (if ever).
    pub last_run_at: Option<DateTime<Utc>>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// Whether to wake the agent when this job completes.
    pub notify_agent: bool,
}

impl Job {
    /// Calculate and update `next_run_at` based on the schedule and current time.
    pub fn advance_next_run(&mut self, now: DateTime<Utc>) {
        if let Some(next) = self.schedule.next_run_after(now) {
            self.next_run_at = next;
        }
    }
}

/// Status of an individual job run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobRunStatus {
    /// Currently running.
    Running,
    /// Completed successfully.
    Succeeded,
    /// Failed with an error.
    Failed,
}

impl JobRunStatus {
    /// Parse from string.
    pub fn from_str_value(s: &str) -> Result<Self, SchedulerError> {
        match s {
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(SchedulerError::Internal(format!(
                "unknown run status: {other}"
            ))),
        }
    }

    /// String representation for database storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
}

/// A record of a single job execution.
#[derive(Debug, Clone)]
pub struct JobRun {
    /// Unique run ID.
    pub id: Uuid,
    /// Which job this run belongs to.
    pub job_id: Uuid,
    /// When execution started.
    pub started_at: DateTime<Utc>,
    /// When execution finished (None if still running).
    pub finished_at: Option<DateTime<Utc>>,
    /// Run status.
    pub status: JobRunStatus,
    /// Result payload (empty if not yet finished or no output).
    pub result: Vec<u8>,
    /// Error message (None if succeeded or still running).
    pub error: Option<String>,
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

    #[test]
    fn job_owner_from_parts() {
        assert!(matches!(
            JobOwner::from_parts("system", None).unwrap(),
            JobOwner::System
        ));

        let id = Uuid::now_v7();
        assert!(matches!(
            JobOwner::from_parts("user", Some(id)).unwrap(),
            JobOwner::User(uid) if uid == id
        ));

        assert!(JobOwner::from_parts("user", None).is_err());
        assert!(JobOwner::from_parts("unknown", None).is_err());
    }

    #[test]
    fn job_status_roundtrip() {
        for status in [
            JobStatus::Active,
            JobStatus::Paused,
            JobStatus::Cancelled,
            JobStatus::Running,
        ] {
            let s = status.as_str();
            assert_eq!(JobStatus::from_str_value(s).unwrap(), status);
        }
    }

    #[test]
    fn job_advance_next_run() {
        let now = Utc::now();
        let mut job = Job {
            id: Uuid::now_v7(),
            name: "test".into(),
            owner: JobOwner::System,
            schedule: JobSchedule::parse("every: 30s").unwrap(),
            payload: vec![],
            status: JobStatus::Active,
            next_run_at: now,
            last_run_at: None,
            created_at: now,
            notify_agent: false,
        };
        job.advance_next_run(now);
        assert_eq!((job.next_run_at - now).num_seconds(), 30);
    }
}
