use crate::ansi;

#[derive(Debug, Clone)]
pub struct Entry {
    /// Optional left-aligned label (the icon, usually).
    pub lcontent: String,
    /// Right-aligned content (the count, usually).
    pub rcontent: String,
    /// How many display columns this entry spans.
    pub width: usize,
    /// ANSI prefix codes to apply.
    pub codes: String,
}

impl Entry {
    pub fn empty() -> Self {
        Self::text("")
    }
    pub fn text(s: &str) -> Self {
        Self {
            lcontent: String::new(),
            rcontent: s.to_string(),
            width: 1,
            codes: String::new(),
        }
    }
    pub fn header(s: &str) -> Self {
        Self {
            lcontent: s.to_string(),
            rcontent: String::new(),
            width: 1,
            codes: String::new(),
        }
    }
    pub fn cells(mut self, w: usize) -> Self {
        self.width = w;
        self
    }
    pub fn with_code(mut self, c: &str) -> Self {
        self.codes.push_str(c);
        self
    }

    fn entry_width(&self) -> usize {
        let lw = ansi::display_width(&self.lcontent);
        let rw = ansi::display_width(&self.rcontent);
        if lw == 0 || rw == 0 {
            lw + rw
        } else {
            lw + rw + 1
        }
    }
}

const HSEP: &str = " │ ";

/// Render a table to a non-empty Vec of lines.
pub fn print_aligned(rows: Vec<Vec<Entry>>) -> Vec<String> {
    if rows.is_empty() {
        return vec![String::new()];
    }
    let widths = compute_column_widths(&rows);
    rows.iter().map(|row| render_row(row, &widths)).collect()
}

fn compute_column_widths(rows: &[Vec<Entry>]) -> Vec<usize> {
    let mut widths = Vec::new();
    let mut rows = rows.to_vec();
    while rows.iter().any(|r| !r.is_empty()) {
        let next_width = rows
            .iter()
            .filter_map(|r| r.first())
            .filter(|e| e.width == 1)
            .map(|e| e.entry_width())
            .max()
            .unwrap_or(0);
        widths.push(next_width);
        let target = next_width;
        rows = rows.iter().filter_map(|r| chop_first(r, target)).collect();
    }
    widths
}

fn chop_first(row: &[Entry], target_width: usize) -> Option<Vec<Entry>> {
    let first = row.first()?;
    if first.width > 1 {
        let mut shrunk = first.clone();
        shrunk.width -= 1;
        let pad = first
            .entry_width()
            .saturating_sub(target_width + ansi::display_width(HSEP));
        shrunk.lcontent = String::new();
        shrunk.rcontent = " ".repeat(pad);
        let mut new_row = vec![shrunk];
        new_row.extend_from_slice(&row[1..]);
        Some(new_row)
    } else {
        let rest: Vec<Entry> = row.iter().skip(1).cloned().collect();
        if rest.is_empty() { None } else { Some(rest) }
    }
}

fn render_row(row: &[Entry], widths: &[usize]) -> String {
    let mut cols_left: &[usize] = widths;
    let mut parts: Vec<String> = Vec::new();
    for e in row {
        let take = e.width.min(cols_left.len());
        let span = &cols_left[..take];
        cols_left = &cols_left[take..];
        let mut total = span.iter().sum::<usize>();
        if take > 0 {
            total += ansi::display_width(HSEP) * (take - 1);
        }
        parts.push(render_entry(e, total));
    }
    parts.join(HSEP)
}

fn render_entry(e: &Entry, target_width: usize) -> String {
    let lw = ansi::display_width(&e.lcontent);
    let rw = ansi::display_width(&e.rcontent);
    let spaces = target_width.saturating_sub(lw + rw);
    let body = format!("{}{}{}", e.lcontent, " ".repeat(spaces), e.rcontent);
    if e.codes.is_empty() {
        body
    } else {
        format!("{}{}{}", e.codes, body, ansi::RESET)
    }
}

/// Prepend `top` to the first row, `mid` to all other rows except the last,
/// and `bot` to the last row.
pub fn prepend_lines(top: &str, mid: &str, bot: &str, rows: &[String]) -> String {
    assert!(!rows.is_empty(), "prepend_lines requires at least one row");
    let mut out = String::new();
    out.push_str(top);
    out.push_str(&rows[0]);
    if rows.len() > 1 {
        for line in &rows[1..rows.len() - 1] {
            out.push('\n');
            out.push_str(mid);
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(bot);
        out.push_str(&rows[rows.len() - 1]);
    }
    out
}
