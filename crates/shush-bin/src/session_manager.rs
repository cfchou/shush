use shush_core::session::{Session, SessionState};
use std::collections::HashMap;
use std::sync::RwLock;
use uuid::Uuid;

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
        self.sessions.write().unwrap().remove(&id).is_some()
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions.read().unwrap().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
