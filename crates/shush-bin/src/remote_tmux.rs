use std::io;
use std::process::{Command, ExitStatus, Output};

pub const TMUX_SOCKET: &str = "shush";

pub fn is_local_host(host: &str) -> bool {
    host.is_empty() || host == "localhost"
}

pub fn run_tmux_status(host: &str, tmux_args: &[&str]) -> io::Result<ExitStatus> {
    if is_local_host(host) {
        Command::new("tmux")
            .args(["-L", TMUX_SOCKET])
            .args(tmux_args)
            .status()
    } else {
        let mut cmd = Command::new("ssh");
        apply_ssh_config(&mut cmd);
        cmd.arg(host)
            .arg("tmux")
            .args(["-L", TMUX_SOCKET])
            .args(tmux_args);
        cmd.status()
    }
}

pub async fn run_tmux_output(host: &str, tmux_args: &[&str]) -> io::Result<Output> {
    if is_local_host(host) {
        tokio::process::Command::new("tmux")
            .args(["-L", TMUX_SOCKET])
            .args(tmux_args)
            .output()
            .await
    } else {
        let mut cmd = tokio::process::Command::new("ssh");
        apply_ssh_config_async(&mut cmd);
        cmd.arg(host)
            .arg("tmux")
            .args(["-L", TMUX_SOCKET])
            .args(tmux_args);
        cmd.output().await
    }
}

fn apply_ssh_config(cmd: &mut Command) {
    if let Ok(config) = std::env::var("SHUSH_SSH_CONFIG") {
        if !config.is_empty() {
            cmd.args(["-F", &config]);
        }
    }
}

fn apply_ssh_config_async(cmd: &mut tokio::process::Command) {
    if let Ok(config) = std::env::var("SHUSH_SSH_CONFIG") {
        if !config.is_empty() {
            cmd.args(["-F", &config]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts_are_detected() {
        assert!(is_local_host(""));
        assert!(is_local_host("localhost"));
        assert!(!is_local_host("dev@127.0.0.1"));
    }
}
