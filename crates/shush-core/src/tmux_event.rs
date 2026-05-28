use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TmuxEvent {
    Begin(u64, u64, u32),
    End(u64, u64, u32),
    Error(u64, u64, u32, String),
    Output { pane: String, data: Vec<u8> },
    WindowAdd(String),
    SessionChanged(String, u64),
    Unknown(String),
}

impl fmt::Display for TmuxEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TmuxEvent::Begin(time, window, pane) => {
                write!(f, "%begin {time} {window} {pane}")
            }
            TmuxEvent::End(time, window, pane) => {
                write!(f, "%end {time} {window} {pane}")
            }
            TmuxEvent::Error(time, window, pane, msg) => {
                write!(f, "%error {time} {window} {pane} {msg}")
            }
            TmuxEvent::Output { pane, data } => {
                let escaped: String = data.iter().map(|b| format!("\\{b:03o}")).collect();
                write!(f, "%output %{pane} {escaped}")
            }
            TmuxEvent::WindowAdd(name) => write!(f, "%window-add {name}"),
            TmuxEvent::SessionChanged(name, id) => write!(f, "%session-changed {name} {id}"),
            TmuxEvent::Unknown(raw) => f.write_str(raw),
        }
    }
}

/// Parse a single line from tmux control mode output.
///
/// Format reference: `man tmux` under "Control Mode":
///   %begin <time> <window-index> <pane-index>
///   %end <time> <window-index> <pane-index>
///   %error <time> <window-index> <pane-index> <error-message>
///   %output %<pane> <octal-escaped-bytes>
///   %window-add <name>
///   %session-changed <name> <session-id>
fn parse_time_window_pane(parts: &[&str]) -> (u64, u64, u32) {
    let time = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let window = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let pane = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    (time, window, pane)
}

pub fn parse_tmux_event(line: &str) -> TmuxEvent {
    if let Some(rest) = line.strip_prefix("%begin ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let (time, window, pane) = parse_time_window_pane(&parts);
        return TmuxEvent::Begin(time, window, pane);
    }
    if let Some(rest) = line.strip_prefix("%end ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let (time, window, pane) = parse_time_window_pane(&parts);
        return TmuxEvent::End(time, window, pane);
    }
    if let Some(rest) = line.strip_prefix("%error ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let (time, window, pane) = parse_time_window_pane(&parts);
        let msg = parts.get(3..).map(|s| s.join(" ")).unwrap_or_default();
        return TmuxEvent::Error(time, window, pane, msg);
    }
    if let Some(rest) = line.strip_prefix("%output ") {
        if let Some(pane_start) = rest.strip_prefix('%') {
            let data_start = pane_start
                .find(' ')
                .map(|i| i + 1)
                .unwrap_or(pane_start.len());
            let pane = pane_start[..data_start.saturating_sub(1)].to_string();
            let data_str = &pane_start[data_start..];
            let data = decode_octal(data_str);
            return TmuxEvent::Output { pane, data };
        }
    }
    if let Some(name) = line.strip_prefix("%window-add ") {
        return TmuxEvent::WindowAdd(name.to_string());
    }
    if let Some(rest) = line.strip_prefix("%session-changed ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            let name = parts[0].to_string();
            let id = parts[1].parse().unwrap_or(0);
            return TmuxEvent::SessionChanged(name, id);
        }
    }
    TmuxEvent::Unknown(line.to_string())
}

fn decode_octal(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let octal: String = chars.by_ref().take(3).collect();
            if octal.len() == 3 && octal.chars().all(|c| c.is_ascii_digit()) {
                result.push(u8::from_str_radix(&octal, 8).unwrap_or(0));
            } else {
                result.push(b'\\');
                result.extend(octal.bytes());
            }
        } else {
            result.push(ch as u8);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_begin_event() {
        assert_eq!(
            parse_tmux_event("%begin 123 456 7"),
            TmuxEvent::Begin(123, 456, 7)
        );
    }

    #[test]
    fn parse_end_event() {
        assert_eq!(
            parse_tmux_event("%end 456 789 3"),
            TmuxEvent::End(456, 789, 3)
        );
    }

    #[test]
    fn parse_error_event() {
        assert_eq!(
            parse_tmux_event("%error 999 0 1 no such window: @1"),
            TmuxEvent::Error(999, 0, 1, "no such window: @1".to_string())
        );
    }

    #[test]
    fn parse_output_with_plain_text() {
        match parse_tmux_event("%output %1 hello world") {
            TmuxEvent::Output { pane, data } => {
                assert_eq!(pane, "1");
                assert_eq!(data, b"hello world");
            }
            other => panic!("expected Output, got {other:?}"),
        }
    }

    #[test]
    fn parse_output_with_octal_escapes() {
        match parse_tmux_event("%output %1 hello\\012world\\033[0m") {
            TmuxEvent::Output { pane, data } => {
                assert_eq!(pane, "1");
                assert_eq!(data, b"hello\nworld\x1b[0m");
            }
            other => panic!("expected Output, got {other:?}"),
        }
    }

    #[test]
    fn parse_window_add() {
        assert_eq!(
            parse_tmux_event("%window-add myproject"),
            TmuxEvent::WindowAdd("myproject".to_string())
        );
    }

    #[test]
    fn parse_session_changed() {
        assert_eq!(
            parse_tmux_event("%session-changed dev 42"),
            TmuxEvent::SessionChanged("dev".to_string(), 42)
        );
    }

    #[test]
    fn parse_unknown_line() {
        assert_eq!(
            parse_tmux_event("%unrecognized foo bar"),
            TmuxEvent::Unknown("%unrecognized foo bar".to_string())
        );
    }
}
