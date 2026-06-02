use crate::remote_tmux;
use chrono::Utc;
use shush_core::session::{CommandCard, CommandState, Session, SessionState};
use std::collections::HashMap;
use std::sync::RwLock;
use tokio::sync::broadcast;
use tracing::error;
use uuid::Uuid;

const MONITOR_COLS: &str = "220";
const MONITOR_ROWS: &str = "50";
const CARD_BROADCAST_CAPACITY: usize = 64;

struct ManagedSession {
    session: Session,

    // Past(completed, not active) commands
    history: Vec<CommandCard>,

    // Using broadcast channel to allow multiple viewers to receive command
    // card updates
    card_sender: broadcast::Sender<CommandCard>,
}

pub struct SessionManager {
    sessions: RwLock<HashMap<Uuid, ManagedSession>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommandError {
    NotFound,
    CommandAlreadyActive,
    PendingCommandRequired,
    ExecutingCommandRequired,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn create(&self, name: String, host: String) -> Session {
        let exists = remote_tmux::run_tmux_status(&host, &["has-session", "-t", &name])
            .map(|status| status.success())
            .unwrap_or_else(|e| {
                error!(error = ?e, "Failed to check tmux session existence");
                false
            });

        if !exists {
            let _ = remote_tmux::run_tmux_status(&host, &["new-session", "-d", "-s", &name]);
        }

        let _ = remote_tmux::run_tmux_status(
            &host,
            &["set-window-option", "-t", &name, "window-size", "manual"],
        );
        let _ = remote_tmux::run_tmux_status(
            &host,
            &[
                "resize-window",
                "-t",
                &name,
                "-x",
                MONITOR_COLS,
                "-y",
                MONITOR_ROWS,
            ],
        );

        let session = Session {
            id: Uuid::new_v4(),
            name,
            host,
            state: SessionState::Idle,
            yolo: false,
            current_command: None,
            created_at: Utc::now(),
        };
        let (card_sender, _) = broadcast::channel(CARD_BROADCAST_CAPACITY);
        let id = session.id;
        self.sessions.write().unwrap().insert(
            id,
            ManagedSession {
                session: session.clone(),
                history: Vec::new(),
                card_sender,
            },
        );
        session
    }

    pub fn get(&self, id: Uuid) -> Option<Session> {
        self.sessions
            .read()
            .unwrap()
            .get(&id)
            .map(|managed| managed.session.clone())
    }

    pub fn delete(&self, id: Uuid) -> bool {
        if let Some(session) = self.get(id) {
            let _ =
                remote_tmux::run_tmux_status(&session.host, &["kill-session", "-t", &session.name]);
        }

        self.sessions.write().unwrap().remove(&id).is_some()
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions
            .read()
            .unwrap()
            .values()
            .map(|managed| managed.session.clone())
            .collect()
    }

    pub fn submit_command(
        &self,
        id: Uuid,
        command: String,
    ) -> Result<Session, SessionCommandError> {
        let (session, sender, card) = {
            let mut sessions = self.sessions.write().unwrap();
            let managed = sessions.get_mut(&id).ok_or(SessionCommandError::NotFound)?;
            if managed.session.current_command.is_some() {
                return Err(SessionCommandError::CommandAlreadyActive);
            }

            let card = CommandCard::new(command);
            managed.session.state = SessionState::Pending;
            managed.session.current_command = Some(card.clone());
            (managed.session.clone(), managed.card_sender.clone(), card)
        };
        let _ = sender.send(card);
        Ok(session)
    }

    pub fn approve_command(&self, id: Uuid) -> Result<Session, SessionCommandError> {
        let (session, sender, card) = {
            let mut sessions = self.sessions.write().unwrap();
            let managed = sessions.get_mut(&id).ok_or(SessionCommandError::NotFound)?;
            let Some(current) = managed.session.current_command.as_mut() else {
                return Err(SessionCommandError::PendingCommandRequired);
            };
            if current.state != CommandState::Pending {
                return Err(SessionCommandError::PendingCommandRequired);
            }

            current.state = CommandState::Executing;
            let card = current.clone();
            managed.session.state = SessionState::Executing;
            (managed.session.clone(), managed.card_sender.clone(), card)
        };
        let _ = sender.send(card);
        Ok(session)
    }

    pub fn deny_command(&self, id: Uuid) -> Result<Session, SessionCommandError> {
        let (session, sender, card) = {
            let mut sessions = self.sessions.write().unwrap();
            let managed = sessions.get_mut(&id).ok_or(SessionCommandError::NotFound)?;
            let Some(current) = managed.session.current_command.take() else {
                return Err(SessionCommandError::PendingCommandRequired);
            };
            if current.state != CommandState::Pending {
                managed.session.current_command = Some(current);
                return Err(SessionCommandError::PendingCommandRequired);
            }

            let mut rejected = current;
            rejected.state = CommandState::Rejected;
            rejected.resolved_at = Some(Utc::now());
            rejected.resolved_by = Some("human".to_string());
            managed.session.state = SessionState::Idle;
            managed.history.push(rejected.clone());
            (
                managed.session.clone(),
                managed.card_sender.clone(),
                rejected,
            )
        };
        let _ = sender.send(card);
        Ok(session)
    }

    pub fn complete_command(
        &self,
        id: Uuid,
        exit_code: i32,
        output: String,
        resolved_by: &str,
    ) -> Result<Session, SessionCommandError> {
        let (session, sender, card) = {
            let mut sessions = self.sessions.write().unwrap();
            let managed = sessions.get_mut(&id).ok_or(SessionCommandError::NotFound)?;
            let Some(current) = managed.session.current_command.take() else {
                return Err(SessionCommandError::ExecutingCommandRequired);
            };
            if current.state != CommandState::Executing {
                managed.session.current_command = Some(current);
                return Err(SessionCommandError::ExecutingCommandRequired);
            }

            let mut completed = current;
            completed.state = CommandState::Completed;
            completed.exit_code = Some(exit_code);
            completed.output = output;
            completed.resolved_at = Some(Utc::now());
            completed.resolved_by = Some(resolved_by.to_string());
            managed.session.state = SessionState::Idle;
            managed.history.push(completed.clone());
            (
                managed.session.clone(),
                managed.card_sender.clone(),
                completed,
            )
        };
        let _ = sender.send(card);
        Ok(session)
    }

    pub fn fail_command(&self, id: Uuid, output: String) -> Result<Session, SessionCommandError> {
        let (session, sender, card) = {
            let mut sessions = self.sessions.write().unwrap();
            let managed = sessions.get_mut(&id).ok_or(SessionCommandError::NotFound)?;
            let Some(current) = managed.session.current_command.take() else {
                return Err(SessionCommandError::ExecutingCommandRequired);
            };

            let mut aborted = current;
            aborted.state = CommandState::Aborted;
            aborted.output = output;
            aborted.resolved_at = Some(Utc::now());
            aborted.resolved_by = Some("system".to_string());
            managed.session.state = SessionState::Idle;
            managed.history.push(aborted.clone());
            (
                managed.session.clone(),
                managed.card_sender.clone(),
                aborted,
            )
        };
        let _ = sender.send(card);
        Ok(session)
    }

    pub fn list_commands(
        &self,
        id: Uuid,
        limit: usize,
    ) -> Result<Vec<CommandCard>, SessionCommandError> {
        let sessions = self.sessions.read().unwrap();
        let managed = sessions.get(&id).ok_or(SessionCommandError::NotFound)?;
        Ok(managed.history.iter().rev().take(limit).cloned().collect())
    }

    pub fn subscribe_cards(
        &self,
        id: Uuid,
    ) -> Result<broadcast::Receiver<CommandCard>, SessionCommandError> {
        let sessions = self.sessions.read().unwrap();
        let managed = sessions.get(&id).ok_or(SessionCommandError::NotFound)?;
        Ok(managed.card_sender.subscribe())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::process::Command;

    #[test]
    fn new_manager_list_is_empty() {
        let mgr = SessionManager::new();
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn create_adds_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("test-session".into(), "".into());
        assert_eq!(session.name, "test-session");
        let list = mgr.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, session.id);
    }

    #[test]
    fn get_returns_created_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("s1".into(), "".into());
        let fetched = mgr.get(session.id).expect("should exist");
        assert_eq!(fetched.name, session.name);
        assert_eq!(fetched.id, session.id);
    }

    #[test]
    fn get_missing_returns_none() {
        let mgr = SessionManager::new();
        assert!(mgr.get(Uuid::new_v4()).is_none());
    }

    #[test]
    fn delete_removes_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("to-delete".into(), "".into());
        let id = session.id;
        assert!(mgr.delete(id));
        assert!(mgr.get(id).is_none());
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn delete_missing_returns_false() {
        let mgr = SessionManager::new();
        assert!(!mgr.delete(Uuid::new_v4()));
    }

    #[test]
    fn submit_command_sets_pending_current_command() {
        let mgr = SessionManager::new();
        let session = mgr.create("cmd-submit".into(), "".into());

        let updated = mgr
            .submit_command(session.id, "echo hello".into())
            .expect("submit should succeed");

        assert_eq!(updated.state, SessionState::Pending);
        let current = updated.current_command.expect("command should be present");
        assert_eq!(current.command, "echo hello");
        assert_eq!(current.state, CommandState::Pending);
    }

    #[test]
    fn submit_command_rejects_second_active_command() {
        let mgr = SessionManager::new();
        let session = mgr.create("cmd-conflict".into(), "".into());
        mgr.submit_command(session.id, "echo one".into()).unwrap();

        let err = mgr
            .submit_command(session.id, "echo two".into())
            .expect_err("second command should fail");

        assert_eq!(err, SessionCommandError::CommandAlreadyActive);
    }

    #[test]
    fn approve_command_marks_session_executing() {
        let mgr = SessionManager::new();
        let session = mgr.create("cmd-approve".into(), "".into());
        mgr.submit_command(session.id, "echo run".into()).unwrap();

        let updated = mgr.approve_command(session.id).unwrap();

        assert_eq!(updated.state, SessionState::Executing);
        assert_eq!(
            updated
                .current_command
                .expect("command should be present")
                .state,
            CommandState::Executing
        );
    }

    #[test]
    fn deny_command_moves_card_to_history() {
        let mgr = SessionManager::new();
        let session = mgr.create("cmd-deny".into(), "".into());
        mgr.submit_command(session.id, "echo no".into()).unwrap();

        let updated = mgr.deny_command(session.id).unwrap();

        assert_eq!(updated.state, SessionState::Idle);
        assert!(updated.current_command.is_none());
        let cards = mgr.list_commands(session.id, 10).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].state, CommandState::Rejected);
    }

    #[test]
    fn complete_command_moves_card_to_history() {
        let mgr = SessionManager::new();
        let session = mgr.create("cmd-complete".into(), "".into());
        mgr.submit_command(session.id, "echo yes".into()).unwrap();
        mgr.approve_command(session.id).unwrap();

        let updated = mgr
            .complete_command(session.id, 0, "yes\n".into(), "human")
            .unwrap();

        assert_eq!(updated.state, SessionState::Idle);
        assert!(updated.current_command.is_none());
        let cards = mgr.list_commands(session.id, 10).unwrap();
        assert_eq!(cards[0].state, CommandState::Completed);
        assert_eq!(cards[0].exit_code, Some(0));
        assert_eq!(cards[0].output, "yes\n");
    }

    #[tokio::test]
    async fn create_sets_manual_window_size_for_monitor_stability() {
        let mgr = SessionManager::new();
        let session = mgr.create(format!("size-lock-{}", Uuid::new_v4()), "".into());

        let option_output = Command::new("tmux")
            .args([
                "-L",
                remote_tmux::TMUX_SOCKET,
                "show-window-options",
                "-t",
                &session.name,
                "window-size",
            ])
            .output()
            .await
            .unwrap();
        assert!(option_output.status.success());
        let option_text = String::from_utf8_lossy(&option_output.stdout);
        assert!(option_text.contains("window-size manual"));

        let size_output = Command::new("tmux")
            .args([
                "-L",
                remote_tmux::TMUX_SOCKET,
                "display-message",
                "-p",
                "-t",
                &session.name,
                "#{window_width}x#{window_height}",
            ])
            .output()
            .await
            .unwrap();
        assert!(size_output.status.success());
        let size_text = String::from_utf8_lossy(&size_output.stdout);
        assert_eq!(
            size_text.trim(),
            format!("{}x{}", MONITOR_COLS, MONITOR_ROWS)
        );
    }
}
