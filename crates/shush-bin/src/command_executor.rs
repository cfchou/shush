use crate::{
    fe_master::{FeEvent, FeMasterRegistry},
    remote_tmux,
    session_manager::SessionManager,
};
use shush_core::marker::{MarkerDetector, MarkerEvent, MarkerInjector, Nonce};
use shush_core::session::CommandState;
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;
use uuid::Uuid;

const EXECUTION_TIMEOUT: Duration = Duration::from_secs(15);

pub struct CommandExecutor {
    session_manager: Arc<SessionManager>,
    fe_masters: Arc<FeMasterRegistry>,
}

impl CommandExecutor {
    pub fn new(session_manager: Arc<SessionManager>, fe_masters: Arc<FeMasterRegistry>) -> Self {
        Self {
            session_manager,
            fe_masters,
        }
    }

    pub async fn execute_approved_command(&self, session_id: Uuid) -> Result<(), String> {
        let session = self
            .session_manager
            .get(session_id)
            .ok_or_else(|| "session not found".to_string())?;
        let current = session
            .current_command
            .clone()
            .ok_or_else(|| "current command missing".to_string())?;
        if current.state != CommandState::Executing {
            return Err("current command is not executing".to_string());
        }

        let execution: Result<(), String> = async {
            let handle = self
                .fe_masters
                .get_or_spawn(session_id, session.name.clone(), session.host.clone())
                .await?;
            let mut rx = handle.subscribe();

            let injector = MarkerInjector::new();
            let (wrapped_command, nonce) = injector.inject(&current.command);
            inject_wrapped_command(&session.host, &session.name, &wrapped_command).await?;

            let (exit_code, raw_output) = wait_for_completion(&mut rx, &nonce).await?;
            let output = extract_command_output(&raw_output, &nonce, exit_code);
            let resolved_by = if session.yolo { "yolo" } else { "human" };
            self.session_manager
                .complete_command(session_id, exit_code, output, resolved_by)
                .map_err(|err| format!("failed to complete command: {err:?}"))?;
            Ok(())
        }
        .await;

        if let Err(err) = execution {
            let _ = self.session_manager.fail_command(session_id, err.clone());
            return Err(err);
        }

        Ok(())
    }
}

async fn inject_wrapped_command(host: &str, session_name: &str, wrapped_command: &str) -> Result<(), String> {
    let literal_send = if remote_tmux::is_local_host(host) {
        remote_tmux::run_tmux_status_async(host, &["send-keys", "-l", "-t", session_name, wrapped_command])
            .await
            .map_err(|err| format!("send-keys -l failed: {err}"))?
    } else {
        remote_tmux::run_tmux_shell_status_async(
            host,
            &["send-keys", "-l", "-t", session_name, wrapped_command],
        )
        .await
        .map_err(|err| format!("remote shell send-keys -l failed: {err}"))?
    };

    if !literal_send.success() {
        return Err(format!("send-keys -l exited with {literal_send}"));
    }

    let enter_send = remote_tmux::run_tmux_status_async(host, &["send-keys", "-t", session_name, "Enter"])
        .await
        .map_err(|err| format!("send-keys Enter failed: {err}"))?;

    if !enter_send.success() {
        return Err(format!("send-keys Enter exited with {enter_send}"));
    }

    Ok(())
}

async fn wait_for_completion(
    rx: &mut broadcast::Receiver<FeEvent>,
    nonce: &Nonce,
) -> Result<(i32, Vec<u8>), String> {
    let target_nonce = nonce.hex();
    let mut detector = MarkerDetector::new();
    let mut raw_output = Vec::new();

    tokio::time::timeout(EXECUTION_TIMEOUT, async {
        loop {
            match rx.recv().await {
                Ok(FeEvent::Chunk(chunk)) => {
                    raw_output.extend_from_slice(&chunk);
                    for event in detector.feed(&chunk) {
                        match event {
                            MarkerEvent::Start(found) if found.hex() == target_nonce => {}
                            MarkerEvent::End(found, exit_code) if found.hex() == target_nonce => {
                                return Ok((exit_code, raw_output));
                            }
                            _ => {}
                        }
                    }
                }
                Ok(FeEvent::Closed) => return Err("terminal stream closed".to_string()),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err("terminal stream closed".to_string());
                }
            }
        }
    })
    .await
    .map_err(|_| "timed out waiting for command completion".to_string())?
}

fn extract_command_output(raw: &[u8], nonce: &Nonce, exit_code: i32) -> String {
    let start = format!("\x1b_BEGIN_{}\x1b\\", nonce.hex());
    let end = format!("\x1b_END_{}_{}\x1b\\", nonce.hex(), exit_code);
    let output = match (
        find_subsequence(raw, start.as_bytes()),
        find_subsequence(raw, end.as_bytes()),
    ) {
        (Some(start_index), Some(end_index)) if end_index >= start_index + start.len() => {
            &raw[start_index + start.len()..end_index]
        }
        _ => raw,
    };

    String::from_utf8_lossy(output).to_string()
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::broadcast;

    #[test]
    fn extract_command_output_strips_markers() {
        let (_, nonce) = MarkerInjector::new().inject("echo hello");
        let raw = format!(
            "noise\x1b_BEGIN_{}\x1b\\hello world\n\x1b_END_{}_0\x1b\\tail",
            nonce.hex(),
            nonce.hex()
        );

        let output = extract_command_output(raw.as_bytes(), &nonce, 0);

        assert_eq!(output, "hello world\n");
    }

    #[tokio::test]
    async fn wait_for_completion_detects_matching_end_marker_and_exit_code() {
        let (sender, mut rx) = broadcast::channel(8);
        let (_, nonce) = MarkerInjector::new().inject("echo hello");
        let chunk = format!(
            "prefix\x1b_BEGIN_{}\x1b\\hello world\n\x1b_END_{}_7\x1b\\suffix",
            nonce.hex(),
            nonce.hex()
        );

        let send_task = tokio::spawn(async move {
            let _ = sender.send(FeEvent::Chunk(chunk.into_bytes()));
        });

        let (exit_code, raw_output) = wait_for_completion(&mut rx, &nonce).await.unwrap();
        send_task.await.unwrap();

        assert_eq!(exit_code, 7);
        assert_eq!(
            extract_command_output(&raw_output, &nonce, exit_code),
            "hello world\n"
        );
    }

    #[test]
    fn extract_command_output_falls_back_to_raw_when_markers_missing() {
        let (_, nonce) = MarkerInjector::new().inject("echo hello");

        let output = extract_command_output(b"literal output\n", &nonce, 0);

        assert_eq!(output, "literal output\n");
    }
}
