use std::collections::{BTreeMap, HashMap};

use crate::build_reports::BuildReportMap;
use crate::builds::{Derivation, FailType, Host, OutputName, StorePath};
use crate::cache_id::{CacheId, IdMap, IdSet};
use crate::nix_message::{Activity, ActivityId, ActivityProgress};

pub type DerivationId = CacheId<Derivation>;
pub type StorePathId = CacheId<StorePath>;
pub type DerivationSet = IdSet<Derivation>;
pub type StorePathSet = IdSet<StorePath>;
pub type DerivationMap<V> = IdMap<Derivation, V>;
pub type StorePathMap<V> = IdMap<StorePath, V>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StorePathState {
    DownloadPlanned,
    Downloading(RunningTransferInfo),
    Uploading(RunningTransferInfo),
    Downloaded(CompletedTransferInfo),
    Uploaded(CompletedTransferInfo),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RunningTransferInfo {
    pub host: Host,
    pub start: f64_ord::F64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CompletedTransferInfo {
    pub host: Host,
    pub start: f64_ord::F64,
    pub end: Option<f64_ord::F64>,
}

/// A wrapper module providing a total-ordered f64 for use inside Ord structures.
pub mod f64_ord {
    use std::cmp::Ordering;

    #[derive(Debug, Clone, Copy)]
    pub struct F64(pub f64);

    impl PartialEq for F64 {
        fn eq(&self, other: &Self) -> bool {
            self.0.total_cmp(&other.0) == Ordering::Equal
        }
    }
    impl Eq for F64 {}
    impl PartialOrd for F64 {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for F64 {
        fn cmp(&self, other: &Self) -> Ordering {
            self.0.total_cmp(&other.0)
        }
    }
}

pub type Time = f64_ord::F64;

#[derive(Debug, Clone)]
pub struct RunningBuildInfo {
    pub start: Time,
    pub host: Host,
    pub estimate_secs: Option<i32>,
    pub activity_id: Option<ActivityId>,
}

#[derive(Debug, Clone)]
pub struct CompletedBuildInfo {
    pub start: Time,
    pub host: Host,
    pub end: Time,
}

#[derive(Debug, Clone)]
pub struct FailedBuildInfo {
    pub start: Time,
    pub host: Host,
    pub end: Time,
    pub fail: FailType,
}

#[derive(Debug, Clone)]
pub enum BuildStatus {
    Unknown,
    Planned,
    Building(RunningBuildInfo),
    Failed(FailedBuildInfo),
    Built(CompletedBuildInfo),
}

#[derive(Debug, Clone, Default)]
pub struct DependencySummary {
    pub planned_builds: DerivationSet,
    pub running_builds: DerivationMap<RunningBuildInfo>,
    pub completed_builds: DerivationMap<CompletedBuildInfo>,
    pub failed_builds: DerivationMap<FailedBuildInfo>,
    pub planned_downloads: StorePathSet,
    pub completed_downloads: StorePathMap<CompletedTransferInfo>,
    pub completed_uploads: StorePathMap<CompletedTransferInfo>,
    pub running_downloads: StorePathMap<RunningTransferInfo>,
    pub running_uploads: StorePathMap<RunningTransferInfo>,
}

#[derive(Debug, Clone)]
pub struct InputDerivation {
    pub derivation: DerivationId,
}

#[derive(Debug, Clone)]
pub struct DerivationInfo {
    pub name: Derivation,
    pub outputs: BTreeMap<OutputName, StorePathId>,
    pub input_derivations: Vec<InputDerivation>,
    pub input_sources: StorePathSet,
    pub build_status: BuildStatus,
    pub dependency_summary: DependencySummary,
    pub cached: bool,
    pub derivation_parents: DerivationSet,
    pub pname: Option<String>,
    pub platform: Option<String>,
}

impl DerivationInfo {
    pub fn empty(d: Derivation) -> Self {
        Self {
            name: d,
            outputs: BTreeMap::new(),
            input_derivations: Vec::new(),
            input_sources: StorePathSet::new(),
            build_status: BuildStatus::Unknown,
            dependency_summary: DependencySummary::default(),
            cached: false,
            derivation_parents: DerivationSet::new(),
            pname: None,
            platform: None,
        }
    }

    /// Name shown in the UI: pname if available, otherwise derivation name with
    /// trailing version chars stripped.
    pub fn report_name(&self) -> String {
        if let Some(p) = &self.pname {
            return p.clone();
        }
        let n = &self.name.store_path.name;
        let bytes = n.as_bytes();
        let mut end = bytes.len();
        while end > 0 {
            let b = bytes[end - 1];
            if b.is_ascii_digit() || matches!(b, b'.' | b'-') {
                end -= 1;
            } else {
                break;
            }
        }
        n[..end].to_string()
    }
}

#[derive(Debug, Clone)]
pub struct StorePathInfo {
    pub name: StorePath,
    pub states: Vec<StorePathState>,
    pub producer: Option<DerivationId>,
    pub input_for: DerivationSet,
}

impl StorePathInfo {
    pub fn empty(p: StorePath) -> Self {
        Self {
            name: p,
            states: Vec::new(),
            producer: None,
            input_for: DerivationSet::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActivityStatus {
    pub activity: Activity,
    pub phase: Option<String>,
    pub progress: Option<ActivityProgress>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ProgressState {
    JustStarted,
    InputReceived,
    Finished,
}

#[derive(Debug, Clone)]
pub struct NomState {
    pub derivation_infos: DerivationMap<DerivationInfo>,
    pub store_path_infos: StorePathMap<StorePathInfo>,
    pub full_summary: DependencySummary,
    pub forest_roots: Vec<DerivationId>,
    pub build_reports: BuildReportMap,
    pub start_time: f64,
    pub progress_state: ProgressState,
    pub store_path_ids: HashMap<StorePath, StorePathId>,
    pub derivation_ids: HashMap<Derivation, DerivationId>,
    pub touched_ids: DerivationSet,
    pub activities: HashMap<u64, ActivityStatus>,
    pub nix_errors: Vec<String>,
    pub nix_traces: Vec<String>,
    pub build_platform: Option<String>,
    pub interesting_activities: std::collections::HashSet<u64>,
}

impl NomState {
    pub fn new(now: f64, platform: Option<String>, build_reports: BuildReportMap) -> Self {
        Self {
            derivation_infos: DerivationMap::new(),
            store_path_infos: StorePathMap::new(),
            full_summary: DependencySummary::default(),
            forest_roots: Vec::new(),
            build_reports,
            start_time: now,
            progress_state: ProgressState::JustStarted,
            store_path_ids: HashMap::new(),
            derivation_ids: HashMap::new(),
            touched_ids: DerivationSet::new(),
            activities: HashMap::new(),
            nix_errors: Vec::new(),
            nix_traces: Vec::new(),
            build_platform: platform,
            interesting_activities: std::collections::HashSet::new(),
        }
    }

    /// Resolve or allocate an id for the given store path.
    pub fn intern_store_path(&mut self, path: StorePath) -> StorePathId {
        if let Some(id) = self.store_path_ids.get(&path) {
            return *id;
        }
        let id = self.store_path_infos.next_id();
        self.store_path_infos
            .insert(id, StorePathInfo::empty(path.clone()));
        self.store_path_ids.insert(path, id);
        id
    }

    /// Resolve or allocate an id for the given derivation.
    pub fn intern_derivation(&mut self, drv: Derivation) -> DerivationId {
        if let Some(id) = self.derivation_ids.get(&drv) {
            return *id;
        }
        let id = self.derivation_infos.next_id();
        self.derivation_infos
            .insert(id, DerivationInfo::empty(drv.clone()));
        self.derivation_ids.insert(drv, id);
        id
    }

    pub fn derivation_info(&self, id: DerivationId) -> &DerivationInfo {
        self.derivation_infos
            .get(id)
            .expect("BUG: drv id missing from derivation_infos")
    }

    pub fn derivation_info_mut(&mut self, id: DerivationId) -> &mut DerivationInfo {
        self.derivation_infos
            .get_mut(id)
            .expect("BUG: drv id missing from derivation_infos")
    }

    pub fn store_path_info(&self, id: StorePathId) -> &StorePathInfo {
        self.store_path_infos
            .get(id)
            .expect("BUG: store path id missing from store_path_infos")
    }

    pub fn store_path_info_mut(&mut self, id: StorePathId) -> &mut StorePathInfo {
        self.store_path_infos
            .get_mut(id)
            .expect("BUG: store path id missing from store_path_infos")
    }

    /// Append `-<platform>` to a derivation name when the derivation's platform
    /// differs from the host's build platform.
    pub fn append_differing_platform(&self, info: &DerivationInfo, base: &str) -> String {
        match (&self.build_platform, &info.platform) {
            (Some(p1), Some(p2)) if p1 != p2 => format!("{base}-{p2}"),
            _ => base.to_string(),
        }
    }
}

/// Insert/remove a derivation from a summary based on a status transition.
pub fn update_summary_for_derivation(
    summary: &mut DependencySummary,
    old: &BuildStatus,
    new: &BuildStatus,
    id: DerivationId,
) {
    clear_derivation_id_from_summary(summary, old, id);
    match new {
        BuildStatus::Unknown => {}
        BuildStatus::Planned => {
            summary.planned_builds.insert(id);
        }
        BuildStatus::Building(info) => {
            summary.running_builds.insert(id, info.clone());
        }
        BuildStatus::Failed(info) => {
            summary.failed_builds.insert(id, info.clone());
        }
        BuildStatus::Built(info) => {
            summary.completed_builds.insert(id, info.clone());
        }
    }
}

pub fn clear_derivation_id_from_summary(
    summary: &mut DependencySummary,
    old: &BuildStatus,
    id: DerivationId,
) {
    match old {
        BuildStatus::Unknown => {}
        BuildStatus::Planned => {
            summary.planned_builds.remove(id);
        }
        BuildStatus::Building(_) => {
            summary.running_builds.remove(id);
        }
        BuildStatus::Failed(_) => {
            summary.failed_builds.remove(id);
        }
        BuildStatus::Built(_) => {
            summary.completed_builds.remove(id);
        }
    }
}

/// Apply state-set changes for a store path to a summary.
pub fn update_summary_for_store_path(
    summary: &mut DependencySummary,
    old_states: &[StorePathState],
    new_states: &[StorePathState],
    id: StorePathId,
) {
    for s in old_states {
        if !contains_state(new_states, s) {
            remove_store_path_state_from_summary(summary, s, id);
        }
    }
    for s in new_states {
        if !contains_state(old_states, s) {
            insert_store_path_state_into_summary(summary, s, id);
        }
    }
}

fn contains_state(states: &[StorePathState], s: &StorePathState) -> bool {
    states.iter().any(|x| store_path_state_eq(x, s))
}

pub fn store_path_state_eq(a: &StorePathState, b: &StorePathState) -> bool {
    use StorePathState as S;
    match (a, b) {
        (S::DownloadPlanned, S::DownloadPlanned) => true,
        (S::Downloading(x), S::Downloading(y)) => x == y,
        (S::Uploading(x), S::Uploading(y)) => x == y,
        (S::Downloaded(x), S::Downloaded(y)) => x == y,
        (S::Uploaded(x), S::Uploaded(y)) => x == y,
        _ => false,
    }
}

fn insert_store_path_state_into_summary(
    summary: &mut DependencySummary,
    s: &StorePathState,
    id: StorePathId,
) {
    match s {
        StorePathState::DownloadPlanned => {
            summary.planned_downloads.insert(id);
        }
        StorePathState::Downloading(info) => {
            summary.running_downloads.insert(id, info.clone());
        }
        StorePathState::Uploading(info) => {
            summary.running_uploads.insert(id, info.clone());
        }
        StorePathState::Downloaded(info) => {
            summary.completed_downloads.insert(id, info.clone());
        }
        StorePathState::Uploaded(info) => {
            summary.completed_uploads.insert(id, info.clone());
        }
    }
}

fn remove_store_path_state_from_summary(
    summary: &mut DependencySummary,
    s: &StorePathState,
    id: StorePathId,
) {
    match s {
        StorePathState::DownloadPlanned => {
            summary.planned_downloads.remove(id);
        }
        StorePathState::Downloading(_) => {
            summary.running_downloads.remove(id);
        }
        StorePathState::Uploading(_) => {
            summary.running_uploads.remove(id);
        }
        StorePathState::Downloaded(_) => {
            summary.completed_downloads.remove(id);
        }
        StorePathState::Uploaded(_) => {
            summary.completed_uploads.remove(id);
        }
    }
}
