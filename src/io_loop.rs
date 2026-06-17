use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ansi;
use crate::parser_json::parse_line;
use crate::parser_old::parse_old_style_line;
use crate::print::{Config, state_to_text};
use crate::state::NomState;
use crate::update::{
    UpdateOutcome, detect_finished_local_builds, finalize, maintain, process_json_message,
    process_old_style,
};

const MIN_FRAME_DELAY: Duration = Duration::from_millis(60);
const MAX_FRAME_DELAY: Duration = Duration::from_millis(1000);
const STORE_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Input modes nom understands.
pub enum InputMode {
    Json,
    OldStyle,
}

/// Shared between threads.
struct Shared {
    state: Mutex<NomState>,
    output_buffer: Mutex<Vec<u8>>,
    /// Lines printed at the bottom of the screen during the previous redraw,
    /// used to compute how many lines to clear.
    last_line_count: Mutex<usize>,
    refresh: Mutex<bool>,
    cv: Condvar,
    done: AtomicBool,
}

/// Run the IO loop. `reader` produces nix output bytes; `out` receives the
/// rendered terminal updates (typically stderr).
pub fn run<R: Read + Send + 'static, W: Write>(
    config: Config,
    mode: InputMode,
    reader: R,
    mut out: W,
    initial_state: NomState,
) -> std::io::Result<NomState> {
    let shared = Arc::new(Shared {
        state: Mutex::new(initial_state),
        output_buffer: Mutex::new(Vec::new()),
        last_line_count: Mutex::new(0),
        refresh: Mutex::new(false),
        cv: Condvar::new(),
        done: AtomicBool::new(false),
    });

    // hide the cursor and remember to show it on exit (drop guard).
    let _cursor_guard = CursorGuard::new(&mut out)?;

    // Spawn reader thread.
    let reader_shared = shared.clone();
    let reader_handle = thread::spawn(move || reader_thread(reader_shared, reader, mode));

    // Printer loop on the main thread.
    let mut last_store_poll = Instant::now();
    loop {
        // Wait until either refresh is requested or MAX_FRAME_DELAY elapses.
        wait_for_refresh(&shared);

        if shared.done.load(Ordering::SeqCst) {
            break;
        }

        // Periodic disk poll to detect locally-finished builds when nix
        // doesn't tell us they finished.
        if last_store_poll.elapsed() >= STORE_POLL_INTERVAL {
            let mut st = shared.state.lock().unwrap();
            if detect_finished_local_builds(&mut st, crate::time::now()) {
                request_refresh(&shared);
            }
            last_store_poll = Instant::now();
        }

        redraw(&shared, &config, &mut out)?;

        // Throttle: at most ~17 frames/sec.
        thread::sleep(MIN_FRAME_DELAY);
    }

    // Reader finished — let it join.
    let _ = reader_handle.join();

    // Final redraw with `Finished` state.
    {
        let mut st = shared.state.lock().unwrap();
        finalize(&mut st, crate::time::now());
    }
    redraw(&shared, &config, &mut out)?;
    out.write_all(b"\n")?;
    out.flush()?;

    let state = Arc::try_unwrap(shared)
        .map_err(|_| std::io::Error::other("could not reclaim shared state"))?
        .state
        .into_inner()
        .map_err(|_| std::io::Error::other("state mutex poisoned"))?;
    Ok(state)
}

fn wait_for_refresh(shared: &Shared) {
    let lock = shared.refresh.lock().unwrap();
    let (mut guard, _) = shared
        .cv
        .wait_timeout_while(lock, MAX_FRAME_DELAY, |refresh| {
            !*refresh && !shared.done.load(Ordering::SeqCst)
        })
        .unwrap();
    *guard = false;
}

fn request_refresh(shared: &Shared) {
    let mut g = shared.refresh.lock().unwrap();
    *g = true;
    shared.cv.notify_all();
}

fn redraw<W: Write>(shared: &Shared, config: &Config, out: &mut W) -> std::io::Result<()> {
    // Take the buffered pass-through output.
    let pass_through = {
        let mut buf = shared.output_buffer.lock().unwrap();
        std::mem::take(&mut *buf)
    };

    // Snapshot state and maintain it.
    let now = crate::time::now();
    let rendered = {
        let mut st = shared.state.lock().unwrap();
        maintain(&mut st);
        let (w, h) = terminal_size();
        state_to_text(config, &st, w, h, now)
    };

    let pass_lines: Vec<&[u8]> = if pass_through.is_empty() {
        Vec::new()
    } else {
        pass_through.split(|b| *b == b'\n').collect()
    };
    let pass_lines: Vec<&[u8]> = pass_lines.into_iter().filter(|l| !l.is_empty()).collect();

    let nom_lines: Vec<String> = rendered.lines().map(str::to_string).collect();
    let nom_line_count = nom_lines.len();

    let last_printed = {
        let mut last = shared.last_line_count.lock().unwrap();
        let prev = *last;
        *last = nom_line_count;
        prev
    };

    let mut payload: Vec<u8> = Vec::with_capacity(pass_through.len() + rendered.len() + 64);
    payload.extend_from_slice(ansi::BEGIN_SYNC.as_bytes());
    // Move cursor to the start of the current line, clear it.
    if last_printed >= 1 {
        payload.extend_from_slice(ansi::CURSOR_COLUMN_0.as_bytes());
        payload.extend_from_slice(ansi::CLEAR_LINE.as_bytes());
    }
    // Move up and clear additional lines.
    for _ in 1..last_printed {
        payload.extend_from_slice(ansi::CURSOR_UP_LINE.as_bytes());
        payload.extend_from_slice(ansi::CLEAR_LINE.as_bytes());
    }
    // Write any nix passthrough lines first.
    for line in &pass_lines {
        payload.extend_from_slice(line);
        payload.push(b'\n');
    }
    // Then the nom block.
    for (i, line) in nom_lines.iter().enumerate() {
        if i > 0 {
            payload.push(b'\n');
        }
        payload.extend_from_slice(line.as_bytes());
    }
    if nom_line_count == 0 && !pass_lines.is_empty() {
        payload.push(b'\n');
    }
    payload.extend_from_slice(ansi::END_SYNC.as_bytes());

    out.write_all(&payload)?;
    out.flush()
}

fn reader_thread<R: Read>(shared: Arc<Shared>, reader: R, mode: InputMode) {
    let mut buf_reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        match buf_reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Ok(_) => {
                // Trim trailing \n (but keep \r if any — not standard but cheap).
                while matches!(line.last(), Some(&b'\n')) {
                    line.pop();
                }
                let now = crate::time::now();
                let outcome = {
                    let mut st = shared.state.lock().unwrap();
                    match mode {
                        InputMode::Json => {
                            let msg = parse_line(&line);
                            process_json_message(&mut st, msg, now)
                        }
                        InputMode::OldStyle => {
                            let s = String::from_utf8_lossy(&line);
                            let parsed = parse_old_style_line(&s);
                            process_old_style(&mut st, parsed, &line, now)
                        }
                    }
                };
                apply_outcome(&shared, outcome);
            }
            Err(_) => break,
        }
    }
    shared.done.store(true, Ordering::SeqCst);
    request_refresh(&shared);
}

fn apply_outcome(shared: &Shared, outcome: UpdateOutcome) {
    {
        let mut buf = shared.output_buffer.lock().unwrap();
        for err in &outcome.errors {
            let prefix = ansi::bold_red("nix-output-monitor error: ");
            let line = format!("{prefix}{err}\n");
            buf.extend_from_slice(line.as_bytes());
        }
        if !outcome.pass_through.is_empty() {
            buf.extend_from_slice(&outcome.pass_through);
        }
    }
    if outcome.state_changed || !outcome.errors.is_empty() || !outcome.pass_through.is_empty() {
        request_refresh(shared);
    }
}

fn terminal_size() -> (Option<usize>, Option<usize>) {
    match crossterm::terminal::size() {
        Ok((w, h)) if w > 0 && h > 0 => (Some(w as usize), Some(h as usize)),
        _ => (None, None),
    }
}

struct CursorGuard;

impl CursorGuard {
    fn new<W: Write>(out: &mut W) -> std::io::Result<Self> {
        out.write_all(ansi::HIDE_CURSOR.as_bytes())?;
        out.flush()?;
        Ok(Self)
    }
}

impl Drop for CursorGuard {
    fn drop(&mut self) {
        let mut stderr = std::io::stderr();
        let _ = stderr.write_all(ansi::SHOW_CURSOR.as_bytes());
        let _ = stderr.flush();
    }
}
