use shush_core::marker::{MarkerInjector, Nonce};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

// ── Control-mode client ──────────────────────────────────────────────────────

/// Persistent `tmux -L <socket> -CC attach` connection via a PTY master.
///
/// `spawn_with_socket` first runs `tmux new-session -d -s <name>` to create
/// the backing session, then attaches in control mode via a PTY pair so that
/// tmux gets a proper controlling terminal (required on macOS ≥ 3.x).
///
/// I/O is stored as boxed async traits so unit tests can inject `io::duplex`
/// streams without touching the PTY path.
pub struct TmuxControlModeClient {
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    pub stdin: Option<Box<dyn AsyncWrite + Unpin + Send>>,
    pub stdout: Option<BufReader<Box<dyn AsyncRead + Unpin + Send>>>,
    pub active_pane: Option<String>,
}

impl TmuxControlModeClient {
    pub fn new() -> Self {
        Self {
            child: None,
            stdin: None,
            stdout: None,
            active_pane: None,
        }
    }

    /// Spawn using the default `shush` socket name.
    #[allow(dead_code)]
    pub async fn spawn(&mut self, session_name: &str) -> Result<(), String> {
        self.spawn_with_socket(session_name, "shush").await
    }

    /// Create a tmux session and attach to it in control mode.
    ///
    /// Steps:
    /// 1. `tmux -L <socket> new-session -d -s <name>` — detached session
    /// 2. Open a PTY pair
    /// 3. `tmux -L <socket> -CC attach -t <name>` on the slave end
    /// 4. Store master fds as async reader / writer
    pub async fn spawn_with_socket(
        &mut self,
        session_name: &str,
        socket: &str,
    ) -> Result<(), String> {
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};
        use std::os::unix::io::{FromRawFd, RawFd};

        // Step 1 — create the detached session.
        let status = tokio::process::Command::new("tmux")
            .args(["-L", socket, "new-session", "-d", "-s", session_name])
            .stdin(std::process::Stdio::null())
            .status()
            .await
            .map_err(|e| format!("failed to create tmux session: {e}"))?;
        if !status.success() {
            return Err(format!("tmux new-session failed with status {status}"));
        }

        // Steps 2 & 3 — open PTY and attach in control mode.
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize::default())
            .map_err(|e| format!("failed to open pty: {e}"))?;

        let mut cmd = CommandBuilder::new("tmux");
        cmd.args(["-L", socket, "-CC", "attach", "-t", session_name]);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("failed to spawn tmux -CC attach: {e}"))?;

        // Step 4 — dup the master fd into independent async reader/writer.
        let master_fd: RawFd = pair.master.as_raw_fd().ok_or("PTY master has no raw fd")?;

        let reader_fd = unsafe { libc::dup(master_fd) };
        if reader_fd < 0 {
            return Err(format!(
                "dup(reader) failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        let writer_fd = unsafe { libc::dup(master_fd) };
        if writer_fd < 0 {
            unsafe { libc::close(reader_fd) };
            return Err(format!(
                "dup(writer) failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        for fd in [reader_fd, writer_fd] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                unsafe {
                    libc::close(reader_fd);
                    libc::close(writer_fd);
                }
                return Err(format!(
                    "fcntl(O_NONBLOCK) failed: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }

        let async_reader = unsafe { tokio::fs::File::from_raw_fd(reader_fd) };
        let async_writer = unsafe { tokio::fs::File::from_raw_fd(writer_fd) };

        // Drop portable-pty master after dup — closes the original fd.
        drop(pair.master);

        self.child = Some(child);
        self.stdin = Some(Box::new(async_writer));
        self.stdout = Some(BufReader::new(Box::new(async_reader)));

        Ok(())
    }

    /// Write a control-mode command to stdin, e.g. `send-keys -l <text>`.
    pub async fn send_keys(&mut self, text: &str) -> Result<(), String> {
        let cmd = format!("send-keys -l {text}\n");
        if let Some(stdin) = self.stdin.as_mut() {
            stdin
                .write_all(cmd.as_bytes())
                .await
                .map_err(|e| format!("write error: {e}"))?;
            stdin
                .flush()
                .await
                .map_err(|e| format!("flush error: {e}"))?;
            Ok(())
        } else {
            Err("no stdin available".into())
        }
    }

    #[allow(dead_code)]
    pub async fn inject_command(&mut self, command: &str) -> Result<Nonce, String> {
        let injector = MarkerInjector::new();
        let (wrapped, nonce) = injector.inject(command);
        self.send_keys(&wrapped).await?;
        self.send_keys("Enter").await?;
        Ok(nonce)
    }

    /// Read one line from the tmux event stream.  Returns `None` on EOF.
    /// Trailing whitespace (including `\r\n`) is stripped.
    pub async fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        if let Some(stdout) = self.stdout.as_mut() {
            stdout.read_line(&mut line).await.ok()?;
            if line.is_empty() {
                None
            } else {
                Some(line.trim_end().to_string())
            }
        } else {
            None
        }
    }

    /// Kill the control-mode child process and drop I/O handles.
    pub async fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.stdin = None;
        self.stdout = None;
    }
}

impl Drop for TmuxControlModeClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
        }
    }
}

// ── Unit tests (mock I/O, no real tmux) ──────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{self, AsyncReadExt};

    #[test]
    fn new_creates_empty_client() {
        let client = TmuxControlModeClient::new();
        assert!(client.child.is_none());
        assert!(client.stdin.is_none());
        assert!(client.stdout.is_none());
        assert!(client.active_pane.is_none());
    }

    fn mock_client(
        stdin: Box<dyn AsyncWrite + Unpin + Send>,
        stdout: Box<dyn AsyncRead + Unpin + Send>,
    ) -> TmuxControlModeClient {
        TmuxControlModeClient {
            child: None,
            stdin: Some(stdin),
            stdout: Some(BufReader::new(stdout)),
            active_pane: None,
        }
    }

    #[tokio::test]
    async fn send_keys_writes_correct_format() {
        let (client_stdin, mut mock_stdin) = io::duplex(1024);
        let (_mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        client.send_keys("hello").await.unwrap();

        let mut buf = vec![0u8; 64];
        let n = mock_stdin.read(&mut buf).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&buf[..n]), "send-keys -l hello\n");
    }

    #[tokio::test]
    async fn send_keys_with_special_chars() {
        let (client_stdin, mut mock_stdin) = io::duplex(1024);
        let (_mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        client.send_keys("ls -la | grep foo").await.unwrap();

        let mut buf = vec![0u8; 128];
        let n = mock_stdin.read(&mut buf).await.unwrap();
        assert_eq!(
            String::from_utf8_lossy(&buf[..n]),
            "send-keys -l ls -la | grep foo\n"
        );
    }

    #[tokio::test]
    async fn send_keys_flushes_after_write() {
        let (client_stdin, mut mock_stdin) = io::duplex(1024);
        let (_mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        client.send_keys("x").await.unwrap();

        let mut buf = [0u8; 32];
        let n = mock_stdin.read(&mut buf).await.unwrap();
        assert!(n > 0, "flush should make data available immediately");
    }

    #[tokio::test]
    async fn read_line_returns_content() {
        let (client_stdin, _) = io::duplex(1024);
        let (mut mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        mock_stdout.write_all(b"%begin 123 456 7\n").await.unwrap();

        assert_eq!(
            client.read_line().await.as_deref(),
            Some("%begin 123 456 7")
        );
    }

    #[tokio::test]
    async fn read_line_trailing_whitespace_stripped() {
        let (client_stdin, _) = io::duplex(1024);
        let (mut mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        mock_stdout.write_all(b"hello\r\n").await.unwrap();

        assert_eq!(client.read_line().await.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn read_line_multiple_lines() {
        let (client_stdin, _) = io::duplex(1024);
        let (mut mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        mock_stdout
            .write_all(b"first\nsecond\nthird\n")
            .await
            .unwrap();

        assert_eq!(client.read_line().await.as_deref(), Some("first"));
        assert_eq!(client.read_line().await.as_deref(), Some("second"));
        assert_eq!(client.read_line().await.as_deref(), Some("third"));
    }

    #[tokio::test]
    async fn read_line_returns_none_on_eof() {
        let (client_stdin, _) = io::duplex(1024);
        let (mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        drop(mock_stdout);

        assert!(client.read_line().await.is_none());
    }

    #[tokio::test]
    async fn read_line_returns_none_when_no_stdout() {
        let mut client = TmuxControlModeClient::new();
        assert!(client.read_line().await.is_none());
    }

    #[tokio::test]
    async fn send_keys_returns_error_when_no_stdin() {
        let mut client = TmuxControlModeClient::new();
        let result = client.send_keys("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no stdin"));
    }

    #[tokio::test]
    async fn kill_cleans_up_io() {
        let (client_stdin, _) = io::duplex(1024);
        let (_, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        client.kill().await;
        assert!(client.stdin.is_none());
        assert!(client.stdout.is_none());
    }

    #[tokio::test]
    async fn roundtrip_send_and_receive() {
        let (client_stdin, mut mock_stdin) = io::duplex(1024);
        let (mut mock_stdout, client_stdout) = io::duplex(1024);
        let mut client = mock_client(Box::new(client_stdin), Box::new(client_stdout));

        client.send_keys("echo hello").await.unwrap();

        let mut buf = vec![0u8; 128];
        let n = mock_stdin.read(&mut buf).await.unwrap();
        assert_eq!(
            String::from_utf8_lossy(&buf[..n]),
            "send-keys -l echo hello\n"
        );

        mock_stdout.write_all(b"%output %1 hello\n").await.unwrap();
        assert_eq!(
            client.read_line().await.as_deref(),
            Some("%output %1 hello")
        );
    }
}

// ── Integration tests (real tmux, #[ignore]) ──────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::process::{Command, Stdio};
    use uuid::Uuid;

    fn unique_name(prefix: &str) -> String {
        format!("{}-{}", prefix, Uuid::new_v4())
    }

    fn unique_socket(prefix: &str) -> String {
        format!("{}-{}", prefix, Uuid::new_v4())
    }

    /// Kills the tmux server on `socket` when dropped, cleaning up after each test.
    struct TmuxTestGuard {
        socket: String,
    }

    impl TmuxTestGuard {
        fn new(socket: &str) -> Self {
            Self {
                socket: socket.to_string(),
            }
        }
    }

    impl Drop for TmuxTestGuard {
        fn drop(&mut self) {
            let _ = Command::new("tmux")
                .args(["-L", &self.socket, "kill-server"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    /// Run `tmux -L <socket> list-sessions` and return stdout lines.
    fn tmux_list(socket: &str) -> Vec<String> {
        let out = Command::new("tmux")
            .args(["-L", socket, "list-sessions"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect()
    }

    /// Run `tmux -L <socket> capture-pane -t <target> -p`.
    fn tmux_capture(socket: &str, target: &str) -> String {
        let out = Command::new("tmux")
            .args([
                "-L",
                socket,
                "capture-pane",
                "-t",
                target,
                "-p",
                "-S",
                "-100",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    #[tokio::test]
    #[ignore]
    async fn spawn_creates_real_tmux_session() {
        let socket = unique_socket("shush-it");
        let name = unique_name("it-create");
        let _guard = TmuxTestGuard::new(&socket);

        let mut cc = TmuxControlModeClient::new();
        cc.spawn_with_socket(&name, &socket).await.unwrap();

        let sessions = tmux_list(&socket);
        assert!(
            sessions.iter().any(|s| s.contains(&name)),
            "tmux session should be listed: {sessions:?}"
        );

        cc.kill().await;
    }

    #[tokio::test]
    #[ignore]
    async fn spawn_reads_greeting_events() {
        let socket = unique_socket("shush-it");
        let name = unique_name("it-greet");
        let _guard = TmuxTestGuard::new(&socket);

        let mut cc = TmuxControlModeClient::new();
        cc.spawn_with_socket(&name, &socket).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let line = cc.read_line().await;
        assert!(line.is_some(), "should read at least one event line");
        let line = line.unwrap();
        assert!(
            line.starts_with('%') || line.starts_with("\x1bP"),
            "tmux control-mode events start with % or DCS: got {line:?}"
        );

        cc.kill().await;
    }

    #[tokio::test]
    #[ignore]
    async fn send_keys_delivers_to_pane() {
        let socket = unique_socket("shush-it");
        let name = unique_name("it-keys");
        let _guard = TmuxTestGuard::new(&socket);

        let mut cc = TmuxControlModeClient::new();
        cc.spawn_with_socket(&name, &socket).await.unwrap();

        // Drain greeting until %session-changed or timeout.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
        loop {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(100), cc.read_line()).await
            {
                Ok(Some(line)) if line.starts_with("%session-changed") => break,
                Ok(Some(_)) => continue,
                _ => break,
            }
        }

        cc.send_keys("echo shush_marker_99").await.unwrap();
        // Send an Enter key via control-mode command.
        if let Some(stdin) = cc.stdin.as_mut() {
            stdin.write_all(b"send-keys Enter\n").await.unwrap();
            stdin.flush().await.unwrap();
        }

        // Drain events looking for the marker.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut found = false;
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(200), cc.read_line()).await
            {
                Ok(Some(line)) if line.contains("shush_marker_99") => {
                    found = true;
                    break;
                }
                Ok(Some(_)) => continue,
                _ => break,
            }
        }

        // Fallback: capture-pane via plain tmux command.
        if !found {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            found = tmux_capture(&socket, &name).contains("shush_marker_99");
        }

        assert!(found, "should see the echoed marker in tmux output");
        cc.kill().await;
    }

    #[tokio::test]
    #[ignore]
    async fn kill_cleans_up() {
        let socket = unique_socket("shush-it");
        let name = unique_name("it-kill");
        let _guard = TmuxTestGuard::new(&socket);

        let mut cc = TmuxControlModeClient::new();
        cc.spawn_with_socket(&name, &socket).await.unwrap();
        cc.kill().await;

        assert!(cc.stdin.is_none());
        assert!(cc.stdout.is_none());
        assert!(cc.child.is_none());
    }
}
