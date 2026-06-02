use crate::remote_tmux;
use base64::Engine;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    collections::HashMap,
    os::unix::io::{FromRawFd, RawFd},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::broadcast, task::JoinHandle};
use tracing::{debug, info, warn};
use uuid::Uuid;

const FE_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const FE_BROADCAST_CAPACITY: usize = 512;
const FE_REPLAY_BUFFER_MAX_BYTES: usize = 128 * 1024;
const MARKER_ECHO_PREFIX: &[u8] = b"printf '\\033_BEGIN_";
const MARKER_ECHO_SUFFIX: &[u8] = b"; unset __shush_exit";
const MARKER_ECHO_MIDDLE: &[u8] = b"\\033\\\\'; ";
const MARKER_ECHO_END_PREFIX: &[u8] = b"; __shush_exit=$?; printf '\\033_END_";
const MARKER_ECHO_END_SUFFIX: &[u8] = b"_%s\\033\\\\' \"$__shush_exit\"; unset __shush_exit";

#[derive(Clone, Debug)]
pub enum FeEvent {
    Chunk(Vec<u8>),
    Closed,
}

#[derive(Default)]
struct MarkerEchoFilter {
    pending: Vec<u8>,
    stripping: bool,
    replacement: Option<Vec<u8>>,
}

#[derive(Default)]
struct MarkerSequenceFilter {
    pending: Vec<u8>,
}

#[derive(Default)]
struct TerminalOutputFilter {
    echo: MarkerEchoFilter,
    markers: MarkerSequenceFilter,
}

impl MarkerEchoFilter {
    fn queue_replacement(&mut self, command: &[u8]) {
        self.replacement = Some(command.to_vec());
    }

    fn filter_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(chunk);
        let mut output = Vec::new();

        loop {
            if self.stripping {
                match find_subsequence(&self.pending, MARKER_ECHO_SUFFIX) {
                    Some(index) => {
                        let mut drain_end = index + MARKER_ECHO_SUFFIX.len();
                        while drain_end < self.pending.len()
                            && matches!(self.pending[drain_end], b'\r' | b'\n')
                        {
                            drain_end += 1;
                        }
                        if let Some(replacement) = self.replacement.take() {
                            output.extend_from_slice(&replacement);
                            output.extend_from_slice(
                                &self.pending[index + MARKER_ECHO_SUFFIX.len()..drain_end],
                            );
                        }
                        self.pending.drain(..drain_end);
                        self.stripping = false;
                    }
                    None => {
                        break;
                    }
                }
            } else {
                match find_subsequence(&self.pending, MARKER_ECHO_PREFIX) {
                    Some(index) => {
                        output.extend_from_slice(&self.pending[..index]);
                        self.pending.drain(..index + MARKER_ECHO_PREFIX.len());
                        self.stripping = true;
                    }
                    None => {
                        let keep = MARKER_ECHO_PREFIX.len().saturating_sub(1);
                        if self.pending.len() > keep {
                            let emit_len = self.pending.len() - keep;
                            output.extend_from_slice(&self.pending[..emit_len]);
                            self.pending.drain(..emit_len);
                        }
                        break;
                    }
                }
            }
        }

        output
    }

    fn finish(&mut self) -> Vec<u8> {
        if self.stripping {
            Vec::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

impl MarkerSequenceFilter {
    fn filter_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        const MARKER_START_PREFIX: &[u8] = b"\x1b_BEGIN_";
        const MARKER_END_PREFIX: &[u8] = b"\x1b_END_";
        const MARKER_TERMINATOR: &[u8] = b"\x1b\\";
        const PREFIX_KEEP: usize = MARKER_START_PREFIX.len() - 1;

        self.pending.extend_from_slice(chunk);
        let mut output = Vec::new();

        loop {
            let start = find_subsequence(&self.pending, MARKER_START_PREFIX)
                .into_iter()
                .chain(find_subsequence(&self.pending, MARKER_END_PREFIX))
                .min();

            match start {
                Some(index) => {
                    output.extend_from_slice(&self.pending[..index]);
                    match find_subsequence(&self.pending[index + 1..], MARKER_TERMINATOR) {
                        Some(relative_end) => {
                            let consumed = index + 1 + relative_end + MARKER_TERMINATOR.len();
                            self.pending.drain(..consumed);
                        }
                        None => {
                            self.pending.drain(..index);
                            break;
                        }
                    }
                }
                None => {
                    if self.pending.len() > PREFIX_KEEP {
                        let emit_len = self.pending.len() - PREFIX_KEEP;
                        output.extend_from_slice(&self.pending[..emit_len]);
                        self.pending.drain(..emit_len);
                    }
                    break;
                }
            }
        }

        output
    }

    fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

impl TerminalOutputFilter {
    fn queue_replacement(&mut self, command: &[u8]) {
        self.echo.queue_replacement(command);
    }

    fn filter_chunk(&mut self, chunk: &[u8]) -> Vec<u8> {
        let echoed = self.echo.filter_chunk(chunk);
        self.markers.filter_chunk(&echoed)
    }

    fn finish(&mut self) -> Vec<u8> {
        let echoed = self.echo.finish();
        let mut output = self.markers.filter_chunk(&echoed);
        output.extend(self.markers.finish());
        output
    }
}

fn extract_wrapped_command(bytes: &[u8]) -> Option<(Vec<u8>, usize, Vec<u8>)> {
    let prefix_end = MARKER_ECHO_PREFIX.len();
    let nonce_end = prefix_end + 64;
    if bytes.len() < nonce_end {
        return None;
    }
    if !bytes[prefix_end..nonce_end]
        .iter()
        .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }

    let middle_index = find_subsequence(&bytes[nonce_end..], MARKER_ECHO_MIDDLE)? + nonce_end;
    let command_start = middle_index + MARKER_ECHO_MIDDLE.len();
    let end_prefix_index =
        find_subsequence(&bytes[command_start..], MARKER_ECHO_END_PREFIX)? + command_start;
    let command = bytes[command_start..end_prefix_index].to_vec();

    let end_nonce_start = end_prefix_index + MARKER_ECHO_END_PREFIX.len();
    let end_nonce_end = end_nonce_start + 64;
    if bytes.len() < end_nonce_end {
        return None;
    }
    if !bytes[end_nonce_start..end_nonce_end]
        .iter()
        .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }

    if !bytes[end_nonce_start..end_nonce_end].eq(&bytes[prefix_end..nonce_end]) {
        return None;
    }

    let suffix_start = end_nonce_end;
    let suffix_end = suffix_start + MARKER_ECHO_END_SUFFIX.len();
    if bytes.len() < suffix_end || bytes[suffix_start..suffix_end] != *MARKER_ECHO_END_SUFFIX {
        return None;
    }

    let mut consumed = suffix_end;
    while consumed < bytes.len() && matches!(bytes[consumed], b'\r' | b'\n') {
        consumed += 1;
    }

    Some((command, consumed, bytes[suffix_end..consumed].to_vec()))
}

fn sanitize_marker_echoes(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut cursor = 0;

    while let Some(index) = find_subsequence(&bytes[cursor..], MARKER_ECHO_PREFIX) {
        let start = cursor + index;
        output.extend_from_slice(&bytes[cursor..start]);

        if let Some((command, consumed, trailing_newlines)) =
            extract_wrapped_command(&bytes[start..])
        {
            output.extend_from_slice(&command);
            output.extend_from_slice(&trailing_newlines);
            cursor = start + consumed;
        } else {
            output.extend_from_slice(&bytes[start..]);
            cursor = bytes.len();
            break;
        }
    }

    output.extend_from_slice(&bytes[cursor..]);
    output
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub struct FeMasterHandle {
    // One FeMasterHandle corresponds to one child process that attaches tmux
    // session (remote/local).
    session_name: String,
    session_host: String,

    // For broadcasting FE events (terminal output chunks, closure) to viewers.
    sender: broadcast::Sender<FeEvent>,

    // Child process that runs `tmux attach` (via ssh if remote). It is
    // spawned on the SLAVE side of the PTY.
    child: tokio::sync::Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,

    // A "guard" fd to keep the PTY alive until shutdown. `reader_task` has another fd for reading,
    master_guard: tokio::sync::Mutex<Option<std::fs::File>>,

    reader_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,

    // Ring buffer that accumulates raw PTY bytes as they arrive.
    replay_buffer: tokio::sync::Mutex<Vec<u8>>,
    output_filter: tokio::sync::Mutex<TerminalOutputFilter>,

    viewers: AtomicUsize,
    idle_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,

    // Whether the FE master process is alive. It becomes false when
    // `reader_task` encounters EOF or error.
    alive: AtomicBool,
}

impl FeMasterHandle {
    async fn spawn(session_name: &str, session_host: &str) -> Result<Arc<Self>, String> {
        // Spawn a tmux client(viewer) to attach to a session.
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize::default())
            .map_err(|e| format!("failed to open FE pty: {e}"))?;

        let cmd = if remote_tmux::is_local_host(session_host) {
            let mut cmd = CommandBuilder::new("tmux");
            cmd.args([
                "-L",
                remote_tmux::TMUX_SOCKET,
                "attach",
                "-f",
                "read-only,ignore-size",
                "-t",
                session_name,
            ]);
            cmd
        } else {
            info!(session_host = %session_host, session_name = %session_name, "spawning remote FE attach");
            let mut cmd = CommandBuilder::new("ssh");
            if let Ok(config) = std::env::var("SHUSH_SSH_CONFIG") {
                if !config.is_empty() {
                    cmd.args(["-F", &config]);
                }
            }
            // SSH options:
            // * `-o BatchMode=yes`: disables all interactive prompts for passwords or passphrases
            //   during an SSH connection; fail immediately if it can't authenticate
            //   non-interactively.
            // * `-tt`: when running `ssh host command`, SSH does not allocate a PTY on the "remote"
            //   side — it just connects stdin/stdout directly. With `-tt`, SSH allocates a PTY on
            //   the "remote" side and runs the command in it. Note that the `portable_pty` gives
            //   SSH a TTY on the "local" side. The two PTYs are connected through the SSH channel.
            cmd.args(["-o", "BatchMode=yes"]);
            //
            // tmux options:
            // * `-f ignore-size`: tmux client doesn't report its terminal size to the session, so
            //   attaching this client won't cause the session to resize
            cmd.args(["-tt", session_host, "tmux", "-L", remote_tmux::TMUX_SOCKET]);
            cmd.args(["attach", "-f", "read-only,ignore-size", "-t", session_name]);
            cmd
        };

        // PTY pair:
        // * Master: The fd that we read from and writes to. What we write to the master appears
        //   as input to the slave; what the slave program writes appears as output on the master.
        // * Slave: The device(e.g. /dev/pts/N) that the child process uses as its controlling
        //   terminal.

        // Spawn the child process (tmux/ssh) on the SLAVE side. The child's stdin/stdout/stderr
        // are connected to the slave PTY device.
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("failed to spawn FE tmux attach: {e}"))?;

        debug!(session_host = %session_host, session_name = %session_name, "FE attach process spawned");

        let master_fd: RawFd = pair
            .master
            .as_raw_fd()
            .ok_or("FE PTY master has no raw fd")?;

        // Another two copies of `master_fd`:
        // * reader_fd set to non-blocking for async reading (via tokio). It will be owned by the
        //   reader task.
        // * guard_fd kept as a "guard" to keep the PTY alive (since dropping all master file
        //   descriptors would destroy the PTY and kill the child).
        //
        // reader_fd is owned by BufReader in the reader task. It may return early due to EOF or
        // error, then close reader_fd. The master_guard in the handle keeps the PTY alive until
        // shutdown. Without it, PTY may or may not exist when shutdown.
        let reader_fd = unsafe { libc::dup(master_fd) };
        if reader_fd < 0 {
            return Err(format!(
                "dup(FE reader) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let guard_fd = unsafe { libc::dup(master_fd) };
        if guard_fd < 0 {
            unsafe { libc::close(reader_fd) };
            return Err(format!(
                "dup(FE guard) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let flags = unsafe { libc::fcntl(reader_fd, libc::F_GETFL, 0) };
        if flags < 0
            || unsafe { libc::fcntl(reader_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            unsafe {
                libc::close(reader_fd);
                libc::close(guard_fd);
            };
            return Err(format!(
                "fcntl(FE reader, O_NONBLOCK) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        let stdout = unsafe { tokio::fs::File::from_raw_fd(reader_fd) };
        let master_guard = unsafe { std::fs::File::from_raw_fd(guard_fd) };

        drop(pair.master);

        let (sender, _) = broadcast::channel::<FeEvent>(FE_BROADCAST_CAPACITY);

        let handle = Arc::new(Self {
            session_name: session_name.to_string(),
            session_host: session_host.to_string(),
            sender: sender.clone(),
            child: tokio::sync::Mutex::new(Some(child)),
            master_guard: tokio::sync::Mutex::new(Some(master_guard)),
            reader_task: tokio::sync::Mutex::new(None),
            replay_buffer: tokio::sync::Mutex::new(Vec::new()),
            output_filter: tokio::sync::Mutex::new(TerminalOutputFilter::default()),
            viewers: AtomicUsize::new(0),
            idle_task: tokio::sync::Mutex::new(None),
            alive: AtomicBool::new(true),
        });

        let reader_sender = sender.clone();
        let reader_handle = Arc::clone(&handle);
        let reader_session_name = session_name.to_string();
        let reader_session_host = session_host.to_string();
        let reader = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => {
                        let trailing = {
                            let mut filter = reader_handle.output_filter.lock().await;
                            filter.finish()
                        };
                        if !trailing.is_empty() {
                            let mut replay = reader_handle.replay_buffer.lock().await;
                            replay.extend_from_slice(&trailing);
                            if replay.len() > FE_REPLAY_BUFFER_MAX_BYTES {
                                let excess = replay.len() - FE_REPLAY_BUFFER_MAX_BYTES;
                                replay.drain(..excess);
                            }
                            drop(replay);
                            let _ = reader_sender.send(FeEvent::Chunk(trailing));
                        }
                        warn!(session_host = %reader_session_host, session_name = %reader_session_name, "FE reader reached EOF");
                        reader_handle.alive.store(false, Ordering::SeqCst);
                        let _ = reader_sender.send(FeEvent::Closed);
                        break;
                    }
                    Ok(n) => {
                        let filtered = {
                            let mut filter = reader_handle.output_filter.lock().await;
                            filter.filter_chunk(&buf[..n])
                        };
                        if filtered.is_empty() {
                            continue;
                        }

                        // Write PTY bytes to two places:
                        // 1. accumulates in replay_buffer.
                        let mut replay = reader_handle.replay_buffer.lock().await;
                        replay.extend_from_slice(&filtered);
                        if replay.len() > FE_REPLAY_BUFFER_MAX_BYTES {
                            let excess = replay.len() - FE_REPLAY_BUFFER_MAX_BYTES;
                            replay.drain(..excess);
                        }
                        drop(replay);

                        // 2. broadcast to all viewers.
                        let _ = reader_sender.send(FeEvent::Chunk(filtered));
                    }
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    Err(err) => {
                        warn!(session_host = %reader_session_host, session_name = %reader_session_name, error = %err, "FE reader failed");
                        reader_handle.alive.store(false, Ordering::SeqCst);
                        let _ = reader_sender.send(FeEvent::Closed);
                        break;
                    }
                }
            }
        });

        *handle.reader_task.lock().await = Some(reader);
        Ok(handle)
    }

    pub async fn snapshot(&self) -> Result<String, String> {
        let output = remote_tmux::run_tmux_output(
            &self.session_host,
            &["capture-pane", "-p", "-t", &self.session_name],
        )
        .await
        .map_err(|e| format!("capture-pane failed: {e}"))?;

        if !output.status.success() {
            return Err(format!("capture-pane exited with status {}", output.status));
        }

        Ok(
            base64::engine::general_purpose::STANDARD
                .encode(sanitize_marker_echoes(&output.stdout)),
        )
    }

    pub fn subscribe(&self) -> broadcast::Receiver<FeEvent> {
        self.sender.subscribe()
    }

    pub async fn replay_bytes(&self) -> Vec<u8> {
        self.replay_buffer.lock().await.clone()
    }

    async fn queue_command_echo_replacement(&self, command: &[u8]) {
        let mut filter = self.output_filter.lock().await;
        filter.queue_replacement(command);
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn viewer_count(&self) -> usize {
        self.viewers.load(Ordering::SeqCst)
    }

    async fn shutdown(&self) {
        debug!(session_host = %self.session_host, session_name = %self.session_name, "shutting down FE master handle");
        if let Some(task) = self.reader_task.lock().await.take() {
            task.abort();
        }
        self.master_guard.lock().await.take();
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    async fn on_connect(&self) {
        self.viewers.fetch_add(1, Ordering::SeqCst);
        if let Some(task) = self.idle_task.lock().await.take() {
            task.abort();
        }
    }
}

pub struct FeMasterRegistry {
    masters: RwLock<HashMap<Uuid, Arc<FeMasterHandle>>>,
    session_names: RwLock<HashMap<Uuid, String>>,
}

impl FeMasterRegistry {
    pub fn new() -> Self {
        Self {
            masters: RwLock::new(HashMap::new()),
            session_names: RwLock::new(HashMap::new()),
        }
    }

    /// Returns the `FeMasterHandle` for the session, spawning one if it does not exist or if the
    /// existing handle is no longer alive.
    /// Each session should have at most one `FeMasterHandle`. Multiple viewers of the same session
    /// share that handle so they can reuse the replay buffer and avoid spawning multiple tmux
    /// clients for the same session.
    pub async fn get_or_spawn(
        self: &Arc<Self>,
        session_id: Uuid,
        session_name: String,
        session_host: String,
    ) -> Result<Arc<FeMasterHandle>, String> {
        let existing = {
            let guard = self.masters.read().unwrap();
            guard.get(&session_id).cloned()
        };

        if let Some(existing) = existing {
            if !existing.is_alive() {
                let removed = self.masters.write().unwrap().remove(&session_id);
                self.session_names.write().unwrap().remove(&session_id);
                if let Some(stale) = removed {
                    stale.shutdown().await;
                }
            } else {
                existing.on_connect().await;
                return Ok(existing);
            }
        }

        let handle = FeMasterHandle::spawn(&session_name, &session_host).await?;
        handle.on_connect().await;

        self.session_names
            .write()
            .unwrap()
            .insert(session_id, session_name);
        self.masters
            .write()
            .unwrap()
            .insert(session_id, handle.clone());

        Ok(handle)
    }

    pub async fn on_disconnect(self: &Arc<Self>, session_id: Uuid) {
        let maybe = {
            let guard = self.masters.read().unwrap();
            guard.get(&session_id).cloned()
        };
        let Some(handle) = maybe else {
            return;
        };

        let prev = handle.viewers.fetch_sub(1, Ordering::SeqCst);
        if prev > 1 {
            return;
        }

        let this = Arc::clone(self);
        let task = tokio::spawn(async move {
            tokio::time::sleep(FE_IDLE_TIMEOUT).await;

            let should_remove = this
                .masters
                .read()
                .unwrap()
                .get(&session_id)
                .map(|h| h.viewers.load(Ordering::SeqCst) == 0)
                .unwrap_or(false);

            if !should_remove {
                return;
            }

            let removed = this.masters.write().unwrap().remove(&session_id);
            this.session_names.write().unwrap().remove(&session_id);
            if let Some(h) = removed {
                h.shutdown().await;
            }
        });

        *handle.idle_task.lock().await = Some(task);
    }

    pub async fn queue_command_echo_replacement(
        self: &Arc<Self>,
        session_id: Uuid,
        command: &[u8],
    ) {
        let maybe = {
            let guard = self.masters.read().unwrap();
            guard.get(&session_id).cloned()
        };

        if let Some(handle) = maybe {
            handle.queue_command_echo_replacement(command).await;
        }
    }
}

pub fn encode_terminal_chunk(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_terminal_chunk_base64() {
        let got = encode_terminal_chunk(b"abc\n");
        assert_eq!(got, "YWJjCg==");
    }

    #[test]
    fn sanitize_marker_echoes_preserves_user_command_from_snapshot() {
        let raw = b"before\nprintf '\\033_BEGIN_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef\\033\\\\'; echo hello; __shush_exit=$?; printf '\\033_END_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef_%s\\033\\\\' \"$__shush_exit\"; unset __shush_exit\nafter\n";

        let filtered = sanitize_marker_echoes(raw);

        assert_eq!(
            String::from_utf8_lossy(&filtered),
            "before\necho hello\nafter\n"
        );
    }

    #[test]
    fn marker_echo_filter_handles_chunk_boundaries() {
        let mut filter = MarkerEchoFilter::default();
        filter.queue_replacement(b"echo hello");
        let chunk1 = b"before\nprintf '\\033_BEGIN_deadbeefdeadbeef";
        let chunk2 = b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef\\033\\\\'; echo hello; __shush_exit=$?; printf '\\033_END_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef_%s\\033\\\\' \"$__shush_exit\"; unset __shush_exit\nafter\n";

        let mut filtered = filter.filter_chunk(chunk1);
        filtered.extend(filter.filter_chunk(chunk2));
        filtered.extend(filter.finish());

        assert_eq!(
            String::from_utf8_lossy(&filtered),
            "before\necho hello\nafter\n"
        );
    }

    #[test]
    fn terminal_output_filter_removes_apc_markers() {
        let mut filter = TerminalOutputFilter::default();
        let raw = b"before\x1b_BEGIN_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef\x1b\\hello\x1b_END_deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef_0\x1b\\after";

        let mut filtered = filter.filter_chunk(raw);
        filtered.extend(filter.finish());

        assert_eq!(String::from_utf8_lossy(&filtered), "beforehelloafter");
    }
}
