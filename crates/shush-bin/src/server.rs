use crate::session_manager::SessionManager;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use serde::Deserialize;
use shush_core::session::Session;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

#[derive(Deserialize)]
struct CreateSessionRequest {
    name: String,
    host: String,
}

pub fn create_app(session_manager: SessionManager) -> Router {
    let state = Arc::new(session_manager);

    let api = Router::new()
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/:id", get(get_session).delete(delete_session))
        .layer(CorsLayer::permissive())
        .with_state(state);

    Router::new().nest("/api", api).fallback_service(
        tower_http::services::ServeDir::new("frontend/dist").append_index_html_on_directories(true),
    )
}

async fn list_sessions(State(mgr): State<Arc<SessionManager>>) -> Json<Vec<Session>> {
    Json(mgr.list())
}

async fn create_session(
    State(mgr): State<Arc<SessionManager>>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let session = mgr.create(req.name, req.host);
    (StatusCode::CREATED, Json(session))
}

async fn get_session(
    State(mgr): State<Arc<SessionManager>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Session>, StatusCode> {
    mgr.get(id).map(Json).ok_or(StatusCode::NOT_FOUND)
}

async fn delete_session(
    State(mgr): State<Arc<SessionManager>>,
    Path(id): Path<Uuid>,
) -> StatusCode {
    if mgr.delete(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use tower::util::ServiceExt;
    use uuid::Uuid;

    use crate::session_manager::SessionManager;

    #[tokio::test]
    async fn list_sessions_returns_empty() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);
        let response = app
            .oneshot(Request::get("/api/sessions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn create_session_returns_created() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);
        let response = app
            .oneshot(
                Request::post("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"test","host":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(session["name"], "test");
        assert!(session["id"].is_string());
    }

    #[tokio::test]
    async fn get_session_by_id() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);

        // Create a session first (clone the router to keep the state alive)
        let create_resp = app
            .clone()
            .oneshot(
                Request::post("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"by-id","host":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_str().unwrap();

        // Get by id
        let response = app
            .oneshot(
                Request::get(format!("/api/sessions/{}", id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(session["name"], "by-id");
    }

    #[tokio::test]
    async fn get_session_not_found() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);
        let response = app
            .oneshot(
                Request::get(format!("/api/sessions/{}", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_session_removes() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);

        let create_resp = app
            .clone()
            .oneshot(
                Request::post("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"name":"del","host":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_str().unwrap();

        let delete_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri(format!("/api/sessions/{}", id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_resp.status(), StatusCode::NO_CONTENT);

        let list_resp = app
            .oneshot(Request::get("/api/sessions").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let sessions: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(sessions.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn delete_missing_returns_not_found() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);
        let response = app
            .oneshot(
                Request::builder()
                    .method(http::Method::DELETE)
                    .uri(format!("/api/sessions/{}", Uuid::new_v4()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
