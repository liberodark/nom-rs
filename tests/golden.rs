use nix_output_monitor::build_reports::BuildReportMap;
use nix_output_monitor::parser_json::parse_line as parse_json;
use nix_output_monitor::parser_old::parse_old_style_line as parse_old;
use nix_output_monitor::state::{NomState, ProgressState};
use nix_output_monitor::update::{finalize, process_json_message, process_old_style};

fn fresh_state() -> NomState {
    NomState::new(0.0, None, BuildReportMap::new())
}

fn replay_json(bytes: &[u8]) -> NomState {
    let mut s = fresh_state();
    for (i, raw_line) in bytes.split(|b| *b == b'\n').enumerate() {
        if raw_line.is_empty() {
            continue;
        }
        let msg = parse_json(raw_line);
        let _ = process_json_message(&mut s, msg, i as f64);
    }
    finalize(&mut s, 1_000_000.0);
    s
}

fn replay_old(bytes: &[u8]) -> NomState {
    let mut s = fresh_state();
    for (i, raw_line) in bytes.split(|b| *b == b'\n').enumerate() {
        if raw_line.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(raw_line).unwrap_or("");
        let parsed = parse_old(text);
        let _ = process_old_style(&mut s, parsed, raw_line, i as f64);
    }
    finalize(&mut s, 1_000_000.0);
    s
}

#[test]
fn standard_json_completes_four_builds() {
    let bytes = include_bytes!("fixtures/standard/stderr.json");
    let s = replay_json(bytes);
    assert_eq!(s.progress_state, ProgressState::Finished);
    assert_eq!(
        s.full_summary.completed_builds.len(),
        4,
        "expected 4 completed builds"
    );
    assert_eq!(s.full_summary.running_builds.len(), 0);
    assert_eq!(s.full_summary.failed_builds.len(), 0);
    assert!(
        s.nix_errors.is_empty(),
        "no errors expected, got {:?}",
        s.nix_errors
    );
}

#[test]
fn fail_json_records_one_failure_and_one_error() {
    let bytes = include_bytes!("fixtures/fail/stderr.json");
    let s = replay_json(bytes);
    assert_eq!(s.progress_state, ProgressState::Finished);
    assert_eq!(
        s.full_summary.failed_builds.len(),
        1,
        "expected exactly 1 failed build"
    );
    assert!(!s.nix_errors.is_empty(), "expected at least 1 nix error");
    assert!(
        s.nix_errors
            .iter()
            .any(|e| e.contains("builder for") || e.contains("failed"))
    );
}

#[test]
fn standard_old_format_records_running_builds() {
    let bytes = include_bytes!("fixtures/standard/stderr");
    let s = replay_old(bytes);
    assert_eq!(s.progress_state, ProgressState::Finished);
    // The old format has no explicit "stop", so builds remain "running"
    // unless their output is found on disk — which isn't the case in tests.
    assert_eq!(
        s.full_summary.running_builds.len(),
        4,
        "expected 4 active builds (old format has no stop marker)"
    );
}

#[test]
fn fail_old_format_records_a_failed_build() {
    let bytes = include_bytes!("fixtures/fail/stderr");
    let s = replay_old(bytes);
    assert_eq!(s.progress_state, ProgressState::Finished);
    assert!(
        !s.full_summary.failed_builds.is_empty(),
        "expected at least 1 failed build"
    );
}
