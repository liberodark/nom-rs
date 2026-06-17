use serde_json::Value;

use crate::builds::{Derivation, StorePath, parse_host};
use crate::nix_message::{
    Activity, ActivityId, ActivityProgress, ActivityResult, ActivityType, MessageAction,
    NixJsonMessage, ResultAction, StartAction, StopAction, Verbosity,
};

const NIX_PREFIX: &[u8] = b"@nix ";

/// Parse a single line of nix output. The line should not include a trailing newline.
pub fn parse_line(line: &[u8]) -> NixJsonMessage {
    let Some(rest) = line.strip_prefix(NIX_PREFIX) else {
        return NixJsonMessage::Plain(line.to_vec());
    };
    match serde_json::from_slice::<Value>(rest) {
        Ok(val) => match parse_action(&val) {
            Ok(msg) => msg,
            Err(e) => NixJsonMessage::ParseError {
                msg: e,
                raw: rest.to_vec(),
            },
        },
        Err(e) => NixJsonMessage::ParseError {
            msg: e.to_string(),
            raw: rest.to_vec(),
        },
    }
}

fn parse_action(v: &Value) -> Result<NixJsonMessage, String> {
    let obj = v.as_object().ok_or("expected JSON object")?;
    let action = obj
        .get("action")
        .and_then(Value::as_str)
        .ok_or("missing 'action'")?;
    match action {
        "start" => Ok(NixJsonMessage::Start(parse_start(obj)?)),
        "stop" => Ok(NixJsonMessage::Stop(parse_stop(obj)?)),
        "result" => Ok(NixJsonMessage::Result(parse_result(obj)?)),
        "msg" => Ok(NixJsonMessage::Message(parse_message(obj)?)),
        other => Err(format!("unknown action type: {other}")),
    }
}

fn obj_get<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    obj.get(key).ok_or_else(|| format!("missing field '{key}'"))
}

fn get_i64(obj: &serde_json::Map<String, Value>, key: &str) -> Result<i64, String> {
    obj_get(obj, key)?
        .as_i64()
        .ok_or_else(|| format!("field '{key}' is not an integer"))
}

fn get_u64(obj: &serde_json::Map<String, Value>, key: &str) -> Result<u64, String> {
    obj_get(obj, key)?
        .as_u64()
        .ok_or_else(|| format!("field '{key}' is not an unsigned integer"))
}

fn get_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, String> {
    obj_get(obj, key)?
        .as_str()
        .ok_or_else(|| format!("field '{key}' is not a string"))
}

fn get_verbosity(obj: &serde_json::Map<String, Value>) -> Result<Verbosity, String> {
    let n = get_i64(obj, "level")?;
    Verbosity::from_int(n).ok_or_else(|| format!("invalid verbosity level: {n}"))
}

fn parse_stop(obj: &serde_json::Map<String, Value>) -> Result<StopAction, String> {
    Ok(StopAction {
        id: ActivityId(get_u64(obj, "id")?),
    })
}

fn parse_message(obj: &serde_json::Map<String, Value>) -> Result<MessageAction, String> {
    Ok(MessageAction {
        level: get_verbosity(obj)?,
        message: get_str(obj, "msg")?.to_string(),
    })
}

fn fields_text(obj: &serde_json::Map<String, Value>) -> Result<Vec<String>, String> {
    let arr = obj_get(obj, "fields")?
        .as_array()
        .ok_or("'fields' is not an array")?;
    arr.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| "expected text field".to_string())
        })
        .collect()
}

fn fields_int(obj: &serde_json::Map<String, Value>) -> Result<Vec<i64>, String> {
    let arr = obj_get(obj, "fields")?
        .as_array()
        .ok_or("'fields' is not an array")?;
    arr.iter()
        .map(|v| {
            v.as_i64()
                .ok_or_else(|| "expected integer field".to_string())
        })
        .collect()
}

/// Mixed text-or-int fields (used by Build activities).
fn fields_text_or_int(obj: &serde_json::Map<String, Value>) -> Result<Vec<TextOrInt>, String> {
    let arr = obj_get(obj, "fields")?
        .as_array()
        .ok_or("'fields' is not an array")?;
    arr.iter()
        .map(|v| match v {
            Value::String(s) => Ok(TextOrInt::Text(s.clone())),
            Value::Number(n) if n.is_i64() => Ok(TextOrInt::Int),
            _ => Err("expected text or int field".to_string()),
        })
        .collect()
}

enum TextOrInt {
    Text(String),
    Int,
}

fn expect_text(t: TextOrInt) -> Result<String, String> {
    match t {
        TextOrInt::Text(s) => Ok(s),
        TextOrInt::Int => Err("got int, expected text".to_string()),
    }
}

fn parse_result(obj: &serde_json::Map<String, Value>) -> Result<ResultAction, String> {
    let id = ActivityId(get_u64(obj, "id")?);
    let ty = get_i64(obj, "type")?;
    let result = match ty {
        100 => {
            let f = fields_int(obj)?;
            need_n(&f, 2, "type=100 result")?;
            ActivityResult::FileLinked {
                size: f[0],
                hashed: f[1],
            }
        }
        101 => ActivityResult::BuildLogLine(one_text(obj)?),
        102 => ActivityResult::UntrustedPath(
            StorePath::parse(&one_text(obj)?).ok_or("invalid store path")?,
        ),
        103 => ActivityResult::CorruptedPath(
            StorePath::parse(&one_text(obj)?).ok_or("invalid store path")?,
        ),
        104 => ActivityResult::SetPhase(one_text(obj)?),
        105 => {
            let f = fields_int(obj)?;
            need_n(&f, 4, "type=105 result")?;
            ActivityResult::Progress(ActivityProgress {
                done: f[0],
                expected: f[1],
                running: f[2],
                failed: f[3],
            })
        }
        106 => {
            let f = fields_int(obj)?;
            need_n(&f, 2, "type=106 result")?;
            let activity_type = ActivityType::from_int(f[0])
                .ok_or_else(|| format!("invalid activity type: {}", f[0]))?;
            ActivityResult::SetExpected(activity_type, f[1])
        }
        107 => ActivityResult::PostBuildLogLine(one_text(obj)?),
        108 => ActivityResult::FetchStatus(one_text(obj)?),
        other => return Err(format!("invalid activity result type: {other}")),
    };
    Ok(ResultAction { id, result })
}

fn need_n<T>(v: &[T], n: usize, ctx: &str) -> Result<(), String> {
    if v.len() != n {
        Err(format!("{ctx}: expected {n} fields, got {}", v.len()))
    } else {
        Ok(())
    }
}

fn one_text(obj: &serde_json::Map<String, Value>) -> Result<String, String> {
    let mut f = fields_text(obj)?;
    if f.len() != 1 {
        return Err(format!("expected 1 text field, got {}", f.len()));
    }
    Ok(f.swap_remove(0))
}

fn parse_start(obj: &serde_json::Map<String, Value>) -> Result<StartAction, String> {
    let id = ActivityId(get_u64(obj, "id")?);
    let text = get_str(obj, "text")?.to_string();
    let level = get_verbosity(obj)?;
    let type_int = get_i64(obj, "type")?;
    let activity_type = ActivityType::from_int(type_int)
        .ok_or_else(|| format!("invalid activity type: {type_int}"))?;
    let activity = match activity_type {
        ActivityType::Unknown => Activity::Unknown,
        ActivityType::CopyPath => {
            let f = fields_text(obj)?;
            need_n(&f, 3, "CopyPath")?;
            Activity::CopyPath {
                path: StorePath::parse(&f[0]).ok_or("invalid store path in CopyPath")?,
                from: parse_host(&f[1]),
                to: parse_host(&f[2]),
            }
        }
        ActivityType::FileTransfer => Activity::FileTransfer(one_text(obj)?),
        ActivityType::Realise => Activity::Realise,
        ActivityType::CopyPaths => Activity::CopyPaths,
        ActivityType::Builds => Activity::Builds,
        ActivityType::Build => {
            let f = fields_text_or_int(obj)?;
            if f.len() < 2 {
                return Err(format!("Build: expected >=2 fields, got {}", f.len()));
            }
            let mut it = f.into_iter();
            let drv_s = expect_text(it.next().unwrap())?;
            let host_s = expect_text(it.next().unwrap())?;
            Activity::Build {
                drv: Derivation::parse(&drv_s).ok_or("invalid derivation path in Build")?,
                host: parse_host(&host_s),
            }
        }
        ActivityType::OptimiseStore => Activity::OptimiseStore,
        ActivityType::VerifyPaths => Activity::VerifyPaths,
        ActivityType::Substitute => {
            let f = fields_text(obj)?;
            need_n(&f, 2, "Substitute")?;
            Activity::Substitute {
                path: StorePath::parse(&f[0]).ok_or("invalid store path in Substitute")?,
                host: parse_host(&f[1]),
            }
        }
        ActivityType::QueryPathInfo => {
            let f = fields_text(obj)?;
            need_n(&f, 2, "QueryPathInfo")?;
            Activity::QueryPathInfo {
                path: StorePath::parse(&f[0]).ok_or("invalid store path in QueryPathInfo")?,
                host: parse_host(&f[1]),
            }
        }
        ActivityType::PostBuildHook => Activity::PostBuildHook(
            Derivation::parse(&one_text(obj)?).ok_or("invalid derivation in PostBuildHook")?,
        ),
        ActivityType::BuildWaiting => Activity::BuildWaiting,
        ActivityType::FetchTree => Activity::FetchTree,
    };
    Ok(StartAction {
        id,
        level,
        text,
        activity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_progress_result() {
        let line = br#"@nix {"action":"result","fields":[1,2,3,4],"id":1,"type":105}"#;
        let m = parse_line(line);
        match m {
            NixJsonMessage::Result(r) => match r.result {
                ActivityResult::Progress(p) => {
                    assert_eq!((p.done, p.expected, p.running, p.failed), (1, 2, 3, 4));
                }
                other => panic!("unexpected result variant: {other:?}"),
            },
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[test]
    fn parses_a_start_build() {
        let line = br#"@nix {"action":"start","fields":["/nix/store/xv3g9i3081rqx4wilyi304c17axrajnq-build1.drv","",0,0],"id":7,"level":3,"text":"building","type":105}"#;
        let m = parse_line(line);
        match m {
            NixJsonMessage::Start(s) => match s.activity {
                Activity::Build { drv, host } => {
                    assert_eq!(drv.store_path.name, "build1");
                    assert!(matches!(host, crate::builds::Host::Localhost));
                }
                other => panic!("unexpected activity: {other:?}"),
            },
            other => panic!("unexpected message: {other:?}"),
        }
    }
}
