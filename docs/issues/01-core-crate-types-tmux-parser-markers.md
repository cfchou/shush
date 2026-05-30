# Core crate: types, tmux parser, markers

Status: ready-for-agent

## What to build

Complete the `shush-core` crate with three modules:

**`session.rs`** (partially done — add missing exports and ensure it compiles):
- `SessionState` enum: `Idle`, `Pending`, `Executing`
- `CommandState` enum: `Pending`, `Executing`, `Completed(i32)`, `Rejected`, `Aborted`
- `CommandCard` struct with `id`, `command`, `state`, `exit_code`, `output`, timestamps
- `Session` struct with `id`, `name`, `host`, `state`, `yolo`, `current_command`, `created_at`

**`tmux_event.rs`** (new):
- `TmuxEvent` enum: `Begin(u64, u64, u32)`, `End(u64, u64, u32)`, `Error(u64, u64, u32, String)`, `Output { pane: String, data: Vec<u8> }`, `WindowAdd(String)`, `SessionChanged(String, u64)`, `Unknown(String)`
- `parse_tmux_event(line: &str) -> TmuxEvent` — parse `%begin`, `%end`, `%error`, `%output %<pane> <octal>`, `%window-add`, `%session-changed`
- Octal-escaped byte decoder (Control Mode escapes control chars to octal)
- Tests with sample tmux control-mode output lines

**`marker.rs`** (new):
- `MarkerInjector::new() -> Self` — initializes CSPRNG
- `MarkerInjector::inject(command: &str) -> (String, Nonce)` — wraps command with start/end APC markers
- `MarkerDetector::feed(bytes: &[u8]) -> Vec<MarkerEvent>` — scans byte stream, detects start/end markers with valid nonce
- `Nonce` — 32-byte random value, hex-encoded
- Tests for inject+detect round-trip, non-matching nonce detection

## Acceptance criteria

- [x] `cargo test -p shush-core` passes with unit tests for all three modules
- [x] TmuxEvent parser handles all known `%` control lines from tmux Control Mode
- [x] MarkerInjector produces wrapped commands; MarkerDetector correctly identifies start/end boundaries with matching nonce
- [x] No clippy warnings (`cargo clippy -p shush-core -- -D warnings`)

## Blocked by

None - can start immediately

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 2)
- Existing: `crates/shush-core/src/session.rs` (partially complete)
- Existing: `crates/shush-core/Cargo.toml` (dependencies declared)
