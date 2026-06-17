use std::collections::BTreeMap;
use std::fs::{File, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use crate::builds::Host;

/// `(host, drv_pname) -> { utc_timestamp -> seconds }`.
pub type BuildReportMap = BTreeMap<(Host, String), BTreeMap<String, i32>>;

const FILENAME: &str = "build-reports.csv";
const HEADER: &str = "hostname,derivation name,utc time,build seconds";
const HISTORY_LIMIT: usize = 10;

fn reports_dir() -> Option<PathBuf> {
    let mut p = dirs::state_dir()?;
    p.push("nix-output-monitor");
    Some(p)
}

/// Load the cached build reports. Errors are swallowed and yield an empty map.
pub fn load() -> BuildReportMap {
    let Some(dir) = reports_dir() else {
        return BuildReportMap::new();
    };
    let mut path = dir;
    path.push(FILENAME);
    let Ok(file) = File::open(&path) else {
        return BuildReportMap::new();
    };
    let mut out = BuildReportMap::new();
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    // Skip header.
    let _ = lines.next();
    for line in lines.map_while(Result::ok) {
        if let Some(record) = parse_csv_line(&line) {
            let key = (
                if record.host.is_empty() {
                    Host::Localhost
                } else {
                    Host::Remote(record.host)
                },
                record.drv_name,
            );
            out.entry(key)
                .or_default()
                .insert(record.end_time, record.build_secs);
        }
    }
    out
}

/// Append a new record for `(host, drv_pname)` taking `seconds`, persist to disk,
/// and return the updated in-memory map (clamped to `HISTORY_LIMIT` per key).
pub fn record(
    mut map: BuildReportMap,
    host: Host,
    drv_pname: String,
    end_time: String,
    seconds: i32,
) -> BuildReportMap {
    let inner = map.entry((host, drv_pname)).or_default();
    inner.insert(end_time, seconds);
    while inner.len() > HISTORY_LIMIT {
        let oldest = inner.keys().next().cloned();
        if let Some(k) = oldest {
            inner.remove(&k);
        } else {
            break;
        }
    }
    let _ = save(&map);
    map
}

fn save(map: &BuildReportMap) -> std::io::Result<()> {
    let Some(dir) = reports_dir() else {
        return Ok(());
    };
    create_dir_all(&dir)?;
    let mut path = dir;
    path.push(FILENAME);
    let mut tmp = path.clone();
    tmp.set_extension("csv.tmp");
    {
        let mut f = File::create(&tmp)?;
        writeln!(f, "{HEADER}")?;
        for ((host, name), per_time) in map {
            for (ts, secs) in per_time {
                let host_str = match host {
                    Host::Localhost => "",
                    Host::Remote(s) => s.as_str(),
                };
                writeln!(
                    f,
                    "{},{},{},{}",
                    csv_field(host_str),
                    csv_field(name),
                    csv_field(ts),
                    secs
                )?;
            }
        }
        f.flush()?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Median build duration for `(host, name)`, if any history exists.
pub fn median(map: &BuildReportMap, host: &Host, name: &str) -> Option<i32> {
    let inner = map.get(&(host.clone(), name.to_string()))?;
    let mut values: Vec<i32> = inner.values().copied().collect();
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let len = values.len();
    Some(if len % 2 == 1 {
        values[len / 2]
    } else {
        ((values[len / 2 - 1] as i64 + values[len / 2] as i64) / 2) as i32
    })
}

struct Record {
    host: String,
    drv_name: String,
    end_time: String,
    build_secs: i32,
}

fn parse_csv_line(line: &str) -> Option<Record> {
    let mut fields = Vec::with_capacity(4);
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek().copied() == Some('"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == ',' {
            fields.push(std::mem::take(&mut cur));
        } else if c == '"' {
            in_quotes = true;
        } else {
            cur.push(c);
        }
    }
    fields.push(cur);
    if fields.len() != 4 {
        return None;
    }
    let secs: i32 = fields[3].trim().parse().ok()?;
    Some(Record {
        host: fields[0].clone(),
        drv_name: fields[1].clone(),
        end_time: fields[2].clone(),
        build_secs: secs,
    })
}

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}
