use crate::remote_tmux;
use shush_core::session::{Session, SessionState};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing::error;
use uuid::Uuid;

const MONITOR_COLS: &str = "220";
const MONITOR_ROWS: &str = "50";

pub struct SessionManager {
    sessions: RwLock<HashMap<Uuid, Session>>,
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
                // swallow
                false
            });

        // Create a detached session if it doesn't exist
        if !exists {
            // '-d' detaches the session
            let _ = remote_tmux::run_tmux_status(&host, &["new-session", "-d", "-s", &name]);
        }

        // Configure the tmux session's window size to be fixed and
        // deterministic.
        //
        // By default, tmux's window-size option is set to latest — meaning
        // the window automatically resizes to match the size of the most
        // recently attached client.
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
            created_at: chrono::Utc::now(),
        };
        let id = session.id;
        self.sessions.write().unwrap().insert(id, session.clone());
        session
    }

    pub fn get(&self, id: Uuid) -> Option<Session> {
        self.sessions.read().unwrap().get(&id).cloned()
    }

    pub fn delete(&self, id: Uuid) -> bool {
        if let Some(session) = self.sessions.read().unwrap().get(&id).cloned() {
            let _ =
                remote_tmux::run_tmux_status(&session.host, &["kill-session", "-t", &session.name]);
        }

        self.sessions.write().unwrap().remove(&id).is_some()
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions.read().unwrap().values().cloned().collect()
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
