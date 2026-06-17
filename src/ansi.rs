pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";

pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const GREY: &str = "\x1b[90m";

pub const CLEAR_LINE: &str = "\x1b[2K";
pub const CURSOR_UP_LINE: &str = "\x1b[1F";
pub const CURSOR_COLUMN_0: &str = "\x1b[1G";
pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";

// Synchronized update mode (iTerm2 spec).
pub const BEGIN_SYNC: &str = "\x1b[?2026h";
pub const END_SYNC: &str = "\x1b[?2026l";

/// Wrap `s` with ANSI codes `open` and a closing reset.
pub fn wrap(open: &str, s: &str) -> String {
    let mut out = String::with_capacity(open.len() + s.len() + RESET.len());
    out.push_str(open);
    out.push_str(s);
    out.push_str(RESET);
    out
}

pub fn bold(s: &str) -> String {
    wrap(BOLD, s)
}
pub fn red(s: &str) -> String {
    wrap(RED, s)
}
pub fn green(s: &str) -> String {
    wrap(GREEN, s)
}
pub fn yellow(s: &str) -> String {
    wrap(YELLOW, s)
}
pub fn blue(s: &str) -> String {
    wrap(BLUE, s)
}
pub fn magenta(s: &str) -> String {
    wrap(MAGENTA, s)
}
pub fn grey(s: &str) -> String {
    wrap(GREY, s)
}
pub fn bold_red(s: &str) -> String {
    wrap("\x1b[1;31m", s)
}
pub fn bold_yellow(s: &str) -> String {
    wrap("\x1b[1;33m", s)
}

/// Display width of a string, ignoring ANSI escape sequences.
/// Heuristic: count code points, dropping anything between ESC `[` ... `m`.
pub fn display_width(s: &str) -> usize {
    let mut w = 0usize;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\x1b' {
            in_esc = true;
        } else {
            w += 1;
        }
    }
    w
}

/// Truncate `s` to at most `cut` display columns (does not break ANSI sequences mid-flight).
pub fn truncate(s: &str, cut: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut w = 0usize;
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            out.push(c);
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\x1b' {
            in_esc = true;
            out.push(c);
        } else {
            if w >= cut {
                break;
            }
            out.push(c);
            w += 1;
        }
    }
    out
}

/// Strip ANSI codes from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_esc = false;
    for c in s.chars() {
        if in_esc {
            if c == 'm' {
                in_esc = false;
            }
        } else if c == '\x1b' {
            in_esc = true;
        } else {
            out.push(c);
        }
    }
    out
}
