use crate::remote_tmux;
use base64::Engine;
use std::{
    collections::HashMap,
    os::unix::io::{FromRawFd, RawFd},
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::broadcast, task::JoinHandle};
use tracing::{debug, info, warn};
use uuid::Uuid;

const FE_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const FE_BROADCAST_CAPACITY: usize = 512;

pub struct FeMasterHandle {
    session_name: String,
    session_host: String,
    sender: broadcast::Sender<Vec<u8>>,
    child: tokio::sync::Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    master_guard: tokio::sync::Mutex<Option<std::fs::File>>,
    reader_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    viewers: AtomicUsize,
    idle_task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl FeMasterHandle {
    async fn spawn(session_name: &str, session_host: &str) -> Result<Arc<Self>, String> {
        use portable_pty::{CommandBuilder, PtySize, native_pty_system};

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
                "-r",
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
            cmd.args(["-o", "BatchMode=yes"]);
            cmd.args(["-tt", session_host, "tmux", "-L", remote_tmux::TMUX_SOCKET]);
            cmd.args(["attach", "-r", "-t", session_name]);
            cmd
        };

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("failed to spawn FE tmux attach: {e}"))?;

        debug!(session_host = %session_host, session_name = %session_name, "FE attach process spawned");

        let master_fd: RawFd = pair
            .master
            .as_raw_fd()
            .ok_or("FE PTY master has no raw fd")?;

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

        let (sender, _) = broadcast::channel::<Vec<u8>>(FE_BROADCAST_CAPACITY);

        let handle = Arc::new(Self {
            session_name: session_name.to_string(),
            session_host: session_host.to_string(),
            sender: sender.clone(),
            child: tokio::sync::Mutex::new(Some(child)),
            master_guard: tokio::sync::Mutex::new(Some(master_guard)),
            reader_task: tokio::sync::Mutex::new(None),
            viewers: AtomicUsize::new(0),
            idle_task: tokio::sync::Mutex::new(None),
        });

        let reader_sender = sender.clone();
        let reader_session_name = session_name.to_string();
        let reader_session_host = session_host.to_string();
        let reader = tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => {
                        warn!(session_host = %reader_session_host, session_name = %reader_session_name, "FE reader reached EOF");
                        break;
                    }
                    Ok(n) => {
                        let _ = reader_sender.send(buf[..n].to_vec());
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

        Ok(base64::engine::general_purpose::STANDARD.encode(output.stdout))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        self.sender.subscribe()
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
            existing.on_connect().await;
            return Ok(existing);
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
}
