use nix_output_monitor::build_reports::BuildReportMap;
use nix_output_monitor::nix_message::NixJsonMessage;
use nix_output_monitor::parser_json::parse_line as parse_json;
use nix_output_monitor::state::{NomState, ProgressState};
use nix_output_monitor::update::{finalize, process_json_message};

fn fresh_state() -> NomState {
    NomState::new(0.0, None, BuildReportMap::new())
}

fn feed(state: &mut NomState, lines: &[&[u8]]) {
    for (i, line) in lines.iter().enumerate() {
        let msg: NixJsonMessage = parse_json(line);
        let _ = process_json_message(state, msg, i as f64);
    }
}

#[test]
fn fresh_state_starts_just_started_and_empty() {
    let s = fresh_state();
    assert_eq!(s.progress_state, ProgressState::JustStarted);
    assert!(s.full_summary.running_builds.is_empty());
    assert!(s.full_summary.completed_builds.is_empty());
    assert!(s.full_summary.failed_builds.is_empty());
}

#[test]
fn any_message_moves_progress_state_to_input_received() {
    let mut s = fresh_state();
    feed(&mut s, &[b"plain unrelated stdout line"]);
    assert_eq!(s.progress_state, ProgressState::InputReceived);
}

#[test]
fn a_build_start_then_stop_transitions_running_to_built() {
    let mut s = fresh_state();
    let drv = "/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-pkg.drv";
    let start = format!(
        r#"@nix {{"action":"start","fields":["{drv}",""],"id":1,"level":3,"text":"building","type":105}}"#
    );
    feed(&mut s, &[start.as_bytes()]);
    assert_eq!(s.full_summary.running_builds.len(), 1);
    assert_eq!(s.full_summary.completed_builds.len(), 0);

    feed(&mut s, &[br#"@nix {"action":"stop","id":1}"#]);
    assert_eq!(s.full_summary.running_builds.len(), 0);
    assert_eq!(s.full_summary.completed_builds.len(), 1);
}

#[test]
fn build_log_lines_get_prefixed_for_pass_through() {
    let mut s = fresh_state();
    let drv = "/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-mypkg.drv";
    let start = format!(
        r#"@nix {{"action":"start","fields":["{drv}",""],"id":1,"level":3,"text":"building","type":105}}"#
    );
    let log = br#"@nix {"action":"result","fields":["compiling mypkg-1.c"],"id":1,"type":101}"#;
    let msg = parse_json(start.as_bytes());
    let _ = process_json_message(&mut s, msg, 0.0);
    let msg = parse_json(log);
    let out = process_json_message(&mut s, msg, 1.0);
    let pass = String::from_utf8_lossy(&out.pass_through);
    assert!(
        pass.contains("compiling mypkg-1.c"),
        "pass_through={pass:?}"
    );
}

#[test]
fn unique_error_lines_are_collected_only_once() {
    let mut s = fresh_state();
    let err = br#"@nix {"action":"msg","level":0,"msg":"error: oh no"}"#;
    feed(&mut s, &[err, err, err]);
    assert_eq!(s.nix_errors.len(), 1);
}

#[test]
fn finalize_marks_progress_finished() {
    let mut s = fresh_state();
    feed(&mut s, &[b"unrelated"]);
    finalize(&mut s, 1.0);
    assert_eq!(s.progress_state, ProgressState::Finished);
}

#[test]
fn a_copy_path_substitute_records_a_download() {
    let mut s = fresh_state();
    let path = "/nix/store/abc1abc1abc1abc1abc1abc1abc1abc1-hello";
    let start = format!(
        r#"@nix {{"action":"start","fields":["{path}","https://cache.nixos.org","local"],"id":7,"level":3,"text":"copying","type":100}}"#
    );
    let stop = br#"@nix {"action":"stop","id":7}"#;
    feed(&mut s, &[start.as_bytes(), stop]);
    assert_eq!(s.full_summary.running_downloads.len(), 0);
    assert_eq!(s.full_summary.completed_downloads.len(), 1);
}
