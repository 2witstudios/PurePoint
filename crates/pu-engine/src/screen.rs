//! Screen-state view of an agent's PTY output.
//!
//! Raw PTY bytes are fed through a terminal emulator so callers can ask what
//! is actually on screen (is the input box ready? what text is in it?) instead
//! of guessing from output timing.

use crate::output_buffer::OutputBuffer;

/// What the agent's input box currently holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputBox {
    /// Text typed into the box; wrapped rows are joined with `\n`.
    pub text: String,
}

impl InputBox {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Drops all whitespace so text that was wrapped or re-flowed by the terminal
/// compares equal to the original.
pub fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn is_prompt_marker(c: char) -> bool {
    c == '>' || c == '❯'
}

fn is_border(row: &str) -> bool {
    let t = row.trim_start();
    t.starts_with('╭') || t.starts_with('╰') || t.starts_with("──") || t.starts_with('─')
}

fn is_top_border(row: &str) -> bool {
    let t = row.trim_start();
    t.starts_with('╭') || t.starts_with('─')
}

/// Strips the box side border and padding from a row.
fn strip_sides(row: &str) -> &str {
    let t = row.trim();
    let t = t.strip_prefix('│').unwrap_or(t);
    let t = t.strip_suffix('│').unwrap_or(t);
    t.trim()
}

/// Finds the input box in a list of screen rows: the last prompt row that is
/// preceded by a top border and followed (after any wrapped rows) by a bottom
/// border. Echoed turns in the transcript also start with `>`, but are never
/// enclosed by borders.
pub fn find_input_box(rows: &[String]) -> Option<InputBox> {
    for start in (0..rows.len()).rev() {
        let first = strip_sides(&rows[start]);
        let mut chars = first.chars();
        let Some(marker) = chars.next() else { continue };
        if !is_prompt_marker(marker) || start == 0 || !is_top_border(&rows[start - 1]) {
            continue;
        }
        let mut lines = vec![chars.as_str().trim().to_string()];
        let mut end = start + 1;
        while end < rows.len() && !is_border(&rows[end]) {
            lines.push(strip_sides(&rows[end]).to_string());
            end += 1;
        }
        if end >= rows.len() {
            continue; // no bottom border: mid-redraw
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        return Some(InputBox {
            text: lines.join("\n"),
        });
    }
    None
}

/// Incrementally feeds an [`OutputBuffer`] into a terminal emulator.
pub struct ScreenTracker {
    parser: vt100::Parser,
    offset: usize,
    rows: u16,
    cols: u16,
}

impl ScreenTracker {
    /// Replays everything currently in `buffer` onto a fresh `rows`×`cols`
    /// screen.
    pub fn new(buffer: &OutputBuffer, rows: u16, cols: u16) -> Self {
        let mut t = Self {
            parser: vt100::Parser::new(rows, cols, 0),
            offset: 0,
            rows,
            cols,
        };
        t.sync(buffer);
        t
    }

    /// Feeds any output written since the last sync.
    pub fn sync(&mut self, buffer: &OutputBuffer) {
        let (bytes, next) = buffer.read_from(self.offset);
        self.parser.process(&bytes);
        self.offset = next;
    }

    pub fn rows(&self) -> Vec<String> {
        self.parser.screen().rows(0, self.cols).collect()
    }

    /// Screen rows with dim cells blanked. Claude Code draws a dim
    /// placeholder hint (`Try "..."`) in its empty input box, and that must
    /// not read as typed text.
    fn rows_without_dim(&self) -> Vec<String> {
        let screen = self.parser.screen();
        (0..self.rows)
            .map(|r| {
                let mut row = String::new();
                for c in 0..self.cols {
                    match screen.cell(r, c) {
                        Some(cell) if cell.is_wide_continuation() => {}
                        Some(cell) if cell.has_contents() && !cell.dim() => {
                            row.push_str(cell.contents())
                        }
                        _ => row.push(' '),
                    }
                }
                row
            })
            .collect()
    }

    pub fn input_box(&self) -> Option<InputBox> {
        find_input_box(&self.rows_without_dim())
    }

    /// The input box is drawn and empty, so keystrokes will land in it.
    pub fn is_ready(&self) -> bool {
        self.input_box().is_some_and(|b| b.is_empty())
    }

    /// Whether `text` appears on screen outside the input box (an echoed turn).
    pub fn echoes(&self, text: &str) -> bool {
        let rows = self.rows();
        let boxed = self.box_row_range(&rows);
        let outside: String = rows
            .iter()
            .enumerate()
            .filter(|(i, _)| !boxed.contains(i))
            .map(|(_, r)| squash(&strip_echo_marker(r)))
            .collect();
        outside.contains(&squash(text))
    }

    /// True when `text` is too large to expect it fully visible on screen.
    pub fn exceeds_screen(&self, text: &str) -> bool {
        squash(text).chars().count() > (self.rows as usize * self.cols as usize) / 2
    }

    fn box_row_range(&self, rows: &[String]) -> std::ops::Range<usize> {
        let Some(bottom) = (0..rows.len()).rev().find(|&i| {
            let t = rows[i].trim_start();
            t.starts_with('╰') || t.starts_with('─')
        }) else {
            return 0..0;
        };
        let top = (0..bottom)
            .rev()
            .find(|&i| is_top_border(&rows[i]))
            .unwrap_or(bottom);
        top..bottom + 1
    }
}

fn strip_echo_marker(row: &str) -> String {
    let t = row.trim_start();
    match t.chars().next() {
        Some(c) if is_prompt_marker(c) => t[c.len_utf8()..].to_string(),
        _ => row.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn given_empty_box_should_report_empty_input() {
        let r = rows(&["╭────╮", "│ >  │", "╰────╯"]);
        let b = find_input_box(&r).unwrap();
        assert!(b.is_empty());
    }

    #[test]
    fn given_typed_text_should_return_it() {
        let r = rows(&["╭──────╮", "│ > hi │", "╰──────╯"]);
        assert_eq!(find_input_box(&r).unwrap().text, "hi");
    }

    #[test]
    fn given_wrapped_text_should_join_rows() {
        let r = rows(&["╭──────╮", "│ > ab │", "│   cd │", "╰──────╯"]);
        assert_eq!(squash(&find_input_box(&r).unwrap().text), "abcd");
    }

    #[test]
    fn given_echoed_turn_above_box_should_read_only_the_box() {
        let r = rows(&["> old turn", "", "╭──────╮", "│ > x  │", "╰──────╯"]);
        assert_eq!(find_input_box(&r).unwrap().text, "x");
    }

    #[test]
    fn given_rule_style_input_should_be_found() {
        let r = rows(&["────────", "❯ hello", "────────"]);
        assert_eq!(find_input_box(&r).unwrap().text, "hello");
    }

    #[test]
    fn given_no_box_should_return_none() {
        assert!(find_input_box(&rows(&["loading hooks..."])).is_none());
    }

    #[test]
    fn given_mid_redraw_without_bottom_border_should_return_none() {
        assert!(find_input_box(&rows(&["╭────╮", "│ > a│"])).is_none());
    }

    #[test]
    fn given_pty_output_should_track_ready_and_echo() {
        let buf = OutputBuffer::new();
        buf.write(b"loading...\r\n");
        let mut t = ScreenTracker::new(&buf, 24, 80);
        assert!(!t.is_ready());
        buf.write(
            "\x1b[2J\x1b[H> full text here\r\n╭────────╮\r\n│ >      │\r\n╰────────╯\r\n"
                .as_bytes(),
        );
        t.sync(&buf);
        assert!(t.is_ready());
        assert!(t.echoes("full text here"));
        assert!(!t.echoes("something else"));
    }

    #[test]
    fn given_dim_placeholder_in_empty_box_should_be_ready() {
        // Claude Code's empty box, as captured from a live agent.
        let buf = OutputBuffer::new();
        buf.write(
            "\x1b[38;2;136;136;136m────────────────────\r\n\x1b[39m❯\u{a0}\x1b[2mTry \"fix typecheck errors\"\r\n\x1b[22m\x1b[38;2;136;136;136m────────────────────\x1b[39m\r\n"
                .as_bytes(),
        );
        let t = ScreenTracker::new(&buf, 24, 80);
        assert!(t.input_box().is_some_and(|b| b.is_empty()));
        assert!(t.is_ready());
    }

    #[test]
    fn given_typed_text_after_placeholder_should_read_typed_text() {
        let buf = OutputBuffer::new();
        buf.write(
            "────────\r\n❯\u{a0}\x1b[2mTry \"x\"\x1b[22m\r\n────────\r\n\x1b[2;3H\x1b[Khello\r\n"
                .as_bytes(),
        );
        let t = ScreenTracker::new(&buf, 24, 80);
        assert_eq!(t.input_box().unwrap().text, "hello");
    }
}
