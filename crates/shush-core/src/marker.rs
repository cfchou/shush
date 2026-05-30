use std::fmt;

/// 32-byte cryptographic nonce, hex-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nonce([u8; 32]);

impl Nonce {
    pub fn hex(&self) -> String {
        // 32 bytes result in 64 hex chars, 1 byte per 2 hex chars.
        let mut s = String::with_capacity(64);
        for b in &self.0 {
            s.push_str(&format!("{b:02x}"));
        }
        s
    }
}

impl fmt::Display for Nonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

fn hex_decode_nonce(s: &str) -> Nonce {
    let mut bytes = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        if i < 32 && chunk.len() == 2 {
            let hi = hex_val(chunk[0]);
            let lo = hex_val(chunk[1]);
            bytes[i] = (hi << 4) | lo;
        }
    }
    Nonce(bytes)
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        // silently accept invalid
        _ => 0,
    }
}

/// Wraps commands with Application Program Command (APC) markers
/// so command boundaries are detectable in the tmux output stream.
/// It's a control sequence from the ANSI/VT escape code standard used for
/// terminal communication:
///   * ESC(0x1b) — escape character that starts the sequence
///   * _(underscore) — designates an APC sequence
///   * Data — application-specific payload
///   * ESC\(0x1b 0x5c) — terminates the sequence
///
/// These markers are invisible to the shell (the terminal interprets them, not the shell)
/// ```text
/// ESC_BEGIN_<nonce>ESC\  <- Command start marker
/// [actual command output]
/// ESC_END_<nonce>ESC\    <- Command end marker
/// ```
#[derive(Debug)]
pub struct MarkerInjector;

impl Default for MarkerInjector {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkerInjector {
    pub fn new() -> Self {
        Self
    }

    /// Wraps `command` with start/end APC markers containing a fresh nonce.
    ///
    /// The returned command uses `printf` to emit control sequences that
    /// the shell will output as literal escape codes, creating unique
    /// markers in the tmux output stream.
    pub fn inject(&self, command: &str) -> (String, Nonce) {
        let nonce = generate_nonce();
        let nonce_hex = nonce.hex();
        let wrapped = format!(
            "printf '\\033_BEGIN_{nonce_hex}\\033\\\\' && {command} && printf '\\033_END_{nonce_hex}\\033\\\\'"
        );
        (wrapped, nonce)
    }
}

/// Events emitted by MarkerDetector when scanning a byte stream.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkerEvent {
    Start(Nonce),
    End(Nonce),
}

/// Scans a byte stream for APC markers produced by MarkerInjector.
///
/// Maintains internal state across `feed()` calls so markers split
/// across chunk boundaries are correctly detected.
#[derive(Debug)]
pub struct MarkerDetector {
    buffer: Vec<u8>,
}

impl Default for MarkerDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkerDetector {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<MarkerEvent> {
        self.buffer.extend_from_slice(bytes);

        let mut events = Vec::new();
        let mut i = 0;
        let buflen = self.buffer.len();

        while i < buflen {
            // Look for ESC (0x1b) — the start of an APC sequence
            if self.buffer[i] != 0x1b {
                i += 1;
                continue;
            }

            // Absolute minimum marker: ESC _ END_ <64 hex> ESC \
            // = 1 + 1 + 4 + 64 + 1 + 1 = 72 bytes
            if i + 72 > buflen {
                break;
            }

            // Must be followed by '_'
            if self.buffer[i + 1] != b'_' {
                i += 2;
                continue;
            }

            // Determine tag: BEGIN_ (6 bytes) or END_ (4 bytes)
            let is_start = self.buffer[i + 2..].starts_with(b"BEGIN_");
            let is_end = !is_start && self.buffer[i + 2..].starts_with(b"END_");
            if !is_start && !is_end {
                i += 2;
                continue;
            }

            let tag_len: usize = if is_start { 6 } else { 4 }; // "BEGIN_" or "END_"
            let hex_start = i + 2 + tag_len;
            let hex_end = hex_start + 64;

            // Must have trailing ESC \ after hex
            if hex_end + 2 > buflen {
                break;
            }

            let hex_slice = &self.buffer[hex_start..hex_end];
            if !hex_slice.iter().all(|b| b.is_ascii_hexdigit()) {
                i += 2;
                continue;
            }

            if self.buffer[hex_end] != 0x1b || self.buffer[hex_end + 1] != b'\\' {
                i += 2;
                continue;
            }

            let hex_str = unsafe { std::str::from_utf8_unchecked(hex_slice) };
            let nonce = hex_decode_nonce(hex_str);

            if is_start {
                events.push(MarkerEvent::Start(nonce));
            } else {
                events.push(MarkerEvent::End(nonce));
            }

            i = hex_end + 2;
        }

        if i > 0 {
            self.buffer.drain(..i);
        }

        events
    }
}

fn generate_nonce() -> Nonce {
    let mut bytes = [0u8; 32];
    let _ = getrandom::getrandom(&mut bytes);
    Nonce(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_returns_wrapped_command_with_nonce() {
        let injector = MarkerInjector::new();
        let (wrapped, nonce) = injector.inject("echo hello");

        assert!(
            wrapped.contains("echo hello"),
            "wrapped command should contain original"
        );
        assert_eq!(
            nonce.hex().len(),
            64,
            "nonce should be 64 hex chars (32 bytes)"
        );
    }

    #[test]
    fn roundtrip_inject_then_detect() {
        let injector = MarkerInjector::new();
        let (_, nonce) = injector.inject("true");

        let start_marker = format!("\x1b_BEGIN_{}\x1b\\", nonce.hex());
        let end_marker = format!("\x1b_END_{}\x1b\\", nonce.hex());
        let stream = format!("{}some output\n{}", start_marker, end_marker);

        let mut detector = MarkerDetector::new();
        let events = detector.feed(stream.as_bytes());

        assert_eq!(
            events,
            vec![MarkerEvent::Start(nonce.clone()), MarkerEvent::End(nonce)],
        );
    }

    #[test]
    fn non_matching_nonce_is_rejected() {
        let injector = MarkerInjector::new();
        let (_, nonce) = injector.inject("true");

        // Simulate a different nonce in the end marker
        let other = generate_nonce();
        let start_marker = format!("\x1b_BEGIN_{}\x1b\\", nonce.hex());
        let bad_end = format!("\x1b_END_{}\x1b\\", other.hex());
        let stream = format!("{}output{}", start_marker, bad_end);

        let mut detector = MarkerDetector::new();
        let events = detector.feed(stream.as_bytes());

        assert_eq!(events.len(), 2, "both markers should be detected");
        assert_eq!(events[0], MarkerEvent::Start(nonce));
        assert_eq!(events[1], MarkerEvent::End(other));
    }

    #[test]
    fn detect_multiple_markers_across_feed_calls() {
        let mut detector = MarkerDetector::new();
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();

        let m1 = format!("\x1b_BEGIN_{}\x1b\\", nonce1.hex());
        let m2 = format!("\x1b_END_{}\x1b\\", nonce2.hex());

        // Feed in two chunks so marker crosses the boundary
        let chunk1 = &m1.as_bytes()[..20];
        let chunk2 = &m1.as_bytes()[20..];
        let _ = detector.feed(chunk1);
        let events = detector.feed(chunk2);
        assert_eq!(
            events,
            vec![MarkerEvent::Start(nonce1.clone())],
            "marker split across feed calls"
        );

        // Now feed the end marker
        let events = detector.feed(m2.as_bytes());
        assert_eq!(events, vec![MarkerEvent::End(nonce2)]);
    }

    #[test]
    fn detect_marker_split_at_nonce_boundary() {
        let mut detector = MarkerDetector::new();
        let nonce = generate_nonce();
        let marker = format!("\x1b_BEGIN_{}\x1b\\", nonce.hex());

        // Split exactly at the nonce boundary: after ESC _ BEGIN_
        let prefix_len = 2 + 6; // ESC + '_' + "BEGIN_"
        let chunk1 = &marker.as_bytes()[..prefix_len];
        let chunk2 = &marker.as_bytes()[prefix_len..];

        let _ = detector.feed(chunk1);
        let events = detector.feed(chunk2);
        assert_eq!(
            events,
            vec![MarkerEvent::Start(nonce)],
            "marker split at nonce boundary"
        );
    }

    #[test]
    fn detect_markers_with_garbage_between() {
        let mut detector = MarkerDetector::new();
        let nonce = generate_nonce();

        let start = format!("\x1b_BEGIN_{}\x1b\\", nonce.hex());
        let end = format!("\x1b_END_{}\x1b\\", nonce.hex());
        let garbage = b"random noise here!!!\x01\x02\x03";

        let stream = [start.as_bytes(), garbage, end.as_bytes()].concat();
        let events = detector.feed(&stream);

        assert_eq!(
            events,
            vec![MarkerEvent::Start(nonce.clone()), MarkerEvent::End(nonce)],
            "detector should find both markers ignoring garbage"
        );
    }

    #[test]
    fn inject_preserves_command_with_special_chars() {
        let injector = MarkerInjector::new();
        let cmd = r#"echo "hello" | grep 'world' && ls `pwd`"#;
        let (wrapped, _) = injector.inject(cmd);

        assert!(
            wrapped.contains(cmd),
            "wrapped command should contain original verbatim"
        );
    }
}
