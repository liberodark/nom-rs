use std::path::PathBuf;

use crate::ansi;
use crate::build_reports;
use crate::builds::{
    Derivation, FailType, Host, IndentedStoreObject, OutputName, STORE_PREFIX, StorePath,
    parse_indented_store_object,
};
use crate::drv_file;
use crate::nix_message::{
    Activity, ActivityResult, MessageAction, NixJsonMessage, NomError, OldStyleMessage,
    ResultAction, StartAction, StopAction, Verbosity,
};
use crate::parser_old::parse_old_style_line;
use crate::state::{
    ActivityStatus, BuildStatus, CompletedBuildInfo, CompletedTransferInfo, DependencySummary,
    DerivationId, DerivationSet, FailedBuildInfo, InputDerivation, NomState, ProgressState,
    RunningBuildInfo, RunningTransferInfo, StorePathId, StorePathState, f64_ord,
    store_path_state_eq, update_summary_for_derivation, update_summary_for_store_path,
};

/// Resulting side effects from processing a single message.
#[derive(Debug, Default)]
pub struct UpdateOutcome {
    pub errors: Vec<NomError>,
    /// Bytes to forward verbatim to the user's terminal (post-prefixing).
    pub pass_through: Vec<u8>,
    /// Whether the visible state changed and the screen needs a redraw.
    pub state_changed: bool,
}

/// Process a JSON message, mutating the state.
pub fn process_json_message(state: &mut NomState, msg: NixJsonMessage, now: f64) -> UpdateOutcome {
    let mut out = UpdateOutcome::default();
    if matches!(state.progress_state, ProgressState::JustStarted) {
        state.progress_state = ProgressState::InputReceived;
        out.state_changed = true;
    }
    match msg {
        NixJsonMessage::Plain(b) => {
            out.pass_through = b;
            out.pass_through.push(b'\n');
        }
        NixJsonMessage::ParseError { msg, raw } => {
            out.errors
                .push(NomError::ParseNixJsonMessageError { msg, raw });
        }
        NixJsonMessage::Message(m) => process_message(state, m, &mut out),
        NixJsonMessage::Result(r) => process_result(state, r, &mut out),
        NixJsonMessage::Start(s) => process_start(state, s, now, &mut out),
        NixJsonMessage::Stop(s) => process_stop(state, s, now, &mut out),
    }
    out
}

/// Process an old-style message; if `result` is `None`, the line was a plain
/// pass-through (forwarded as-is to stdout) but we still want to forward it.
pub fn process_old_style(
    state: &mut NomState,
    parsed: Option<OldStyleMessage>,
    raw_line: &[u8],
    now: f64,
) -> UpdateOutcome {
    let mut out = UpdateOutcome::default();
    if matches!(state.progress_state, ProgressState::JustStarted) {
        state.progress_state = ProgressState::InputReceived;
        out.state_changed = true;
    }
    // Always pass the raw line through.
    out.pass_through = raw_line.to_vec();
    if !out.pass_through.ends_with(b"\n") {
        out.pass_through.push(b'\n');
    }
    let Some(msg) = parsed else {
        return out;
    };
    match msg {
        OldStyleMessage::Uploading { path, to } => {
            let id = state.intern_store_path(path);
            uploaded(state, to, id, now);
            out.state_changed = true;
        }
        OldStyleMessage::Downloading { path, from } => {
            let id = state.intern_store_path(path);
            downloaded(state, from.clone(), id, now);
            finish_build_by_path_id(state, from, id, now);
            out.state_changed = true;
        }
        OldStyleMessage::Build { drv, host } => {
            building(state, host, drv, now, None, &mut out);
            out.state_changed = true;
        }
        OldStyleMessage::Checking(drv) => {
            building(state, Host::Localhost, drv, now, None, &mut out);
            out.state_changed = true;
        }
        OldStyleMessage::Failed { drv, fail } => {
            let id = lookup_derivation(state, drv, &mut out);
            failed_build(state, id, now, fail);
            out.state_changed = true;
        }
    }
    out
}

fn process_message(state: &mut NomState, m: MessageAction, out: &mut UpdateOutcome) {
    let stripped = ansi::strip_ansi(&m.message);
    if m.level <= Verbosity::Info && m.level > Verbosity::Error {
        // Forward the message and try to extract a planned build/download from it.
        out.pass_through.extend_from_slice(m.message.as_bytes());
        out.pass_through.push(b'\n');
        if let Some(obj) = parse_indented_store_object(&m.message) {
            match obj {
                IndentedStoreObject::Drv(d) => {
                    let id = lookup_derivation(state, d, out);
                    set_derivation_status(state, id, BuildStatus::Planned);
                    out.state_changed = true;
                }
                IndentedStoreObject::Path(p) => {
                    let id = state.intern_store_path(p);
                    insert_store_path_state(state, id, StorePathState::DownloadPlanned, None);
                    out.state_changed = true;
                }
            }
        }
        return;
    }
    if m.level == Verbosity::Error {
        if stripped.starts_with("error:") {
            let already_seen = state.nix_errors.iter().any(|prev| {
                ansi::strip_ansi(prev).contains(stripped.trim_start_matches("error:").trim())
            });
            if !already_seen {
                state.nix_errors.push(m.message.clone());
                out.pass_through.extend_from_slice(m.message.as_bytes());
                out.pass_through.push(b'\n');
                out.state_changed = true;
            }
            if let Some(parsed) = parse_old_style_line(&stripped) {
                let inner = process_old_style(state, Some(parsed), b"", crate::time::now());
                out.errors.extend(inner.errors);
            }
        } else if stripped.starts_with("trace:") {
            let already_seen = state.nix_traces.iter().any(|prev| {
                ansi::strip_ansi(prev).contains(stripped.trim_start_matches("trace:").trim())
            });
            if !already_seen {
                state.nix_traces.push(m.message.clone());
                out.pass_through.extend_from_slice(m.message.as_bytes());
                out.pass_through.push(b'\n');
                out.state_changed = true;
            }
        }
    }
}

fn process_result(state: &mut NomState, r: ResultAction, out: &mut UpdateOutcome) {
    let id_value = r.id.0;
    match r.result {
        ActivityResult::BuildLogLine(line) => {
            let prefix = build_log_prefix(state, id_value);
            out.pass_through.extend_from_slice(prefix.as_bytes());
            out.pass_through.extend_from_slice(line.as_bytes());
            out.pass_through.push(b'\n');
        }
        ActivityResult::SetPhase(phase) => {
            if let Some(act) = state.activities.get_mut(&id_value) {
                act.phase = Some(phase);
                out.state_changed = true;
            }
        }
        ActivityResult::Progress(p) => {
            if let Some(act) = state.activities.get_mut(&id_value) {
                act.progress = Some(p);
                out.state_changed = true;
            }
        }
        _ => {}
    }
}

fn build_log_prefix(state: &NomState, activity_id: u64) -> String {
    let Some(act) = state.activities.get(&activity_id) else {
        return String::new();
    };
    let Activity::Build { drv, .. } = &act.activity else {
        return String::new();
    };
    let Some(id) = state.derivation_ids.get(drv) else {
        return String::new();
    };
    let info = state.derivation_info(*id);
    let name = state.append_differing_platform(info, &info.report_name());
    format!(
        "{}{}> {}",
        ansi::RESET,
        ansi::wrap(ansi::BLUE, &name),
        ansi::RESET
    )
}

fn process_start(state: &mut NomState, s: StartAction, now: f64, out: &mut UpdateOutcome) {
    let id_value = s.id.0;
    // Forward non-empty `text` at Info or below verbosity.
    if !s.text.is_empty() && s.level <= Verbosity::Info {
        let prefix = match &s.activity {
            Activity::Build { drv, .. } => {
                if let Some(id) = state.derivation_ids.get(drv).copied() {
                    let info = state.derivation_info(id);
                    let name = state.append_differing_platform(info, &info.report_name());
                    format!(
                        "{}{}> {}",
                        ansi::RESET,
                        ansi::wrap(ansi::BLUE, &name),
                        ansi::RESET
                    )
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };
        out.pass_through.extend_from_slice(prefix.as_bytes());
        out.pass_through.extend_from_slice(s.text.as_bytes());
        out.pass_through.push(b'\n');
    }
    let mut interesting = false;
    let changed = match &s.activity {
        Activity::Build { drv, host } => {
            building(state, host.clone(), drv.clone(), now, Some(s.id.0), out);
            true
        }
        Activity::CopyPath {
            path,
            from,
            to: Host::Localhost,
        } => {
            let id = state.intern_store_path(path.clone());
            downloading(state, from.clone(), id, now);
            true
        }
        Activity::CopyPath {
            path,
            from: Host::Localhost,
            to,
        } => {
            let id = state.intern_store_path(path.clone());
            uploading(state, to.clone(), id, now);
            true
        }
        Activity::Unknown if s.text.starts_with("querying info") => {
            interesting = true;
            true
        }
        Activity::QueryPathInfo { .. } => {
            interesting = true;
            true
        }
        _ => false,
    };
    if interesting {
        state.interesting_activities.insert(id_value);
    }
    if changed {
        state.activities.insert(
            id_value,
            ActivityStatus {
                activity: s.activity,
                phase: None,
                progress: None,
            },
        );
        out.state_changed = true;
    }
}

fn process_stop(state: &mut NomState, s: StopAction, now: f64, out: &mut UpdateOutcome) {
    let id_value = s.id.0;
    let act = state.activities.remove(&id_value);
    let was_interesting = state.interesting_activities.remove(&id_value);
    let Some(act) = act else {
        if was_interesting {
            out.state_changed = true;
        }
        return;
    };
    match act.activity {
        Activity::CopyPath {
            path,
            from,
            to: Host::Localhost,
        } => {
            let id = state.intern_store_path(path);
            downloaded(state, from, id, now);
            out.state_changed = true;
        }
        Activity::CopyPath {
            path,
            from: Host::Localhost,
            to,
        } => {
            let id = state.intern_store_path(path);
            uploaded(state, to, id, now);
            out.state_changed = true;
        }
        Activity::Build { drv, host } => {
            let id = lookup_derivation(state, drv, out);
            // Only transition from Building -> Built. Failed/Built remain as-is.
            if matches!(
                state.derivation_info(id).build_status,
                BuildStatus::Building(_)
            ) {
                finish_build_by_drv_id(state, host, id, now);
                out.state_changed = true;
            }
        }
        _ => {}
    }
}

// ------------------------- transitions -----------------------------

fn building(
    state: &mut NomState,
    host: Host,
    drv: Derivation,
    now: f64,
    activity_id: Option<u64>,
    out: &mut UpdateOutcome,
) {
    let id = lookup_derivation(state, drv, out);
    let pname = state.derivation_info(id).report_name();
    let estimate = build_reports::median(&state.build_reports, &host, &pname);
    set_derivation_status(
        state,
        id,
        BuildStatus::Building(RunningBuildInfo {
            start: f64_ord::F64(now),
            host,
            estimate_secs: estimate,
            activity_id: activity_id.map(crate::nix_message::ActivityId),
        }),
    );
}

fn downloading(state: &mut NomState, host: Host, id: StorePathId, now: f64) {
    insert_store_path_state(
        state,
        id,
        StorePathState::Downloading(RunningTransferInfo {
            host,
            start: f64_ord::F64(now),
        }),
        None,
    );
}

fn downloaded(state: &mut NomState, host: Host, id: StorePathId, now: f64) {
    insert_store_path_state(
        state,
        id,
        StorePathState::Downloaded(CompletedTransferInfo {
            host: host.clone(),
            start: f64_ord::F64(now),
            end: None,
        }),
        Some(Box::new(move |s: StorePathState| match s {
            StorePathState::Downloading(t) if t.host == host => {
                StorePathState::Downloaded(CompletedTransferInfo {
                    host: t.host,
                    start: t.start,
                    end: Some(f64_ord::F64(now)),
                })
            }
            other => other,
        })),
    );
}

fn uploading(state: &mut NomState, host: Host, id: StorePathId, now: f64) {
    insert_store_path_state(
        state,
        id,
        StorePathState::Uploading(RunningTransferInfo {
            host,
            start: f64_ord::F64(now),
        }),
        None,
    );
}

fn uploaded(state: &mut NomState, host: Host, id: StorePathId, now: f64) {
    insert_store_path_state(
        state,
        id,
        StorePathState::Uploaded(CompletedTransferInfo {
            host: host.clone(),
            start: f64_ord::F64(now),
            end: None,
        }),
        Some(Box::new(move |s: StorePathState| match s {
            StorePathState::Uploading(t) if t.host == host => {
                StorePathState::Uploaded(CompletedTransferInfo {
                    host: t.host,
                    start: t.start,
                    end: Some(f64_ord::F64(now)),
                })
            }
            other => other,
        })),
    );
}

fn failed_build(state: &mut NomState, id: DerivationId, now: f64, fail: FailType) {
    let cur = state.derivation_info(id).build_status.clone();
    let (start, host) = match cur {
        BuildStatus::Building(b) => (b.start, b.host),
        BuildStatus::Built(b) => (b.start, b.host),
        _ => return,
    };
    set_derivation_status(
        state,
        id,
        BuildStatus::Failed(FailedBuildInfo {
            start,
            host,
            end: f64_ord::F64(now),
            fail,
        }),
    );
}

fn finish_build_by_drv_id(state: &mut NomState, host: Host, id: DerivationId, now: f64) {
    let info = state.derivation_info(id);
    let cur = info.build_status.clone();
    let BuildStatus::Building(b) = cur else {
        return;
    };
    let pname = info.report_name();
    let secs = (now - b.start.0).max(0.0).floor() as i32;
    let host_for_report = host.clone();
    state.build_reports = build_reports::record(
        std::mem::take(&mut state.build_reports),
        host_for_report,
        pname,
        crate::time::utc_now_string(),
        secs,
    );
    set_derivation_status(
        state,
        id,
        BuildStatus::Built(CompletedBuildInfo {
            start: b.start,
            host: b.host,
            end: f64_ord::F64(now),
        }),
    );
    // `host` was the host reported by the message; we keep the original build's host above.
    let _ = host;
}

fn finish_build_by_path_id(state: &mut NomState, host: Host, path_id: StorePathId, now: f64) {
    let Some(drv_id) = state.store_path_info(path_id).producer else {
        return;
    };
    if matches!(
        state.derivation_info(drv_id).build_status,
        BuildStatus::Building(_)
    ) {
        finish_build_by_drv_id(state, host, drv_id, now);
    }
}

fn derivation_to_any_out_path(state: &NomState, id: DerivationId) -> Option<StorePath> {
    let info = state.derivation_info(id);
    let path_id = info.outputs.values().next()?;
    Some(state.store_path_info(*path_id).name.clone())
}

/// Resolve an id for `drv`, reading the `.drv` file the first time we see it.
fn lookup_derivation(
    state: &mut NomState,
    drv: Derivation,
    out: &mut UpdateOutcome,
) -> DerivationId {
    let id = state.intern_derivation(drv.clone());
    let cached = state.derivation_info(id).cached;
    if !cached {
        let path = PathBuf::from(format!("{drv}"));
        match drv_file::read(&path) {
            Ok(parsed) => insert_derivation_into_state(state, parsed, id),
            Err(_) if !path.exists() => {
                // Silent — the .drv file is not present locally. Mark cached
                // so we don't keep retrying it.
                state.derivation_info_mut(id).cached = true;
            }
            Err(e) => out.errors.push(NomError::DerivationReadError(e)),
        }
    }
    id
}

fn insert_derivation_into_state(
    state: &mut NomState,
    parsed: drv_file::ParsedDerivation,
    id: DerivationId,
) {
    // Outputs: register each output store path and link to this derivation.
    let mut outputs = std::collections::BTreeMap::new();
    for (name, path_str) in parsed.outputs {
        let Some(sp) = StorePath::parse(&path_str) else {
            continue;
        };
        let path_id = state.intern_store_path(sp);
        state.store_path_info_mut(path_id).producer = Some(id);
        outputs.insert(OutputName::parse(&name), path_id);
    }
    // Input sources.
    let mut input_sources = crate::state::StorePathSet::new();
    for src in parsed.input_srcs {
        let Some(sp) = StorePath::parse(&src) else {
            continue;
        };
        let path_id = state.intern_store_path(sp);
        state.store_path_info_mut(path_id).input_for.insert(id);
        input_sources.insert(path_id);
    }
    // Input derivations.
    let mut input_drvs = Vec::new();
    for (drv_path, _outs) in parsed.input_drvs {
        let Some(dep_drv) = Derivation::parse(&drv_path) else {
            continue;
        };
        let dep_id = state.intern_derivation(dep_drv);
        if !state.derivation_info(dep_id).cached {
            // Try to recursively load — silent if the .drv file is missing.
            let p = PathBuf::from(format!(
                "{}.drv",
                state.derivation_info(dep_id).name.store_path
            ));
            match drv_file::read(&p) {
                Ok(child) => insert_derivation_into_state(state, child, dep_id),
                Err(_) => {
                    state.derivation_info_mut(dep_id).cached = true;
                }
            }
        }
        state
            .derivation_info_mut(dep_id)
            .derivation_parents
            .insert(id);
        state.forest_roots.retain(|r| *r != dep_id);
        input_drvs.push(InputDerivation { derivation: dep_id });
    }
    {
        let info = state.derivation_info_mut(id);
        info.outputs = outputs;
        info.input_sources = input_sources;
        info.input_derivations = input_drvs;
        info.cached = true;
        info.platform = Some(parsed.platform);
        info.pname = parsed.env.get("pname").cloned();
    }
    if state.derivation_info(id).derivation_parents.is_empty() {
        state.forest_roots.insert(0, id);
    }
}

// ------------------------ summary plumbing -------------------------

fn set_derivation_status(state: &mut NomState, id: DerivationId, new: BuildStatus) {
    let old = state.derivation_info(id).build_status.clone();
    if std::mem::discriminant(&old) == std::mem::discriminant(&new) && build_status_eq(&old, &new) {
        return;
    }
    state.derivation_info_mut(id).build_status = new.clone();
    let parents = state.derivation_info(id).derivation_parents.clone();
    propagate_summary(state, &parents, &|s, _| {
        update_summary_for_derivation(s, &old, &new, id);
    });
    update_summary_for_derivation(&mut state.full_summary, &old, &new, id);
    state.touched_ids.union_with(&parents);
}

fn build_status_eq(a: &BuildStatus, b: &BuildStatus) -> bool {
    match (a, b) {
        (BuildStatus::Unknown, BuildStatus::Unknown) => true,
        (BuildStatus::Planned, BuildStatus::Planned) => true,
        (BuildStatus::Building(x), BuildStatus::Building(y)) => {
            x.start == y.start && x.host == y.host && x.activity_id == y.activity_id
        }
        (BuildStatus::Built(x), BuildStatus::Built(y)) => {
            x.start == y.start && x.host == y.host && x.end == y.end
        }
        (BuildStatus::Failed(x), BuildStatus::Failed(y)) => {
            x.start == y.start && x.host == y.host && x.end == y.end && x.fail == y.fail
        }
        _ => false,
    }
}

fn insert_store_path_state(
    state: &mut NomState,
    id: StorePathId,
    new: StorePathState,
    update_existing: Option<Box<dyn Fn(StorePathState) -> StorePathState>>,
) {
    let old = state.store_path_info(id).states.clone();
    let mut merged = if let Some(f) = update_existing {
        old.iter().cloned().map(f).collect::<Vec<_>>()
    } else {
        old.clone()
    };
    // Locally filter then insert.
    match &new {
        StorePathState::Downloading(_) | StorePathState::Downloaded(_) => {
            merged.retain(|s| !matches!(s, StorePathState::DownloadPlanned));
        }
        _ => {}
    }
    if !merged.iter().any(|s| store_path_state_eq(s, &new)) {
        merged.push(new);
    }
    state.store_path_info_mut(id).states = merged.clone();

    let info_clone = state.store_path_info(id).clone();
    let mut parents: DerivationSet = info_clone.input_for.clone();
    if let Some(p) = info_clone.producer {
        parents.insert(p);
    }
    propagate_summary(state, &parents, &|s, _| {
        update_summary_for_store_path(s, &old, &merged, id);
    });
    update_summary_for_store_path(&mut state.full_summary, &old, &merged, id);
    state.touched_ids.union_with(&parents);
}

/// Walk transitively from `direct_parents` upward and apply `f` to each
/// ancestor's [`DerivationInfo::dependency_summary`].
fn propagate_summary<F>(state: &mut NomState, direct_parents: &DerivationSet, f: &F)
where
    F: Fn(&mut DependencySummary, DerivationId),
{
    let mut to_visit: Vec<DerivationId> = direct_parents.iter().collect();
    let mut visited = DerivationSet::new();
    while let Some(id) = to_visit.pop() {
        if !visited.insert(id) {
            continue;
        }
        let info = state.derivation_info_mut(id);
        f(&mut info.dependency_summary, id);
        for p in info.derivation_parents.iter() {
            if !visited.contains(p) {
                to_visit.push(p);
            }
        }
    }
}

/// `tick`: do background maintenance — re-sort forest roots if any ids were touched.
pub fn maintain(state: &mut NomState) {
    if state.touched_ids.is_empty() {
        return;
    }
    let touched = std::mem::take(&mut state.touched_ids);
    let _ = touched;
    // Sort forest roots by start-time of earliest running build; cheap heuristic.
    // Build the keys up front so the closure does not need to borrow `state`.
    let mut keyed: Vec<(DerivationId, (u8, f64_ord::F64))> = state
        .forest_roots
        .iter()
        .map(|id| {
            let info = state.derivation_info(*id);
            let k = match &info.build_status {
                BuildStatus::Building(b) => (0u8, b.start),
                BuildStatus::Failed(b) => (1, b.start),
                BuildStatus::Planned => (2, f64_ord::F64(0.0)),
                BuildStatus::Built(b) => (3, b.start),
                BuildStatus::Unknown => (4, f64_ord::F64(0.0)),
            };
            (*id, k)
        })
        .collect();
    keyed.sort_by_key(|a| a.1);
    state.forest_roots = keyed.into_iter().map(|(id, _)| id).collect();
}

/// Scan still-running local builds; if their output now exists in the store,
/// mark them built. Returns whether anything changed.
pub fn detect_finished_local_builds(state: &mut NomState, now: f64) -> bool {
    let running: Vec<DerivationId> = state
        .full_summary
        .running_builds
        .iter()
        .filter_map(|(id, info)| {
            if matches!(info.host, Host::Localhost) {
                Some(id)
            } else {
                None
            }
        })
        .collect();
    let mut changed = false;
    for id in running {
        let Some(out_path) = derivation_to_any_out_path(state, id) else {
            continue;
        };
        if std::path::Path::new(&out_path.to_string()).exists() {
            finish_build_by_drv_id(state, Host::Localhost, id, now);
            changed = true;
        }
    }
    changed
}

/// Mark `progress_state` as Finished and run a final disk check.
pub fn finalize(state: &mut NomState, now: f64) {
    detect_finished_local_builds(state, now);
    state.progress_state = ProgressState::Finished;
}

// Quietly use this constant only via `STORE_PREFIX` import once; keeps the use list trimmed.
const _: &str = STORE_PREFIX;
