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

impl CommandCard {
    pub fn new(command: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            command,
            state: CommandState::Pending,
            exit_code: None,
            output: String::new(),
            created_at: Utc::now(),
            resolved_at: None,
            resolved_by: None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_card_initial_state() {
        let card = CommandCard::new("echo hello".to_string());
        assert_eq!(card.command, "echo hello");
        assert_eq!(card.state, CommandState::Pending);
        assert!(card.exit_code.is_none());
        assert!(card.resolved_at.is_none());
        assert!(card.resolved_by.is_none());
        assert!(card.output.is_empty());
    }
}
