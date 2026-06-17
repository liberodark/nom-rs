use nix_output_monitor::builds::{FailType, Host};
use nix_output_monitor::nix_message::{
    Activity, ActivityResult, NixJsonMessage, OldStyleMessage, Verbosity,
};
use nix_output_monitor::parser_json::parse_line as parse_json;
use nix_output_monitor::parser_old::parse_old_style_line as parse_old;

#[test]
fn json_passes_through_non_nix_lines() {
    let m = parse_json(b"hello world");
    match m {
        NixJsonMessage::Plain(b) => assert_eq!(b, b"hello world"),
        other => panic!("expected Plain, got {other:?}"),
    }
}

#[test]
fn json_reports_parse_error_on_malformed_payload() {
    let m = parse_json(b"@nix not-actually-json");
    assert!(matches!(m, NixJsonMessage::ParseError { .. }));
}

#[test]
fn json_parses_a_message_action() {
    let line = br#"@nix {"action":"msg","level":4,"msg":"evaluating file 'foo.nix'"}"#;
    match parse_json(line) {
        NixJsonMessage::Message(m) => {
            assert_eq!(m.level, Verbosity::Talkative);
            assert!(m.message.contains("evaluating file"));
        }
        other => panic!("expected Message, got {other:?}"),
    }
}

#[test]
fn json_parses_a_stop_action() {
    let line = br#"@nix {"action":"stop","id":42}"#;
    match parse_json(line) {
        NixJsonMessage::Stop(s) => assert_eq!(s.id.0, 42),
        other => panic!("expected Stop, got {other:?}"),
    }
}

#[test]
fn json_parses_a_substitute_activity() {
    let line = br#"@nix {"action":"start","fields":["/nix/store/abc1abc1abc1abc1abc1abc1abc1abc1-hello","https://cache.nixos.org"],"id":7,"level":3,"text":"copying","type":108}"#;
    match parse_json(line) {
        NixJsonMessage::Start(s) => match s.activity {
            Activity::Substitute { path, host } => {
                assert_eq!(path.name, "hello");
                assert!(matches!(host, Host::Remote(_)));
            }
            other => panic!("expected Substitute, got {other:?}"),
        },
        other => panic!("expected Start, got {other:?}"),
    }
}

#[test]
fn json_parses_set_phase_result() {
    let line = br#"@nix {"action":"result","fields":["compile"],"id":1,"type":104}"#;
    match parse_json(line) {
        NixJsonMessage::Result(r) => match r.result {
            ActivityResult::SetPhase(p) => assert_eq!(p, "compile"),
            other => panic!("expected SetPhase, got {other:?}"),
        },
        other => panic!("expected Result, got {other:?}"),
    }
}

#[test]
fn old_style_rejects_unrelated_lines() {
    assert!(parse_old("some random text").is_none());
    assert!(parse_old("warning: foo bar").is_none());
}

#[test]
fn old_style_parses_remote_build() {
    let m = parse_old(
        "building '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv' on 'ssh://builder'...",
    )
    .unwrap();
    match m {
        OldStyleMessage::Build { drv, host } => {
            assert_eq!(drv.store_path.name, "build1");
            assert!(matches!(host, Host::Remote(ref s) if s == "ssh://builder"));
        }
        other => panic!("expected Build, got {other:?}"),
    }
}

#[test]
fn old_style_parses_checking_outputs() {
    let m = parse_old(
        "checking outputs of '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv'...",
    )
    .unwrap();
    assert!(matches!(m, OldStyleMessage::Checking(_)));
}

#[test]
fn old_style_parses_downloading() {
    let m = parse_old(
        "copying path '/nix/store/22d93x5fqmrwfxp18fyb4labbs1q2slw-build1' from 'https://cache.nixos.org'...",
    )
    .unwrap();
    match m {
        OldStyleMessage::Downloading { path, from } => {
            assert_eq!(path.name, "build1");
            assert!(matches!(from, Host::Remote(_)));
        }
        other => panic!("expected Downloading, got {other:?}"),
    }
}

#[test]
fn old_style_parses_hash_mismatch_failure() {
    let m = parse_old(
        "hash mismatch in fixed-output derivation '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv':",
    )
    .unwrap();
    assert!(matches!(
        m,
        OldStyleMessage::Failed {
            fail: FailType::HashMismatch,
            ..
        }
    ));
}

#[test]
fn old_style_unwraps_nix_2_18_error_prefix() {
    let m = parse_old(
        "error: build of '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv' failed: error: builder for '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv' failed with exit code 1",
    )
    .unwrap();
    assert!(matches!(
        m,
        OldStyleMessage::Failed {
            fail: FailType::ExitCode(1),
            ..
        }
    ));
}
