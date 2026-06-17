use std::fmt;

pub const STORE_PREFIX: &str = "/nix/store/";
pub const HASH_LEN: usize = 32;

/// A nix store path: `/nix/store/<hash>-<name>`.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct StorePath {
    pub hash: String,
    pub name: String,
}

impl fmt::Display for StorePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{STORE_PREFIX}{}-{}", self.hash, self.name)
    }
}

impl StorePath {
    /// Parse a store path of the form `/nix/store/<32-char hash>-<name>`.
    pub fn parse(input: &str) -> Option<Self> {
        let rest = input.strip_prefix(STORE_PREFIX)?;
        if rest.len() < HASH_LEN + 2 {
            return None;
        }
        let hash = &rest[..HASH_LEN];
        if rest.as_bytes().get(HASH_LEN).is_none_or(|b| *b != b'-') {
            return None;
        }
        let name = &rest[HASH_LEN + 1..];
        if name.is_empty() || !is_valid_name(name) {
            return None;
        }
        Some(Self {
            hash: hash.to_string(),
            name: name.to_string(),
        })
    }
}

fn is_valid_name(name: &str) -> bool {
    name.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'?' | b'=' | b'_' | b'.' | b'+' | b'-'))
}

/// A derivation is a store path whose name ends in `.drv`. We strip the suffix.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Derivation {
    pub store_path: StorePath,
}

impl fmt::Display for Derivation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.drv", self.store_path)
    }
}

impl Derivation {
    /// Parse a derivation from a full path like `/nix/store/<hash>-<name>.drv`.
    pub fn parse(input: &str) -> Option<Self> {
        let path = StorePath::parse(input)?;
        let real_name = path.name.strip_suffix(".drv")?;
        Some(Self {
            store_path: StorePath {
                hash: path.hash,
                name: real_name.to_string(),
            },
        })
    }
}

/// Either a derivation (`.drv`) or a plain store path. Used when parsing
/// the "  /nix/store/..." indented lines emitted by nix.
pub enum IndentedStoreObject {
    Drv(Derivation),
    Path(StorePath),
}

/// Parse `"  <store-path>"` (two-space indented).
pub fn parse_indented_store_object(input: &str) -> Option<IndentedStoreObject> {
    let body = input.strip_prefix("  ")?;
    let path = StorePath::parse(body)?;
    Some(match path.name.strip_suffix(".drv") {
        Some(real) => IndentedStoreObject::Drv(Derivation {
            store_path: StorePath {
                hash: path.hash,
                name: real.to_string(),
            },
        }),
        None => IndentedStoreObject::Path(path),
    })
}

/// A build/transfer host.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Host {
    Localhost,
    Remote(String),
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Localhost => write!(f, "localhost"),
            Host::Remote(name) => write!(f, "{name}"),
        }
    }
}

/// Parse a host string. Empty/`local`/`unix` map to localhost.
pub fn parse_host(s: &str) -> Host {
    match s {
        "" | "local" | "local://" | "unix" | "unix://" => Host::Localhost,
        other => Host::Remote(other.to_string()),
    }
}

/// Why a build failed.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum FailType {
    ExitCode(i32),
    HashMismatch,
}

impl fmt::Display for FailType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FailType::ExitCode(c) => write!(f, "exit code {c}"),
            FailType::HashMismatch => write!(f, "hash mismatch"),
        }
    }
}

/// Conventional output names of a derivation. Anything else gets `Other(name)`.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum OutputName {
    Out,
    Doc,
    Dev,
    Bin,
    Info,
    Lib,
    Man,
    Dist,
    Other(String),
}

impl OutputName {
    pub fn parse(name: &str) -> Self {
        match name {
            "out" => Self::Out,
            "doc" => Self::Doc,
            "dev" => Self::Dev,
            "bin" => Self::Bin,
            "info" => Self::Info,
            "lib" => Self::Lib,
            "man" => Self::Man,
            "dist" => Self::Dist,
            other => Self::Other(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_store_path() {
        let p = StorePath::parse("/nix/store/22d93x5fqmrwfxp18fyb4labbs1q2slw-build1").unwrap();
        assert_eq!(p.hash, "22d93x5fqmrwfxp18fyb4labbs1q2slw");
        assert_eq!(p.name, "build1");
    }

    #[test]
    fn parses_a_derivation() {
        let d =
            Derivation::parse("/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv").unwrap();
        assert_eq!(d.store_path.name, "build1");
    }

    #[test]
    fn host_parses_local_aliases() {
        assert!(matches!(parse_host(""), Host::Localhost));
        assert!(matches!(parse_host("local"), Host::Localhost));
        assert!(matches!(parse_host("unix://"), Host::Localhost));
        assert!(matches!(parse_host("ssh://foo"), Host::Remote(_)));
    }
}
