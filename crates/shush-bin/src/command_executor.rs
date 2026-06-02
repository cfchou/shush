use crate::{
    fe_master::FeMasterRegistry,
    session_manager::SessionManager,
    tmux_control::{TmuxControlModeClient, TmuxControlModeRegistry},
};
use shush_core::marker::{MarkerDetector, MarkerEvent, MarkerInjector, Nonce};
use shush_core::session::CommandState;
use shush_core::tmux_event::TmuxEvent;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

const EXECUTION_TIMEOUT: Duration = Duration::from_secs(15);

pub struct CommandExecutor {
    session_manager: Arc<SessionManager>,
    fe_masters: Arc<FeMasterRegistry>,
    control_mode: Arc<TmuxControlModeRegistry>,
}

impl CommandExecutor {
    pub fn new(
        session_manager: Arc<SessionManager>,
        fe_masters: Arc<FeMasterRegistry>,
        control_mode: Arc<TmuxControlModeRegistry>,
    ) -> Self {
        Self {
            session_manager,
            fe_masters,
            control_mode,
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
                .control_mode
                .get_or_spawn(session_id, session.name.clone(), session.host.clone())
                .await?;

            let mut control = handle.lock().await;

            let injector = MarkerInjector::new();
            let (wrapped_command, nonce) = injector.inject(&current.command);
            self.fe_masters
                .queue_command_echo_replacement(session_id, current.command.as_bytes())
                .await;
            control.send_keys(&wrapped_command).await?;
            control.send_enter().await?;

            let (exit_code, raw_output) = wait_for_completion(&mut *control, &nonce).await?;
            let output = extract_command_output(&raw_output, &nonce, exit_code);
            let resolved_by = if session.yolo { "yolo" } else { "human" };
            self.session_manager
                .complete_command(session_id, exit_code, output, resolved_by)
                .map_err(|err| format!("failed to complete command: {err:?}"))?;
            Ok(())
        }
        .await;

        if let Err(err) = execution {
            self.control_mode.remove(session_id).await;
            let _ = self.session_manager.fail_command(session_id, err.clone());
            return Err(err);
        }

        Ok(())
    }
}

async fn wait_for_completion(
    client: &mut TmuxControlModeClient,
    nonce: &Nonce,
) -> Result<(i32, Vec<u8>), String> {
    let target_nonce = nonce.hex();
    let mut detector = MarkerDetector::new();
    let mut raw_output = Vec::new();

    tokio::time::timeout(EXECUTION_TIMEOUT, async {
        loop {
            match client.read_event().await {
                Some(TmuxEvent::Output { data, .. }) => {
                    raw_output.extend_from_slice(&data);
                    for event in detector.feed(&data) {
                        match event {
                            MarkerEvent::Start(found) if found.hex() == target_nonce => {}
                            MarkerEvent::End(found, exit_code) if found.hex() == target_nonce => {
                                return Ok((exit_code, raw_output));
                            }
                            _ => {}
                        }
                    }
                }
                Some(TmuxEvent::Error(_, _, _, message)) => {
                    return Err(format!("tmux error: {message}"));
                }
                Some(TmuxEvent::Unknown(_))
                | Some(TmuxEvent::Begin(_, _, _))
                | Some(TmuxEvent::End(_, _, _))
                | Some(TmuxEvent::WindowAdd(_))
                | Some(TmuxEvent::SessionChanged(_, _)) => continue,
                None => return Err("terminal stream closed".to_string()),
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
    use tokio::io::{self, AsyncWriteExt};

    fn octal_escape_bytes(input: &[u8]) -> String {
        input.iter().map(|byte| format!("\\{:03o}", byte)).collect()
    }

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
        let (_, nonce) = MarkerInjector::new().inject("echo hello");
        let mut payload = Vec::new();
        payload.extend_from_slice(format!("\x1b_BEGIN_{}\x1b\\", nonce.hex()).as_bytes());
        payload.extend_from_slice(b"hello world\n");
        payload.extend_from_slice(format!("\x1b_END_{}_{}\x1b\\", nonce.hex(), 7).as_bytes());

        let line = format!("%output %0 {}\n", octal_escape_bytes(&payload));
        let line_for_writer = line.clone();

        let (client_stdin, _mock_stdin) = io::duplex(1024);
        let (mut mock_stdout, client_stdout) = io::duplex(1024);
        let mut client =
            TmuxControlModeClient::from_streams(Box::new(client_stdin), Box::new(client_stdout));

        let write_task = tokio::spawn(async move {
            mock_stdout
                .write_all(line_for_writer.as_bytes())
                .await
                .unwrap();
        });

        let (exit_code, raw_output) = wait_for_completion(&mut client, &nonce).await.unwrap();
        write_task.await.unwrap();

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
