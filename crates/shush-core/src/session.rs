use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Pending,
    Executing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CommandState {
    Pending,
    Executing,
    Completed(i32),
    Rejected,
    Aborted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCard {
    pub id: Uuid,
    pub command: String,
    pub state: CommandState,
    pub exit_code: Option<i32>,
    pub output: String,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub state: SessionState,
    pub yolo: bool,
    pub current_command: Option<CommandCard>,
    pub created_at: DateTime<Utc>,
}
