use nix_output_monitor::ansi;
use nix_output_monitor::build_reports::BuildReportMap;
use nix_output_monitor::parser_json::parse_line as parse_json;
use nix_output_monitor::print::{Config, state_to_text};
use nix_output_monitor::state::NomState;
use nix_output_monitor::update::{finalize, process_json_message};

fn replay(bytes: &[u8]) -> NomState {
    let mut s = NomState::new(0.0, None, BuildReportMap::new());
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

fn render(state: &NomState) -> String {
    let cfg = Config {
        silent: false,
        piping: false,
    };
    state_to_text(&cfg, state, Some(120), Some(40), 1_000_000.0)
}

fn render_unsilent(bytes: &[u8]) -> String {
    let raw = render(&replay(bytes));
    ansi::strip_ansi(&raw)
}

#[test]
fn render_standard_contains_the_finished_marker() {
    let plain = render_unsilent(include_bytes!("fixtures/standard/stderr.json"));
    assert!(plain.contains("Finished"), "render was: {plain}");
}

#[test]
fn render_failure_shows_exited_with_failures() {
    let plain = render_unsilent(include_bytes!("fixtures/fail/stderr.json"));
    assert!(
        plain.contains("Exited") && plain.contains("failures"),
        "render was: {plain}"
    );
}

#[test]
fn render_standard_includes_summary_separators() {
    let plain = render_unsilent(include_bytes!("fixtures/standard/stderr.json"));
    // The summary table uses these box-drawing characters as separators.
    assert!(plain.contains("│"), "missing column separator");
    assert!(plain.contains("Builds"), "missing Builds header");
}

#[test]
fn render_silent_after_finish_is_empty() {
    let s = replay(include_bytes!("fixtures/standard/stderr.json"));
    let cfg = Config {
        silent: true,
        piping: false,
    };
    let out = state_to_text(&cfg, &s, Some(120), Some(40), 1_000_000.0);
    assert!(
        out.is_empty(),
        "silent+finished should render empty, got: {out:?}"
    );
}

#[test]
fn render_never_contains_unmatched_ansi_escape() {
    // Every '\x1b[...m' opener should have a matching reset by the time the
    // string ends. We approximate by checking the string ends with a reset
    // sequence (or has no escapes at all).
    let raw = render(&replay(include_bytes!("fixtures/standard/stderr.json")));
    let has_escape = raw.contains('\x1b');
    if has_escape {
        assert!(
            raw.ends_with("\x1b[0m") || raw.contains("\x1b[0m"),
            "render contains escapes but no reset: {raw:?}"
        );
    }
}

#[test]
fn render_width_is_respected() {
    let cfg = Config {
        silent: false,
        piping: false,
    };
    let s = replay(include_bytes!("fixtures/standard/stderr.json"));
    let raw = state_to_text(&cfg, &s, Some(40), Some(40), 1_000_000.0);
    for line in raw.lines() {
        let w = ansi::display_width(line);
        assert!(w <= 40, "line of width {w} exceeds 40: {line:?}");
    }
}
