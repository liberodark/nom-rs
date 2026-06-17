use crate::builds::{Derivation, FailType, Host, StorePath, parse_host};
use crate::nix_message::OldStyleMessage;

/// Try to parse one full line (with no trailing newline) of nix stderr.
pub fn parse_old_style_line(line: &str) -> Option<OldStyleMessage> {
    if let Some(rest) = line.strip_prefix("building ") {
        return parse_building(rest);
    }
    if let Some(rest) = line.strip_prefix("checking outputs of ") {
        return parse_checking(rest);
    }
    if let Some(rest) = line.strip_prefix("copying ") {
        return parse_copying(rest);
    }
    let after = line.strip_prefix("error: ").unwrap_or(line);
    if let Some(msg) = parse_failed(after) {
        return Some(msg);
    }
    None
}

/// `building '<drv>'[ on '<host>']...`
fn parse_building(rest: &str) -> Option<OldStyleMessage> {
    let (drv_str, after) = take_ticked(rest)?;
    let drv = Derivation::parse(drv_str)?;
    if after == "..." {
        return Some(OldStyleMessage::Build {
            drv,
            host: Host::Localhost,
        });
    }
    let on_rest = after.strip_prefix(" on ")?;
    let (host_str, tail) = take_ticked(on_rest)?;
    if tail != "..." {
        return None;
    }
    Some(OldStyleMessage::Build {
        drv,
        host: parse_host(host_str),
    })
}

/// `checking outputs of '<drv>'...`
fn parse_checking(rest: &str) -> Option<OldStyleMessage> {
    let (drv_str, after) = take_ticked(rest)?;
    if after != "..." {
        return None;
    }
    let drv = Derivation::parse(drv_str)?;
    Some(OldStyleMessage::Checking(drv))
}

/// `copying path '<sp>' from/to '<host>'...` (`copying <N> paths...` is left as a
/// plain pass-through line, since nom does nothing with it).
fn parse_copying(rest: &str) -> Option<OldStyleMessage> {
    if rest.ends_with(" paths...") {
        return None;
    }
    let after_path = rest.strip_prefix("path ")?;
    let (sp_str, after) = take_ticked(after_path)?;
    let sp = StorePath::parse(sp_str)?;
    if let Some(to_rest) = after.strip_prefix(" to ") {
        let (host_str, tail) = take_ticked(to_rest)?;
        if tail != "..." {
            return None;
        }
        return Some(OldStyleMessage::Uploading {
            path: sp,
            to: parse_host(host_str),
        });
    }
    if let Some(from_rest) = after.strip_prefix(" from ") {
        let (host_str, tail) = take_ticked(from_rest)?;
        if tail != "..." {
            return None;
        }
        return Some(OldStyleMessage::Downloading {
            path: sp,
            from: parse_host(host_str),
        });
    }
    None
}

/// `builder for '<drv>' failed with exit code <n>` (possibly prefixed
/// by `error: build of '<drv>' failed: error: ` from nix `>=2.18`).
fn parse_failed(rest: &str) -> Option<OldStyleMessage> {
    // Two shapes: ExitCode or HashMismatch.
    if let Some(after) = rest.strip_prefix("builder for ") {
        let (drv_str, tail) = take_ticked(after)?;
        let drv = Derivation::parse(drv_str)?;
        let code_rest = tail.strip_prefix(" failed with exit code ")?;
        let code_str = code_rest.trim_end_matches(';').split_whitespace().next()?;
        let code: i32 = code_str.parse().ok()?;
        return Some(OldStyleMessage::Failed {
            drv,
            fail: FailType::ExitCode(code),
        });
    }
    if let Some(after) = rest.strip_prefix("hash mismatch in fixed-output derivation ") {
        let (drv_str, tail) = take_ticked(after)?;
        if !tail.starts_with(':') {
            return None;
        }
        let drv = Derivation::parse(drv_str)?;
        return Some(OldStyleMessage::Failed {
            drv,
            fail: FailType::HashMismatch,
        });
    }
    // The nix-2.18+ wrapper around the same error:
    if let Some(after) = rest.strip_prefix("build of ") {
        let (_drv_str, tail) = take_ticked(after)?;
        let inner = tail.find("failed: error: ").map(|i| &tail[i + 15..])?;
        return parse_failed(inner);
    }
    None
}

/// Take a `'...'` prefix, returning (contents, rest after the closing quote).
fn take_ticked(s: &str) -> Option<(&str, &str)> {
    let after_open = s.strip_prefix('\'')?;
    let end = after_open.find('\'')?;
    Some((&after_open[..end], &after_open[end + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_building_local() {
        let m = parse_old_style_line(
            "building '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv'...",
        )
        .unwrap();
        match m {
            OldStyleMessage::Build { drv, host } => {
                assert_eq!(drv.store_path.name, "build1");
                assert!(matches!(host, Host::Localhost));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_copying_path_to_remote() {
        let m = parse_old_style_line(
            "copying path '/nix/store/22d93x5fqmrwfxp18fyb4labbs1q2slw-build1' to 'ssh://x'...",
        )
        .unwrap();
        assert!(matches!(m, OldStyleMessage::Uploading { .. }));
    }

    #[test]
    fn parses_failed_exit_code() {
        let m = parse_old_style_line(
            "builder for '/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv' failed with exit code 2",
        )
        .unwrap();
        assert!(matches!(
            m,
            OldStyleMessage::Failed {
                fail: FailType::ExitCode(2),
                ..
            }
        ));
    }
}
