use crate::{fe_master, fe_master::FeMasterRegistry, session_manager::SessionManager};
use axum::{
    Router,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use serde::Deserialize;
use shush_core::session::Session;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub session_manager: Arc<SessionManager>,
    pub fe_masters: Arc<FeMasterRegistry>,
}

#[derive(Deserialize)]
struct CreateSessionRequest {
    name: String,
    host: String,
}

pub fn create_app(session_manager: SessionManager) -> Router {
    let state = AppState {
        session_manager: Arc::new(session_manager),
        fe_masters: Arc::new(FeMasterRegistry::new()),
    };

    let api = Router::new()
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/:id", get(get_session).delete(delete_session))
        .route("/sessions/:id/stream", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let static_files = ServeDir::new("frontend/dist")
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new("frontend/dist/index.html"));

    Router::new()
        .nest("/api", api)
        .fallback_service(static_files)
}

async fn list_sessions(State(state): State<AppState>) -> Json<Vec<Session>> {
    Json(state.session_manager.list())
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let session = state.session_manager.create(req.name, req.host);
    (StatusCode::CREATED, Json(session))
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Session>, StatusCode> {
    state
        .session_manager
        .get(id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn delete_session(State(state): State<AppState>, Path(id): Path<Uuid>) -> StatusCode {
    if state.session_manager.delete(id) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[derive(serde::Serialize)]
struct StreamMessage<'a> {
    #[serde(rename = "type")]
    msg_type: &'a str,
    data: &'a str,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, StatusCode> {
    let Some(session) = state.session_manager.get(id) else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(ws.on_upgrade(move |socket| handle_socket(state, socket, id, session.name)))
}

async fn handle_socket(
    state: AppState,
    mut socket: WebSocket,
    session_id: Uuid,
    session_name: String,
) {
    let registry = Arc::clone(&state.fe_masters);
    let handle = match registry.get_or_spawn(session_id, session_name).await {
        Ok(h) => h,
        Err(_) => {
            let _ = socket.close().await;
            return;
        }
    };

    let snapshot = match handle.snapshot().await {
        Ok(s) => s,
        Err(_) => {
            let _ = socket.close().await;
            let _ = registry.on_disconnect(session_id).await;
            return;
        }
    };

    let snapshot_msg = serde_json::to_string(&StreamMessage {
        msg_type: "snapshot",
        data: &snapshot,
    });

    let Ok(snapshot_msg) = snapshot_msg else {
        let _ = socket.close().await;
        let _ = registry.on_disconnect(session_id).await;
        return;
    };

    if socket
        .send(Message::Text(snapshot_msg.into()))
        .await
        .is_err()
    {
        let _ = registry.on_disconnect(session_id).await;
        return;
    }

    let mut rx = handle.subscribe();

    loop {
        tokio::select! {
            recv = rx.recv() => {
                let Ok(chunk) = recv else {
                    break;
                };
                let encoded = fe_master::encode_terminal_chunk(&chunk);
                let message = StreamMessage { msg_type: "terminal", data: &encoded };
                let Ok(payload) = serde_json::to_string(&message) else {
                    break;
                };
                if socket.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
            }
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(_)) => {
                        // Read-only stream; ignore client messages.
                    }
                    Some(Err(_)) | None => break,
                }
            }
        }
    }

    registry.on_disconnect(session_id).await;
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as WsMessage;
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

    #[tokio::test]
    async fn websocket_missing_session_returns_not_found() {
        let mgr = SessionManager::new();
        let app = super::create_app(mgr);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{}/api/sessions/{}/stream", addr, Uuid::new_v4());
        let err = tokio_tungstenite::connect_async(url).await.unwrap_err();
        match err {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                assert_eq!(response.status(), StatusCode::NOT_FOUND)
            }
            other => panic!("expected HTTP handshake error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn websocket_connect_sends_snapshot_message() {
        let mgr = SessionManager::new();
        let session = mgr.create("ws-snapshot".to_string(), "".to_string());
        let app = super::create_app(mgr);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{}/api/sessions/{}/stream", addr, session.id);
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();

        let frame = ws.next().await.unwrap().unwrap();
        let text = match frame {
            WsMessage::Text(t) => t,
            other => panic!("expected text frame, got {other:?}"),
        };
        let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(payload["type"], "snapshot");
        assert!(payload["data"].is_string());

        let _ = ws.send(WsMessage::Close(None)).await;
    }

    #[tokio::test]
    async fn websocket_connect_creates_readonly_tmux_client() {
        let mgr = SessionManager::new();
        let session = mgr.create("ws-client-tty".to_string(), "".to_string());
        let app = super::create_app(mgr);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{}/api/sessions/{}/stream", addr, session.id);
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let _ = ws.next().await.unwrap().unwrap();

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut clients_output = String::new();
        while tokio::time::Instant::now() < deadline {
            let output = tokio::process::Command::new("tmux")
                .args([
                    "-L",
                    "shush",
                    "list-clients",
                    "-t",
                    &session.name,
                    "-F",
                    "#{client_tty} readonly=#{client_readonly} session=#{session_name}",
                ])
                .output()
                .await
                .unwrap();
            clients_output = String::from_utf8_lossy(&output.stdout).to_string();
            if clients_output.contains("readonly=1") && clients_output.contains(&session.name) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        assert!(
            clients_output.contains("readonly=1") && clients_output.contains(&session.name),
            "expected readonly tmux client for session, got: {clients_output:?}"
        );

        let _ = ws.send(WsMessage::Close(None)).await;
    }

    #[tokio::test]
    async fn websocket_reconnect_receives_new_snapshot() {
        let mgr = SessionManager::new();
        let session = mgr.create("ws-reconnect".to_string(), "".to_string());
        let app = super::create_app(mgr);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{}/api/sessions/{}/stream", addr, session.id);
        let (mut ws1, _) = tokio_tungstenite::connect_async(url.clone()).await.unwrap();
        let first = ws1.next().await.unwrap().unwrap();
        let first_text = match first {
            WsMessage::Text(t) => t,
            other => panic!("expected text frame, got {other:?}"),
        };
        let first_payload: serde_json::Value = serde_json::from_str(&first_text).unwrap();
        assert_eq!(first_payload["type"], "snapshot");
        let _ = ws1.send(WsMessage::Close(None)).await;

        let (mut ws2, _) = tokio_tungstenite::connect_async(url).await.unwrap();
        let second = ws2.next().await.unwrap().unwrap();
        let second_text = match second {
            WsMessage::Text(t) => t,
            other => panic!("expected text frame, got {other:?}"),
        };
        let second_payload: serde_json::Value = serde_json::from_str(&second_text).unwrap();
        assert_eq!(second_payload["type"], "snapshot");
    }

    #[tokio::test]
    async fn websocket_allows_multiple_viewers_same_session() {
        let mgr = SessionManager::new();
        let session = mgr.create("ws-multi".to_string(), "".to_string());
        let app = super::create_app(mgr);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("ws://{}/api/sessions/{}/stream", addr, session.id);
        let (mut ws1, _) = tokio_tungstenite::connect_async(url.clone()).await.unwrap();
        let (mut ws2, _) = tokio_tungstenite::connect_async(url).await.unwrap();

        let msg1 = ws1.next().await.unwrap().unwrap();
        let msg2 = ws2.next().await.unwrap().unwrap();

        let t1 = match msg1 {
            WsMessage::Text(t) => t,
            other => panic!("expected text frame, got {other:?}"),
        };
        let t2 = match msg2 {
            WsMessage::Text(t) => t,
            other => panic!("expected text frame, got {other:?}"),
        };
        let p1: serde_json::Value = serde_json::from_str(&t1).unwrap();
        let p2: serde_json::Value = serde_json::from_str(&t2).unwrap();
        assert_eq!(p1["type"], "snapshot");
        assert_eq!(p2["type"], "snapshot");

        let _ = ws1.send(WsMessage::Close(None)).await;
        let _ = ws2.send(WsMessage::Close(None)).await;
    }
}
