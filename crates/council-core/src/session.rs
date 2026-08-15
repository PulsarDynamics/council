//! A Council session is one goal the user typed in, from creation to completion.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SessionId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub goal: String,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl Session {
    pub fn new(goal: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            goal: goal.into(),
            status: SessionStatus::Pending,
            created_at: Utc::now(),
            completed_at: None,
        }
    }

    pub fn mark_running(&mut self) {
        self.status = SessionStatus::Running;
    }

    pub fn mark_completed(&mut self) {
        self.status = SessionStatus::Completed;
        self.completed_at = Some(Utc::now());
    }

    pub fn mark_failed(&mut self) {
        self.status = SessionStatus::Failed;
        self.completed_at = Some(Utc::now());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_starts_pending() {
        let s = Session::new("build a thing");
        assert_eq!(s.status, SessionStatus::Pending);
        assert!(s.completed_at.is_none());
    }

    #[test]
    fn mark_completed_sets_timestamp() {
        let mut s = Session::new("x");
        s.mark_completed();
        assert_eq!(s.status, SessionStatus::Completed);
        assert!(s.completed_at.is_some());
    }
}
