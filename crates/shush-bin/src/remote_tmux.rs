use std::io;
use std::process::{Command, ExitStatus, Output};

pub const TMUX_SOCKET: &str = "shush";

pub fn is_local_host(host: &str) -> bool {
    host.is_empty() || host == "localhost"
}

/// Runs a tmux command with provided args on the remote or local, returning the exit status.
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

pub async fn run_tmux_status_async(host: &str, tmux_args: &[&str]) -> io::Result<ExitStatus> {
    run_tmux_output(host, tmux_args)
        .await
        .map(|output| output.status)
}

pub async fn run_tmux_shell_status_async(host: &str, tmux_args: &[&str]) -> io::Result<ExitStatus> {
    let command = build_tmux_shell_command(tmux_args);

    if is_local_host(host) {
        tokio::process::Command::new("bash")
            .args(["-lc", &command])
            .status()
            .await
    } else {
        let mut cmd = tokio::process::Command::new("ssh");
        apply_ssh_config_async(&mut cmd);
        cmd.arg(host).arg(&command);
        cmd.status().await
    }
}

fn build_tmux_shell_command(tmux_args: &[&str]) -> String {
    let mut parts = vec![
        "tmux".to_string(),
        "-L".to_string(),
        shell_quote(TMUX_SOCKET),
    ];
    parts.extend(tmux_args.iter().map(|arg| shell_quote(arg)));
    parts.join(" ")
}

fn shell_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn apply_ssh_config(cmd: &mut Command) {
    cmd.args(["-o", "BatchMode=yes"]);
    if let Ok(config) = std::env::var("SHUSH_SSH_CONFIG") {
        if !config.is_empty() {
            cmd.args(["-F", &config]);
        }
    }
}

fn apply_ssh_config_async(cmd: &mut tokio::process::Command) {
    cmd.args(["-o", "BatchMode=yes"]);
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

    #[test]
    fn build_tmux_shell_command_quotes_arguments() {
        let command = build_tmux_shell_command(&[
            "send-keys",
            "-l",
            "-t",
            "session one",
            "printf '\\033_BEGIN_nonce\\033\\\\'; echo hello",
        ]);

        assert!(command.starts_with("tmux -L 'shush' 'send-keys' '-l' '-t' 'session one' "));
        assert!(command.contains("printf"));
        assert!(command.contains("echo hello"));
        assert!(command.contains("\\033_BEGIN_nonce"));
    }
}
