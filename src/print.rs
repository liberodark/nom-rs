use crate::ansi;
use crate::builds::{FailType, Host};
use crate::state::{BuildStatus, DerivationId, NomState, ProgressState, StorePathState};
use crate::table::{self, Entry};
use crate::tree::{self, TreeLocation, TreeNode};

/// Runtime configuration of the renderer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Config {
    /// If true, never render anything when finished.
    pub silent: bool,
    /// If true, we are reading from a pipe (vs running a subprocess).
    pub piping: bool,
}

// Symbols. See README.
const VERTICAL: &str = "┃";
const LOWER_LEFT: &str = "┗";
const UPPER_LEFT: &str = "┏";
const LEFT_T: &str = "┣";
const HORIZONTAL: &str = "━";
const DOWN: &str = "↓";
const UP: &str = "↑";
const CLOCK: &str = "⏱";
const RUNNING: &str = "⏵";
const DONE: &str = "✔";
const TODO: &str = "⏸";
const WARNING: &str = "⚠";
const AVERAGE: &str = "∅";
const BIG_SUM: &str = "∑";

const TARGET_RATIO: usize = 3;
const DEFAULT_TREE_MAX: usize = 20;

/// Render the state to a multi-line ANSI string.
pub fn state_to_text(
    config: &Config,
    state: &NomState,
    width: Option<usize>,
    height: Option<usize>,
    now: f64,
) -> String {
    let max_height = height.map(|h| h / TARGET_RATIO).unwrap_or(DEFAULT_TREE_MAX);

    if state.progress_state == ProgressState::JustStarted && config.piping {
        let mut t = ansi::bold(&time_string(state, now));
        if now - state.start_time > 15.0 {
            t.push_str(&ansi::grey(
                " nom hasn't detected any input. Have you redirected nix-build stderr into nom? (See -h and the README for details.)",
            ));
        }
        return truncate_output(width, height, &t);
    }

    if state.progress_state == ProgressState::Finished && config.silent {
        return String::new();
    }

    let mut sections: Vec<String> = Vec::new();
    if !state.nix_errors.is_empty() {
        sections.push(error_section(&state.nix_errors, max_height));
    }
    if !state.nix_traces.is_empty() {
        sections.push(trace_section(&state.nix_traces, max_height));
    }
    if !state.forest_roots.is_empty() {
        sections.push(builds_section(state, max_height, now));
    }
    sections.push(table_section(state, now));

    let joined = print_sections(&sections);
    truncate_output(width, height, &joined)
}

fn print_sections(sections: &[String]) -> String {
    let mut s = String::new();
    s.push_str(UPPER_LEFT);
    for (i, sec) in sections.iter().enumerate() {
        if i > 0 {
            s.push_str(ansi::RESET);
            s.push('\n');
            s.push_str(LEFT_T);
        }
        s.push_str(sec);
    }
    s
}

fn error_section(errors: &[String], max_height: usize) -> String {
    section_with_lines(
        errors,
        max_height,
        &ansi::bold_red(&format!(" {} Errors: ", errors.len())),
    )
}

fn trace_section(traces: &[String], max_height: usize) -> String {
    let label = if traces.len() == 1 { "Trace" } else { "Traces" };
    section_with_lines(
        traces,
        max_height,
        &ansi::bold_yellow(&format!(" {} {label}: ", traces.len())),
    )
}

fn section_with_lines(items: &[String], max_height: usize, title: &str) -> String {
    let total_lines: usize = items.iter().map(|s| s.lines().count().max(1)).sum();
    let compact = total_lines > max_height;
    let mut all_lines = Vec::with_capacity(items.len() + 1);
    all_lines.push(format!("{HORIZONTAL}{title}"));
    for item in items {
        let to_show = if compact {
            compact_error(item).to_string()
        } else {
            item.clone()
        };
        for line in to_show.lines() {
            all_lines.push(line.to_string());
        }
    }
    table::prepend_lines(
        "",
        &format!("{VERTICAL} "),
        &format!("{VERTICAL} "),
        &all_lines,
    )
}

fn compact_error(s: &str) -> &str {
    if let Some(idx) = s.find("\n       last 10 log lines:") {
        &s[..idx]
    } else {
        s
    }
}

fn builds_section(state: &NomState, max_height: usize, now: f64) -> String {
    let lines = print_builds(state, max_height, now);
    table::prepend_lines(
        HORIZONTAL,
        &format!("{VERTICAL} "),
        &format!("{VERTICAL} "),
        &lines,
    )
}

fn time_string(state: &NomState, now: f64) -> String {
    if state.progress_state == ProgressState::Finished {
        finish_markup(state, now)
    } else {
        format!("{CLOCK} {}", run_time(state, now))
    }
}

fn finish_markup(state: &NomState, now: f64) -> String {
    let num_failed = state.full_summary.failed_builds.len();
    let clock = crate::time::local_clock_string();
    let suffix = format!(" at {clock} after {}", run_time(state, now));
    if num_failed > 0 {
        ansi::red(&format!(
            "{WARNING} Exited after {num_failed} build failures{suffix}"
        ))
    } else if !state.nix_errors.is_empty() {
        ansi::red(&format!(
            "{WARNING} Exited with {} errors reported by nix{suffix}",
            state.nix_errors.len()
        ))
    } else if !state.nix_traces.is_empty() {
        let trace_label = ansi::yellow(&format!(
            "{WARNING}{}{}",
            ansi::green(" Finished "),
            ansi::yellow(&format!(
                "with {} traces reported by nix",
                state.nix_traces.len()
            ))
        ));
        format!("{trace_label}{}", ansi::green(&suffix))
    } else {
        ansi::green(&format!("Finished{suffix}"))
    }
}

fn run_time(state: &NomState, now: f64) -> String {
    print_duration((now - state.start_time).max(0.0))
}

fn print_duration(secs: f64) -> String {
    let total = secs.round().max(0.0) as i64;
    let s = total % 60;
    let m = (total / 60) % 60;
    let h = (total / 3600) % 24;
    let d = total / 86400;
    if total < 60 {
        format!("{s:02}s")
    } else if total < 3600 {
        format!("{m:02}m{s:02}s")
    } else if total < 86400 {
        format!("{h:02}h{m:02}m{s:02}s")
    } else {
        format!("{d}d{h:02}h{m:02}m{s:02}s")
    }
}

// ---------------- the summary table ----------------

fn table_section(state: &NomState, now: f64) -> String {
    let sum = &state.full_summary;
    let num_running = sum.running_builds.len();
    let num_completed = sum.completed_builds.len();
    let num_planned = sum.planned_builds.len();
    let total_builds = num_running + num_completed + num_planned;
    let downloads_done = sum.completed_downloads.len();
    let downloads_running = sum.running_downloads.len();
    let num_planned_downloads = sum.planned_downloads.len();
    let uploads_done = sum.completed_uploads.len();
    let uploads_running = sum.running_uploads.len();

    let show_builds = total_builds > 0;
    let show_downloads = downloads_done + downloads_running + num_planned_downloads > 0;
    let show_uploads = uploads_done + uploads_running > 0;

    let hosts = collect_hosts(state);
    let show_hosts = hosts.len() > 1;

    let mut headers: Vec<Entry> = Vec::new();
    if show_builds {
        headers.push(Entry::header("Builds").cells(3).with_code(ansi::BOLD));
    }
    if show_downloads {
        headers.push(Entry::header("Downloads").cells(3).with_code(ansi::BOLD));
    }
    if show_uploads {
        headers.push(Entry::header("Uploads").cells(2).with_code(ansi::BOLD));
    }
    if show_hosts {
        headers.push(Entry::header("Host").with_code(ansi::BOLD));
    }
    if headers.is_empty() {
        headers.push(Entry::text(""));
    }

    let mut last_row: Vec<Entry> = Vec::new();
    if show_builds {
        last_row.push(non_zero_bold(RUNNING, num_running, ansi::YELLOW));
        last_row.push(non_zero_bold(DONE, num_completed, ansi::GREEN));
        last_row.push(non_zero_bold(TODO, num_planned, ansi::BLUE));
    }
    if show_downloads {
        last_row.push(non_zero_bold(DOWN, downloads_running, ansi::YELLOW));
        last_row.push(non_zero_bold(DOWN, downloads_done, ansi::GREEN));
        last_row.push(non_zero_bold(TODO, num_planned_downloads, ansi::BLUE));
    }
    if show_uploads {
        last_row.push(non_zero_bold(UP, uploads_running, ansi::YELLOW));
        last_row.push(non_zero_bold(UP, uploads_done, ansi::GREEN));
    }
    last_row.push(Entry::header(&time_string(state, now)).with_code(ansi::BOLD));

    let mut rows = vec![headers];
    if show_hosts {
        for h in &hosts {
            let mut r: Vec<Entry> = Vec::new();
            if show_builds {
                r.push(non_zero_show_bold(
                    RUNNING,
                    state
                        .full_summary
                        .running_builds
                        .values()
                        .filter(|b| &b.host == h)
                        .count(),
                    ansi::YELLOW,
                ));
                r.push(non_zero_show_bold(
                    DONE,
                    state
                        .full_summary
                        .completed_builds
                        .values()
                        .filter(|b| &b.host == h)
                        .count(),
                    ansi::GREEN,
                ));
                r.push(Entry::empty());
            }
            if show_downloads {
                r.push(non_zero_show_bold(
                    DOWN,
                    state
                        .full_summary
                        .running_downloads
                        .values()
                        .filter(|d| &d.host == h)
                        .count(),
                    ansi::YELLOW,
                ));
                r.push(non_zero_show_bold(
                    DOWN,
                    state
                        .full_summary
                        .completed_downloads
                        .values()
                        .filter(|d| &d.host == h)
                        .count(),
                    ansi::GREEN,
                ));
                r.push(Entry::empty());
            }
            if show_uploads {
                r.push(non_zero_show_bold(
                    UP,
                    state
                        .full_summary
                        .running_uploads
                        .values()
                        .filter(|d| &d.host == h)
                        .count(),
                    ansi::YELLOW,
                ));
                r.push(non_zero_show_bold(
                    UP,
                    state
                        .full_summary
                        .completed_uploads
                        .values()
                        .filter(|d| &d.host == h)
                        .count(),
                    ansi::GREEN,
                ));
            }
            r.push(Entry::header(&format!("{h}")).with_code(ansi::MAGENTA));
            rows.push(r);
        }
    }
    rows.push(last_row);
    let lines = table::print_aligned(rows);
    table::prepend_lines(
        &format!("{}{}{} ", HORIZONTAL, HORIZONTAL, HORIZONTAL),
        &format!("{VERTICAL}    "),
        &format!("{LOWER_LEFT}{HORIZONTAL} {BIG_SUM} "),
        &lines,
    )
}

fn collect_hosts(state: &NomState) -> Vec<Host> {
    let mut set: std::collections::BTreeSet<Host> = std::collections::BTreeSet::new();
    set.insert(Host::Localhost);
    for b in state.full_summary.running_builds.values() {
        set.insert(b.host.clone());
    }
    for b in state.full_summary.completed_builds.values() {
        set.insert(b.host.clone());
    }
    for b in state.full_summary.failed_builds.values() {
        set.insert(b.host.clone());
    }
    for d in state.full_summary.completed_downloads.values() {
        set.insert(d.host.clone());
    }
    for d in state.full_summary.completed_uploads.values() {
        set.insert(d.host.clone());
    }
    for d in state.full_summary.running_downloads.values() {
        set.insert(d.host.clone());
    }
    for d in state.full_summary.running_uploads.values() {
        set.insert(d.host.clone());
    }
    set.into_iter().collect()
}

fn non_zero_show_bold(icon: &str, count: usize, color: &str) -> Entry {
    if count > 0 {
        let txt = ansi::wrap(ansi::BOLD, &count.to_string());
        Entry {
            lcontent: icon.to_string(),
            rcontent: txt,
            width: 1,
            codes: color.to_string(),
        }
    } else {
        Entry::empty()
    }
}

fn non_zero_bold(icon: &str, count: usize, color: &str) -> Entry {
    let txt = if count > 0 {
        ansi::wrap(ansi::BOLD, &count.to_string())
    } else {
        count.to_string()
    };
    Entry {
        lcontent: icon.to_string(),
        rcontent: txt,
        width: 1,
        codes: color.to_string(),
    }
}

// ---------------- the build forest ----------------

fn print_builds(state: &NomState, max_height: usize, now: f64) -> Vec<String> {
    let num_roots_raw = state.forest_roots.len();
    // Compute which derivations to show in the tree (up to `max_height`).
    let shown = compute_shown(state, max_height);
    let forest = build_forest(state, &shown);
    let num_roots = forest.len();
    let title = if num_roots_raw <= 1 {
        ansi::bold("Dependency Graph")
    } else if num_roots == num_roots_raw {
        format!("{} with {num_roots} roots", ansi::bold("Dependency Graph"))
    } else {
        format!(
            "{} showing {num_roots} of {num_roots_raw} roots",
            ansi::bold("Dependency Graph")
        )
    };
    let mut lines = vec![format!(" {title}:")];
    let rendered_forest: Vec<TreeNode<String>> = forest
        .into_iter()
        .map(|t| {
            tree::map_roots_twigs_leaves(
                t,
                &|info: DerivationInfoRef| print_node(state, info, TreeLocation::Root, now),
                &|info: DerivationInfoRef| print_node(state, info, TreeLocation::Twig, now),
                &|info: DerivationInfoRef| print_node(state, info, TreeLocation::Leaf, now),
            )
        })
        .collect();
    let body = tree::show_forest(&rendered_forest);
    lines.extend(body);
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[derive(Clone, Copy)]
struct DerivationInfoRef(DerivationId);

fn build_forest(
    state: &NomState,
    shown: &crate::state::DerivationSet,
) -> Vec<TreeNode<DerivationInfoRef>> {
    let mut seen = crate::state::DerivationSet::new();
    let mut out = Vec::new();
    for root in &state.forest_roots {
        if let Some(node) = build_node(*root, state, shown, &mut seen) {
            out.push(node);
        }
    }
    out
}

fn build_node(
    id: DerivationId,
    state: &NomState,
    shown: &crate::state::DerivationSet,
    seen: &mut crate::state::DerivationSet,
) -> Option<TreeNode<DerivationInfoRef>> {
    if !shown.contains(id) || seen.contains(id) {
        return None;
    }
    seen.insert(id);
    let mut children = Vec::new();
    for input in &state.derivation_info(id).input_derivations {
        if let Some(c) = build_node(input.derivation, state, shown, seen) {
            children.push(c);
        }
    }
    Some(TreeNode {
        label: DerivationInfoRef(id),
        children,
    })
}

/// Pick which derivations to show in the tree. Heuristic: roots, then all
/// running builds/transfers, then planned builds, up to `max_height`.
fn compute_shown(state: &NomState, max_height: usize) -> crate::state::DerivationSet {
    let mut out = crate::state::DerivationSet::new();
    if max_height == 0 {
        return out;
    }
    for r in &state.forest_roots {
        out.insert(*r);
        if out.len() >= max_height {
            return out;
        }
    }
    for id in state.full_summary.running_builds.iter().map(|(k, _)| k) {
        out.insert(id);
        if out.len() >= max_height {
            return out;
        }
    }
    for id in state.full_summary.failed_builds.iter().map(|(k, _)| k) {
        out.insert(id);
        if out.len() >= max_height {
            return out;
        }
    }
    for id in state.full_summary.planned_builds.iter() {
        out.insert(id);
        if out.len() >= max_height {
            return out;
        }
    }
    out
}

fn print_node(state: &NomState, r: DerivationInfoRef, _loc: TreeLocation, now: f64) -> String {
    let info = state.derivation_info(r.0);
    let drv_name = state.append_differing_platform(info, &info.name.store_path.name);
    match &info.build_status {
        BuildStatus::Unknown => {
            // Distinguish transfer states: show the first state of the first
            // output store path that has any state set.
            let first_state = info
                .outputs
                .values()
                .filter_map(|id| state.store_path_info(*id).states.first().cloned())
                .next();
            match first_state {
                Some(StorePathState::Downloading(t)) => format!(
                    "{} {drv_name} {} {}",
                    ansi::bold_yellow(&format!("{DOWN} {RUNNING}")),
                    format_from_host(&t.host),
                    duration_chip(now, t.start.0),
                ),
                Some(StorePathState::Uploading(t)) => format!(
                    "{} {drv_name} {} {}",
                    ansi::bold_yellow(&format!("{UP} {RUNNING}")),
                    format_to_host(&t.host),
                    duration_chip(now, t.start.0),
                ),
                Some(StorePathState::DownloadPlanned) => {
                    ansi::blue(&format!("{DOWN} {TODO} {drv_name}"))
                }
                Some(StorePathState::Downloaded(t)) => {
                    let dur = match t.end {
                        Some(e) => duration_chip(e.0, t.start.0),
                        None => String::new(),
                    };
                    ansi::green(&format!(
                        "{DOWN} {DONE} {drv_name} {} {dur}",
                        format_from_host(&t.host)
                    ))
                }
                Some(StorePathState::Uploaded(t)) => {
                    let dur = match t.end {
                        Some(e) => duration_chip(e.0, t.start.0),
                        None => String::new(),
                    };
                    ansi::green(&format!(
                        "{UP} {DONE} {drv_name} {} {dur}",
                        format_to_host(&t.host)
                    ))
                }
                None => drv_name,
            }
        }
        BuildStatus::Planned => ansi::blue(&format!("{TODO} {drv_name}")),
        BuildStatus::Building(b) => {
            let mut s = format!(
                "{} {drv_name} {}",
                ansi::bold_yellow(RUNNING),
                format_on_host(&b.host),
            );
            let elapsed = (now - b.start.0).max(0.0);
            if elapsed > 1.0 {
                s.push_str(&format!(" {CLOCK} {}", print_duration(elapsed)));
            }
            if let Some(est) = b.estimate_secs {
                s.push_str(&format!(" ({AVERAGE} {})", print_duration(est as f64)));
            }
            s
        }
        BuildStatus::Failed(f) => {
            let dur = print_duration((f.end.0 - f.start.0).max(0.0));
            ansi::bold_red(&format!(
                "{WARNING} {drv_name} failed with {} after {CLOCK} {dur}",
                describe_fail(&f.fail)
            ))
        }
        BuildStatus::Built(b) => {
            let dur = (b.end.0 - b.start.0).max(0.0);
            let tail = if dur > 1.0 {
                ansi::grey(&format!(
                    " {} {CLOCK} {}",
                    format_on_host(&b.host),
                    print_duration(dur)
                ))
            } else if !matches!(b.host, Host::Localhost) {
                ansi::grey(&format!(" {}", format_on_host(&b.host)))
            } else {
                String::new()
            };
            format!("{}{tail}", ansi::green(&format!("{DONE} {drv_name}")))
        }
    }
}

fn describe_fail(f: &FailType) -> String {
    match f {
        FailType::ExitCode(c) => format!("exit code {c}"),
        FailType::HashMismatch => "hash mismatch".to_string(),
    }
}

fn format_on_host(h: &Host) -> String {
    match h {
        Host::Localhost => String::new(),
        Host::Remote(name) => format!("on {}", ansi::magenta(name)),
    }
}

fn format_from_host(h: &Host) -> String {
    match h {
        Host::Localhost => String::new(),
        Host::Remote(name) => format!("from {}", ansi::magenta(name)),
    }
}

fn format_to_host(h: &Host) -> String {
    match h {
        Host::Localhost => String::new(),
        Host::Remote(name) => format!("to {}", ansi::magenta(name)),
    }
}

fn duration_chip(now: f64, start: f64) -> String {
    let d = (now - start).max(0.0);
    if d > 1.0 {
        format!("{CLOCK} {}", print_duration(d))
    } else {
        String::new()
    }
}

fn truncate_output(width: Option<usize>, height: Option<usize>, s: &str) -> String {
    if width.is_none() && height.is_none() {
        return s.to_string();
    }
    let lines: Vec<&str> = s.lines().collect();
    let kept: Vec<String> = match height {
        Some(h) if lines.len() >= h.saturating_sub(5) => {
            // Show head and tail with a vertical ellipsis.
            let mut out: Vec<String> = Vec::with_capacity(h);
            if let Some(first) = lines.first() {
                out.push(first.to_string());
            }
            out.push(" ⋮ ".to_string());
            let to_keep = (lines.len() + 5 + 2).saturating_sub(h);
            for line in &lines[to_keep.min(lines.len())..] {
                out.push(line.to_string());
            }
            out
        }
        _ => lines.iter().map(|l| l.to_string()).collect(),
    };
    let truncated: Vec<String> = kept
        .into_iter()
        .map(|l| match width {
            Some(w) if ansi::display_width(&l) > w => {
                let mut t = ansi::truncate(&l, w.saturating_sub(1));
                t.push('…');
                t.push_str(ansi::RESET);
                t
            }
            _ => l,
        })
        .collect();
    truncated.join("\n")
}
