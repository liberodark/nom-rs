use crate::builds::{Derivation, FailType, Host, StorePath};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ActivityId(pub u64);

/// Nix verbosity levels (0 highest, 7 lowest).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum Verbosity {
    Error = 0,
    Warn = 1,
    Notice = 2,
    Info = 3,
    Talkative = 4,
    Chatty = 5,
    Debug = 6,
    Vomit = 7,
}

impl Verbosity {
    pub fn from_int(n: i64) -> Option<Self> {
        Some(match n {
            0 => Self::Error,
            1 => Self::Warn,
            2 => Self::Notice,
            3 => Self::Info,
            4 => Self::Talkative,
            5 => Self::Chatty,
            6 => Self::Debug,
            7 => Self::Vomit,
            _ => return None,
        })
    }
}

/// Nix activity type ids. See `src/libutil/include/nix/util/logging.hh` in nix.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActivityType {
    Unknown,
    CopyPath,
    FileTransfer,
    Realise,
    CopyPaths,
    Builds,
    Build,
    OptimiseStore,
    VerifyPaths,
    Substitute,
    QueryPathInfo,
    PostBuildHook,
    BuildWaiting,
    FetchTree,
}

impl ActivityType {
    pub fn from_int(n: i64) -> Option<Self> {
        Some(match n {
            0 => Self::Unknown,
            100 => Self::CopyPath,
            101 => Self::FileTransfer,
            102 => Self::Realise,
            103 => Self::CopyPaths,
            104 => Self::Builds,
            105 => Self::Build,
            106 => Self::OptimiseStore,
            107 => Self::VerifyPaths,
            108 => Self::Substitute,
            109 => Self::QueryPathInfo,
            110 => Self::PostBuildHook,
            111 => Self::BuildWaiting,
            112 => Self::FetchTree,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Activity {
    Unknown,
    CopyPath {
        path: StorePath,
        from: Host,
        to: Host,
    },
    FileTransfer(String),
    Realise,
    CopyPaths,
    Builds,
    Build {
        drv: Derivation,
        host: Host,
    },
    OptimiseStore,
    VerifyPaths,
    Substitute {
        path: StorePath,
        host: Host,
    },
    QueryPathInfo {
        path: StorePath,
        host: Host,
    },
    PostBuildHook(Derivation),
    BuildWaiting,
    FetchTree,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct ActivityProgress {
    pub done: i64,
    pub expected: i64,
    pub running: i64,
    pub failed: i64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ActivityResult {
    FileLinked { size: i64, hashed: i64 },
    BuildLogLine(String),
    UntrustedPath(StorePath),
    CorruptedPath(StorePath),
    SetPhase(String),
    Progress(ActivityProgress),
    SetExpected(ActivityType, i64),
    PostBuildLogLine(String),
    FetchStatus(String),
}

#[derive(Debug, Clone)]
pub struct StartAction {
    pub id: ActivityId,
    pub level: Verbosity,
    pub text: String,
    pub activity: Activity,
}

#[derive(Debug, Clone)]
pub struct StopAction {
    pub id: ActivityId,
}

#[derive(Debug, Clone)]
pub struct ResultAction {
    pub id: ActivityId,
    pub result: ActivityResult,
}

#[derive(Debug, Clone)]
pub struct MessageAction {
    pub level: Verbosity,
    pub message: String,
}

/// A single parsed `@nix ...` line (or a plain pass-through line).
#[derive(Debug, Clone)]
pub enum NixJsonMessage {
    Start(StartAction),
    Stop(StopAction),
    Result(ResultAction),
    Message(MessageAction),
    /// A non-`@nix` line we pass through unchanged.
    Plain(Vec<u8>),
    /// A line that started with `@nix ` but did not parse.
    ParseError {
        msg: String,
        raw: Vec<u8>,
    },
}

/// A human-readable (old-style) nix log message, parsed from stderr text.
#[derive(Debug, Clone)]
pub enum OldStyleMessage {
    Uploading { path: StorePath, to: Host },
    Downloading { path: StorePath, from: Host },
    Build { drv: Derivation, host: Host },
    Checking(Derivation),
    Failed { drv: Derivation, fail: FailType },
}

/// Errors emitted during input handling/parsing.
#[derive(Debug, Clone)]
pub enum NomError {
    DerivationReadError(String),
    ParseNixJsonMessageError { msg: String, raw: Vec<u8> },
}

impl std::fmt::Display for NomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NomError::DerivationReadError(s) => write!(f, "derivation read error: {s}"),
            NomError::ParseNixJsonMessageError { msg, raw } => {
                write!(
                    f,
                    "parse error: {msg} (raw: {})",
                    String::from_utf8_lossy(raw)
                )
            }
        }
    }
}
