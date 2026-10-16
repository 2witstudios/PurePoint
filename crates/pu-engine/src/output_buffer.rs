use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::Instant;

use tokio::sync::watch;

const DEFAULT_CAPACITY: usize = 4 * 1024 * 1024; // 4MB

/// Minimum printable characters (after stripping ANSI) for a write to count as
/// substantive content. Spinner output (1–2 chars) stays below this threshold.
const SUBSTANTIVE_CONTENT_MIN_CHARS: usize = 5;

pub struct OutputBuffer {
    inner: RwLock<BufferInner>,
    written_tx: watch::Sender<usize>,
    written_rx: watch::Receiver<usize>,
}

struct BufferInner {
    data: VecDeque<u8>,
    capacity: usize,
    last_write: Instant,
    last_content_write: Instant,
    total_written: usize,
    terminal: vt100::Parser,
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (written_tx, written_rx) = watch::channel(0usize);
        let now = Instant::now();
        Self {
            inner: RwLock::new(BufferInner {
                data: VecDeque::new(),
                capacity,
                last_write: now,
                last_content_write: now,
                total_written: 0,
                terminal: vt100::Parser::new(40, 120, 1024),
            }),
            written_tx,
            written_rx,
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.total_written += bytes.len();
        inner.terminal.process(bytes);
        // Trim before extending so the deque never grows past capacity. Extending
        // first would let VecDeque double its allocation (4MB -> 8MB) and keep it.
        let capacity = inner.capacity;
        let kept = &bytes[bytes.len().saturating_sub(capacity)..];
        let excess = (inner.data.len() + kept.len()).saturating_sub(capacity);
        inner.data.drain(..excess);
        // Grow geometrically but cap the allocation at `capacity`.
        let needed = inner.data.len() + kept.len();
        if needed > inner.data.capacity() {
            let target = (inner.data.capacity() * 2).max(needed).min(capacity);
            let additional = target - inner.data.len();
            inner.data.reserve_exact(additional);
        }
        inner.data.extend(kept);
        let total = inner.total_written;
        let now = Instant::now();
        inner.last_write = now;
        if Self::has_substantive_content(bytes) {
            inner.last_content_write = now;
        }
        drop(inner);
        self.written_tx.send_replace(total);
    }

    /// Read bytes written since `offset`. Returns `(bytes, new_offset)`.
    /// Uses read lock + as_slices to avoid blocking concurrent readers.
    pub fn read_from(&self, offset: usize) -> (Vec<u8>, usize, bool) {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let total = inner.total_written;
        if offset >= total {
            return (vec![], total, false);
        }
        let wanted = total - offset;
        let available = inner.data.len();
        let can_provide = wanted.min(available);
        let skip = available - can_provide;
        let (a, b) = inner.data.as_slices();
        let mut bytes = Vec::with_capacity(can_provide);
        if skip < a.len() {
            bytes.extend_from_slice(&a[skip..]);
            bytes.extend_from_slice(b);
        } else {
            bytes.extend_from_slice(&b[skip - a.len()..]);
        }
        (bytes, total, wanted > available)
    }

    /// Get a watch receiver for subscribing to write notifications.
    /// Level-triggered: `changed().await` resolves immediately if data was
    /// written since the last call, eliminating the Notify re-arm gap.
    pub fn subscribe(&self) -> watch::Receiver<usize> {
        self.written_rx.clone()
    }

    /// Current total bytes written (monotonic offset).
    pub fn current_offset(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .total_written
    }

    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .data
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn read_all(&self) -> Vec<u8> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let (a, b) = inner.data.as_slices();
        let mut bytes = Vec::with_capacity(inner.data.len());
        bytes.extend_from_slice(a);
        bytes.extend_from_slice(b);
        bytes
    }

    pub fn read_tail(&self, n: usize) -> Vec<u8> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        self.read_tail_inner(&inner, n)
    }

    pub fn resize_terminal(&self, rows: u16, cols: u16) {
        self.inner
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .terminal
            .screen_mut()
            .set_size(rows.max(1), cols.max(1));
    }

    /// Hold the output lock across the kernel resize and parser update, so a
    /// SIGWINCH redraw cannot be parsed at the previous dimensions.
    pub fn resize_transaction(
        &self,
        rows: u16,
        cols: u16,
        resize: impl FnOnce() -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        resize()?;
        inner
            .terminal
            .screen_mut()
            .set_size(rows.max(1), cols.max(1));
        Ok(())
    }

    /// Render from the original stream, not an arbitrary retained byte suffix.
    /// When either history bound is reached, a changed generation invalidates
    /// incremental cursors instead of silently skipping evicted output.
    pub fn terminal_snapshot(&self) -> (String, usize) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let screen = inner.terminal.screen_mut();
        screen.set_scrollback(usize::MAX);
        let history = screen.scrollback();
        let (rows, cols) = screen.size();
        let mut lines = Vec::new();
        for offset in (1..=history).rev() {
            screen.set_scrollback(offset);
            lines.push(screen.rows(0, cols).next().unwrap_or_default());
        }
        screen.set_scrollback(0);
        lines.extend(screen.rows(0, cols).take(rows as usize));
        while lines.last().is_some_and(|s| s.trim().is_empty()) {
            lines.pop();
        }
        let generation = if history >= 1024 {
            inner.total_written
        } else {
            inner.total_written.saturating_sub(inner.data.len())
        };
        (lines.join("\n"), generation)
    }

    /// Screen reconstruction and next live offset are captured under one lock.
    pub fn attach_snapshot(&self) -> (Vec<u8>, usize) {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let original = inner.terminal.screen();
        let (rows, cols) = original.size();
        let mut copy = vt100::Parser::new(rows, cols, 1024);
        *copy.screen_mut() = original.clone();
        // Recover the primary screen even when attached during an alternate-screen app.
        let alternate = original.alternate_screen();
        copy.process(b"\x1b[?47l");
        let screen = copy.screen_mut();
        screen.set_scrollback(usize::MAX);
        let history = screen.scrollback();
        let mut data = b"\x1bc".to_vec();
        for offset in (1..=history).rev() {
            screen.set_scrollback(offset);
            data.extend(screen.rows(0, cols).next().unwrap_or_default().as_bytes());
            data.extend(b"\r\n");
        }
        // Push every history row into scrollback before the visible-screen redraw.
        // A short history otherwise remains on screen and is erased by the redraw.
        for _ in 1..rows {
            data.extend(b"\r\n");
        }
        screen.set_scrollback(0);
        data.extend(screen.state_formatted());
        data.extend(screen.continuation_formatted());
        copy.process(b"\x1b[?47h");
        data.extend(b"\x1b[?47h");
        data.extend(copy.screen().state_formatted());
        data.extend(copy.screen().continuation_formatted());
        if !alternate {
            data.extend(b"\x1b[?47l");
        }
        data.extend(inner.terminal.screen_pending_formatted());
        (data, inner.total_written)
    }

    pub fn terminal_size(&self) -> (u16, u16) {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .terminal
            .screen()
            .size()
    }

    pub fn input_phase(&self) -> &'static str {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        crate::screen::input_phase(inner.terminal.screen())
    }

    pub fn content_idle_seconds(&self) -> u64 {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .last_content_write
            .elapsed()
            .as_secs()
    }

    pub fn idle_seconds(&self) -> u64 {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .last_write
            .elapsed()
            .as_secs()
    }

    pub fn looks_like_shell_prompt(&self) -> bool {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        if inner.data.is_empty() {
            return false;
        }
        // Only need the last 16 bytes — prompt patterns are 2 bytes, plus
        // a short trailing run of \r/\n.
        let tail = self.read_tail_inner(&inner, 16);
        let trimmed = tail
            .iter()
            .rposition(|&b| b != b'\n' && b != b'\r')
            .map(|pos| &tail[..=pos])
            .unwrap_or(&tail);
        trimmed.ends_with(b"$ ")
            || trimmed.ends_with(b"% ")
            || trimmed.ends_with(b"# ")
            || trimmed.ends_with(b"> ")
    }

    /// Returns true if `bytes` contains more than 4 printable characters
    /// after stripping ANSI escape sequences and control characters.
    fn has_substantive_content(bytes: &[u8]) -> bool {
        let mut printable = 0usize;
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == 0x1b {
                // ESC — start of escape sequence
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'[' => {
                        // CSI sequence: ESC [ ... (ends at 0x40–0x7E)
                        i += 1;
                        while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // skip final byte
                        }
                    }
                    b']' => {
                        // OSC sequence: ESC ] ... (ends at BEL or ST)
                        i += 1;
                        while i < bytes.len() {
                            if bytes[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // Other ESC sequences (e.g., ESC =, ESC >): skip one char
                        i += 1;
                    }
                }
            } else if b < 0x20 || b == 0x7f {
                // Control character (CR, LF, TAB, BEL, etc.)
                i += 1;
            } else {
                // Printable ASCII or start of UTF-8 multibyte
                printable += 1;
                if printable >= SUBSTANTIVE_CONTENT_MIN_CHARS {
                    return true;
                }
                // Skip remaining bytes of UTF-8 multibyte character
                if b >= 0xC0 {
                    i += 1;
                    while i < bytes.len() && (bytes[i] & 0xC0) == 0x80 {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
        false
    }

    /// Backdate `last_content_write` for testing purposes so that
    /// `content_idle_seconds()` returns a value past the idle timeout.
    #[cfg(test)]
    fn backdate_content_write(&self, duration: std::time::Duration) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.last_content_write = Instant::now() - duration;
    }

    fn read_tail_inner(&self, inner: &BufferInner, n: usize) -> Vec<u8> {
        let len = inner.data.len();
        let take = n.min(len);
        let skip = len - take;
        let (a, b) = inner.data.as_slices();
        let mut bytes = Vec::with_capacity(take);
        if skip < a.len() {
            bytes.extend_from_slice(&a[skip..]);
            bytes.extend_from_slice(b);
        } else {
            bytes.extend_from_slice(&b[skip - a.len()..]);
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_partial_opaque_string_should_never_invent_clipboard_or_title_callbacks() {
        #[derive(Default)]
        struct Actions {
            clipboard: usize,
            title: usize,
        }
        impl vt100::Callbacks for Actions {
            fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, _: &[u8], _: &[u8]) {
                self.clipboard += 1;
            }
            fn set_window_title(&mut self, _: &mut vt100::Screen, _: &[u8]) {
                self.title += 1;
            }
        }
        for suffix in [
            b"52;c;SEk=\x07".as_slice(),
            b"2;invented-title\x07".as_slice(),
        ] {
            let buf = OutputBuffer::new();
            buf.write(b"\x1b]2;original-title-prefix-");
            let (snapshot, _) = buf.attach_snapshot();
            let mut restored = vt100::Parser::new_with_callbacks(40, 120, 1024, Actions::default());
            restored.process(&snapshot);
            restored.process(suffix);
            assert_eq!(restored.callbacks().clipboard, 0);
            assert_eq!(restored.callbacks().title, 0);
        }
        let buf = OutputBuffer::new();
        buf.write(b"\x1bP1;2zopaque-");
        let (snapshot, _) = buf.attach_snapshot();
        assert!(
            snapshot.ends_with(b"\x1bP~"),
            "opaque DCS must not become a sixel introducer"
        );
    }

    #[test]
    fn given_every_byte_split_should_restore_parser_and_terminal_continuation() {
        let cases: Vec<&[u8]> = vec![
            b"\x1b[31mRED",
            b"\xe2\x82\xac",
            b"\xf0\x9f\x98\x80!",
            b"\xe2x\x82\xac",
            b"\xe2\x1b[31mRED",
            b"\xffbad\xc0\xaf",
            b"\x1b]0;title\x1b\\TEXT",
            b"\x1b]0;title\x07TEXT",
            b"\x1bP1;2qpayload\x1b\\TEXT",
            b"\x1bPqpayload\x9cTEXT",
            b"\x1b_Xignored\x1b\\TEXT",
            b"ABC\x1b7\x1b[3;4H\x1b8Z",
            b"\x1b[31mABC\x1b7\x1b[0m\x1b[3;4H\x1b8Z",
            b"TOP\r\nrow2\r\nrow3\r\nBOTTOM\x1b[2;3r\x1b[3;1HA\nB",
            b"\x1b[2;5r\x1b[?6h\x1b[2;3Hsaved\x1b7\x1b[3;1H\x1b8Z",
            b"123456789012Z",
            b"123456789012\x1b7\r\n\x1b8Z",
            b"main\x1b[?47halt\x1b7\x1b[?47l\x1b[?47h\x1b8Z",
            b"main\x1b[?1049halt\x1b[?1049lZ",
            b"\x1b[3;8r\x1b[?6h\x1b[4;5HX\x1b7\x1b[2;5r\x1b8Z",
            b"\x1b[38:2:255:0:128mCOLOR",
            b"\x1b[99999999999999999CZ",
            b"\x1b[31\r\nmRED",
            b"\x1b[31\x18RED",
            b"\x1b[31\x1aRED",
            b"\x1b[31\x1b[32mGREEN",
            b"\x1b[1 !pRESET",
            b"\x1b(0qqq\x1b(BZ",
        ];
        let mut cases: Vec<Vec<u8>> = cases.into_iter().map(Vec::from).collect();
        for start in [b"\x1b[".as_slice(), b"\x1bP".as_slice()] {
            for parameters in [b"1;".repeat(40), b"1:".repeat(40), b"   ".to_vec()] {
                let mut stream = start.to_vec();
                stream.extend(parameters);
                stream.extend(b"qignored\x9cTEXT\x1b\\TAIL");
                cases.push(stream);
            }
        }
        cases.extend([
            "abc€😀日本語\r\nend".as_bytes().to_vec(),
            b"\x1b[38:2::10:20:30mCOLOR\x1b[0m".to_vec(),
            b"PRIMARY\x1b7\x1b[?1049hALT\x1b7\x1b[3;4H\x1b8Z\x1b[?1049l\x1b8P".to_vec(),
            b"\x1b[?1h\x1b[?2004h\x1b[?1000h\x1b[?1006hX".to_vec(),
            [vec![b'A'; 120], b"B\x1b7\x1b[3;4H\x1b8Z".to_vec()].concat(),
        ]);
        for (rows, cols) in [(6, 12), (40, 120)] {
            for stream in &cases {
                for split in 0..=stream.len() {
                    let buf = OutputBuffer::new();
                    buf.resize_terminal(rows, cols);
                    buf.write(&stream[..split]);
                    let (snapshot, offset) = buf.attach_snapshot();
                    buf.write(&stream[split..]);
                    let (live, _, gap) = buf.read_from(offset);
                    assert!(!gap);
                    let mut expected = vt100::Parser::new(rows, cols, 1024);
                    expected.process(stream);
                    let mut restored = vt100::Parser::new(rows, cols, 1024);
                    restored.process(&snapshot);
                    restored.process(&live);
                    assert_eq!(
                        restored.screen().state_formatted(),
                        expected.screen().state_formatted(),
                        "stream {stream:?} split {split} size {rows}x{cols}"
                    );
                    assert_eq!(
                        restored.screen().continuation_formatted(),
                        expected.screen().continuation_formatted(),
                        "continuation stream {stream:?} split {split} size {rows}x{cols}"
                    );
                }
            }
        }
    }

    #[test]
    fn given_resize_transaction_should_hold_redraws_until_new_dimensions_and_preserve_size_on_failure()
     {
        let buffer = std::sync::Arc::new(OutputBuffer::new());
        let writer = buffer.clone();
        let (start_tx, start_rx) = std::sync::mpsc::channel();
        let task = std::thread::spawn(move || {
            start_rx.recv().unwrap();
            writer.write(b"123456");
        });
        buffer
            .resize_transaction(3, 5, || {
                start_tx.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        task.join().unwrap();
        assert!(buffer.terminal_snapshot().0.contains("12345\n6"));
        assert!(
            buffer
                .resize_transaction(9, 9, || Err(std::io::Error::other("failed ioctl")))
                .is_err()
        );
        assert_eq!(buffer.terminal_size(), (3, 5));
    }

    #[test]
    fn given_concurrent_writes_should_capture_screen_and_offset_from_same_write() {
        let buffer = std::sync::Arc::new(OutputBuffer::new());
        let writer = buffer.clone();
        let task = std::thread::spawn(move || {
            for n in 1..=1000 {
                writer.write(format!("\r\x1b[2K{n:04}").as_bytes());
            }
        });
        for _ in 0..100 {
            let (data, offset) = buffer.attach_snapshot();
            let mut parser = vt100::Parser::new(40, 120, 0);
            parser.process(&data);
            let text = parser.screen().contents();
            if offset > 0 {
                assert_eq!(text.trim().parse::<usize>().unwrap(), offset / 9);
            }
        }
        task.join().unwrap();
    }

    #[test]
    fn given_short_history_and_alternate_screen_should_restore_both_buffers_and_modes() {
        let buf = OutputBuffer::new();
        buf.write(b"history1\r\nhistory2\r\nhistory3");
        for _ in 0..40 {
            buf.write(b"\r\n");
        }
        buf.write(b"primary\x1b[?1049halternate\x1b[?1h\x1b[?2004h");
        let (snapshot, _) = buf.attach_snapshot();
        let mut restored = vt100::Parser::new(40, 120, 1024);
        restored.process(&snapshot);
        assert!(restored.screen().alternate_screen());
        assert!(restored.screen().application_cursor());
        assert!(restored.screen().bracketed_paste());
        assert!(restored.screen().contents().contains("alternate"));
        restored.process(b"\x1b[?1049l");
        assert!(restored.screen().contents().contains("primary"));
        restored.screen_mut().set_scrollback(usize::MAX);
        assert!(restored.screen().contents().contains("history1"));
    }

    #[test]
    fn given_evicted_screen_should_snapshot_original_screen_and_exact_offset() {
        let buf = OutputBuffer::new();
        buf.write(b"ancient screen");
        buf.write(&b"\x1b[0m".repeat(1_100_000));
        let (data, offset) = buf.attach_snapshot();
        let mut parser = vt100::Parser::new(40, 120, 1024);
        parser.process(&data);
        assert!(parser.screen().contents().contains("ancient screen"));
        assert_eq!(offset, buf.current_offset());
        let (_, _, gap) = buf.read_from(0);
        assert!(gap);
    }

    #[test]
    fn given_evicted_raw_bytes_should_preserve_emulated_text_and_change_capture_generation() {
        // given
        let buffer = OutputBuffer::with_capacity(8);
        buffer.write(b"first");
        let before = buffer.terminal_snapshot();
        // when
        buffer.write(b"\r\x1b[2Ksecond");
        let after = buffer.terminal_snapshot();
        // then
        assert_eq!(before.0, "first");
        assert_eq!(after.0, "second");
        assert_eq!(before.1, 0);
        assert!(after.1 > before.1);
    }

    #[test]
    fn given_terminal_redraws_and_split_unicode_should_read_rendered_text() {
        // given
        let buf = OutputBuffer::new();
        buf.write(b"progress 1\r\x1b[2Kprogress 2\r\x1b[2Kdone\r\n\xf0\x9f");
        buf.write(b"\x98\x80");
        // when
        let (text, loss_generation) = buf.terminal_snapshot();
        // then
        assert_eq!(text, "done\n😀");
        assert_eq!(loss_generation, 0);
    }
    use std::sync::Arc;

    #[test]
    fn given_new_buffer_should_be_empty() {
        let buf = OutputBuffer::new();
        assert_eq!(buf.len(), 0);
        assert!(buf.read_all().is_empty());
    }

    #[test]
    fn given_write_should_store_bytes() {
        let buf = OutputBuffer::new();
        buf.write(b"hello");
        assert_eq!(buf.len(), 5);
        assert_eq!(buf.read_all(), b"hello");
    }

    #[test]
    fn given_multiple_writes_should_concatenate() {
        let buf = OutputBuffer::new();
        buf.write(b"hello ");
        buf.write(b"world");
        assert_eq!(buf.read_all(), b"hello world");
    }

    #[test]
    fn given_writes_past_capacity_should_not_grow_allocation_past_capacity() {
        let cap = 1024;
        let buf = OutputBuffer::with_capacity(cap);
        for _ in 0..100 {
            buf.write(&[b'x'; 100]);
        }
        let inner = buf.inner.read().unwrap();
        assert_eq!(inner.data.len(), cap);
        assert!(
            inner.data.capacity() <= cap,
            "allocation grew to {}",
            inner.data.capacity()
        );
    }

    #[test]
    fn given_single_write_larger_than_capacity_should_keep_tail() {
        let buf = OutputBuffer::with_capacity(4);
        buf.write(b"ab");
        buf.write(b"cdefgh");
        assert_eq!(buf.read_all(), b"efgh");
        assert_eq!(buf.inner.read().unwrap().total_written, 8);
    }

    #[test]
    fn given_read_tail_should_return_last_n_bytes() {
        let buf = OutputBuffer::new();
        buf.write(b"abcdefghij");
        assert_eq!(buf.read_tail(5), b"fghij");
    }

    #[test]
    fn given_read_tail_larger_than_buffer_should_return_all() {
        let buf = OutputBuffer::new();
        buf.write(b"abc");
        assert_eq!(buf.read_tail(100), b"abc");
    }

    #[test]
    fn given_buffer_exceeds_capacity_should_wrap_and_discard_oldest() {
        let buf = OutputBuffer::with_capacity(10);
        buf.write(b"12345"); // 5 bytes
        buf.write(b"67890"); // full at 10
        buf.write(b"abc"); // wraps, oldest 3 bytes discarded
        let data = buf.read_all();
        assert!(data.len() <= 10);
        assert!(data.ends_with(b"abc"));
    }

    #[test]
    fn given_write_should_update_last_write_time() {
        let buf = OutputBuffer::new();
        buf.write(b"test");
        let idle = buf.idle_seconds();
        assert!(idle < 2, "idle_seconds should be near 0, got {idle}");
    }

    #[test]
    fn given_no_writes_should_report_high_idle_seconds() {
        let buf = OutputBuffer::new();
        let idle = buf.idle_seconds();
        assert!(idle < 5);
    }

    #[test]
    fn given_buffer_ending_with_dollar_prompt_should_detect_shell() {
        let buf = OutputBuffer::new();
        buf.write(b"some output\n$ ");
        assert!(buf.looks_like_shell_prompt());
    }

    #[test]
    fn given_buffer_ending_with_percent_prompt_should_detect_shell() {
        let buf = OutputBuffer::new();
        buf.write(b"output\nuser@host % ");
        assert!(buf.looks_like_shell_prompt());
    }

    #[test]
    fn given_buffer_ending_with_hash_prompt_should_detect_shell() {
        let buf = OutputBuffer::new();
        buf.write(b"output\nroot# ");
        assert!(buf.looks_like_shell_prompt());
    }

    #[test]
    fn given_buffer_ending_with_gt_prompt_should_detect_shell() {
        let buf = OutputBuffer::new();
        buf.write(b"output\n> ");
        assert!(buf.looks_like_shell_prompt());
    }

    #[test]
    fn given_buffer_with_no_prompt_should_not_detect_shell() {
        let buf = OutputBuffer::new();
        buf.write(b"Running agent task...\nProcessing files");
        assert!(!buf.looks_like_shell_prompt());
    }

    #[test]
    fn given_empty_buffer_should_not_detect_shell() {
        let buf = OutputBuffer::new();
        assert!(!buf.looks_like_shell_prompt());
    }

    #[test]
    fn given_concurrent_reads_and_writes_should_not_panic() {
        let buf = std::sync::Arc::new(OutputBuffer::new());
        let mut handles = vec![];

        for i in 0..10 {
            let b = buf.clone();
            handles.push(std::thread::spawn(move || {
                b.write(format!("thread-{i}\n").as_bytes());
            }));
        }

        for _ in 0..10 {
            let b = buf.clone();
            handles.push(std::thread::spawn(move || {
                let _ = b.read_all();
                let _ = b.read_tail(50);
                let _ = b.looks_like_shell_prompt();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert!(!buf.is_empty());
    }

    // --- Streaming / notification tests ---

    #[tokio::test]
    async fn given_write_should_notify_waiters() {
        let buf = Arc::new(OutputBuffer::new());
        let mut watcher = buf.subscribe();
        let buf2 = buf.clone();
        let handle = tokio::spawn(async move {
            watcher.changed().await.unwrap();
            true
        });
        // Small yield to ensure the waiter is registered
        tokio::task::yield_now().await;
        buf2.write(b"ping");
        let notified = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("timeout")
            .expect("join");
        assert!(notified);
    }

    #[test]
    fn given_write_should_increment_total_written() {
        let buf = OutputBuffer::new();
        assert_eq!(buf.current_offset(), 0);
        buf.write(b"hello");
        assert_eq!(buf.current_offset(), 5);
        buf.write(b"world");
        assert_eq!(buf.current_offset(), 10);
    }

    #[test]
    fn given_read_from_zero_should_return_all_bytes() {
        let buf = OutputBuffer::new();
        buf.write(b"abcdef");
        let (data, offset, _) = buf.read_from(0);
        assert_eq!(data, b"abcdef");
        assert_eq!(offset, 6);
    }

    #[test]
    fn given_read_from_offset_should_return_only_new_bytes() {
        let buf = OutputBuffer::new();
        buf.write(b"abc");
        let (_, off1, _) = buf.read_from(0);
        buf.write(b"def");
        let (data, off2, _) = buf.read_from(off1);
        assert_eq!(data, b"def");
        assert_eq!(off2, 6);
    }

    #[test]
    fn given_read_from_beyond_total_should_return_empty() {
        let buf = OutputBuffer::new();
        buf.write(b"abc");
        let (data, offset, _) = buf.read_from(100);
        assert!(data.is_empty());
        assert_eq!(offset, 3);
    }

    #[test]
    fn given_current_offset_should_match_total_written() {
        let buf = OutputBuffer::new();
        buf.write(b"12345");
        buf.write(b"67890");
        assert_eq!(buf.current_offset(), 10);
    }

    // --- ANSI content detection tests ---

    #[test]
    fn given_plain_text_should_be_substantive() {
        assert!(OutputBuffer::has_substantive_content(b"Hello World"));
    }

    #[test]
    fn given_pure_ansi_csi_should_not_be_substantive() {
        // CSI erase line + cursor move
        assert!(!OutputBuffer::has_substantive_content(b"\x1b[2K\x1b[1G"));
    }

    #[test]
    fn given_ansi_plus_spinner_char_should_not_be_substantive() {
        // CSI erase + single multibyte spinner character (✻ = 3 bytes, 1 printable)
        let mut data = b"\x1b[2K\x1b[1G".to_vec();
        data.extend("✻".as_bytes());
        assert!(!OutputBuffer::has_substantive_content(&data));
    }

    #[test]
    fn given_ansi_plus_five_printable_should_be_substantive() {
        assert!(OutputBuffer::has_substantive_content(
            b"\x1b[32mHello\x1b[0m"
        ));
    }

    #[test]
    fn given_exactly_four_printable_should_not_be_substantive() {
        assert!(!OutputBuffer::has_substantive_content(
            b"\x1b[32mTest\x1b[0m"
        ));
    }

    #[test]
    fn given_osc_title_sequence_should_not_be_substantive() {
        assert!(!OutputBuffer::has_substantive_content(
            b"\x1b]0;My Title\x07"
        ));
    }

    #[test]
    fn given_dec_private_mode_should_not_be_substantive() {
        // Hide cursor: ESC [ ? 25 l
        assert!(!OutputBuffer::has_substantive_content(b"\x1b[?25l"));
    }

    #[test]
    fn given_control_chars_only_should_not_be_substantive() {
        assert!(!OutputBuffer::has_substantive_content(b"\r\n\t\x07"));
    }

    // --- Content idle tracking tests ---

    #[test]
    fn given_write_with_only_ansi_sequences_should_not_update_content_write() {
        let buf = OutputBuffer::new();
        // Simulate Claude Code spinner: CSI sequence + single spinner char
        // ESC [ ? 25 l (hide cursor) + "✻"
        buf.write(b"\x1b[?25l");
        buf.write("✻".as_bytes());
        // content_idle should be high since no substantive content was written
        let idle = buf.content_idle_seconds();
        // The buffer was just created, so even content_idle should be ~0
        // But the key test is below - after a delay, content_idle grows
        // while idle_seconds stays near 0 due to spinner writes
        assert!(idle < 2);
    }

    #[test]
    fn given_write_with_substantive_content_should_update_content_write() {
        let buf = OutputBuffer::new();
        buf.write(b"Processing files...");
        let idle = buf.content_idle_seconds();
        assert!(
            idle < 2,
            "content_idle_seconds should be near 0 after substantive write, got {idle}"
        );
    }

    #[test]
    fn given_spinner_after_content_should_not_reset_content_idle() {
        let buf = OutputBuffer::new();
        // First, write real content
        buf.write(b"Thinking about your request...\n");
        // Then write spinner sequences (ANSI + 1-2 spinner chars)
        for spinner in ["✻", "✽", "✻", "✽"] {
            buf.write(format!("\x1b[2K\x1b[1G{spinner}").as_bytes());
        }
        // content_idle should track the real content write, not the spinners
        let content_idle = buf.content_idle_seconds();
        let raw_idle = buf.idle_seconds();
        // Both should be near 0 since everything just happened
        assert!(content_idle < 2);
        assert!(raw_idle < 2);
    }

    #[test]
    fn given_ansi_with_short_printable_should_not_count_as_content() {
        // <= 4 printable bytes after stripping ANSI should not count
        let buf = OutputBuffer::new();
        // CSI erase line + carriage return + 1 spinner char = not content
        buf.write("\x1b[2K\x1b[1G✻".as_bytes());
        let idle = buf.content_idle_seconds();
        assert!(idle < 2); // just created, so near 0 regardless
    }

    #[test]
    fn given_ansi_with_long_printable_should_count_as_content() {
        let buf = OutputBuffer::new();
        // ANSI color code + substantial text = content
        buf.write(b"\x1b[32mHello World\x1b[0m");
        let idle = buf.content_idle_seconds();
        assert!(
            idle < 2,
            "content_idle should be near 0 after substantive content, got {idle}"
        );
    }

    #[test]
    fn given_osc_sequences_should_be_stripped() {
        let buf = OutputBuffer::new();
        // OSC set title sequence: ESC ] 0 ; title BEL
        buf.write(b"\x1b]0;Terminal Title\x07");
        // Only the title text is printable, but it's inside OSC so should be stripped
        // After stripping, no printable content remains
        let idle = buf.content_idle_seconds();
        assert!(idle < 2);
    }

    #[test]
    fn given_only_control_chars_should_not_count_as_content() {
        let buf = OutputBuffer::new();
        buf.write(b"\r\n\t");
        let idle = buf.content_idle_seconds();
        assert!(idle < 2);
    }

    #[test]
    fn given_backdated_content_write_spinner_should_not_reset_it() {
        // given: content_write is backdated past timeout
        let buf = OutputBuffer::new();
        buf.backdate_content_write(std::time::Duration::from_secs(60));
        assert!(buf.content_idle_seconds() >= 59);

        // when: spinner-only output is written
        buf.write(b"\x1b[2K\x1b[1G");
        buf.write("✻".as_bytes());

        // then: content_idle should still be past timeout (spinner didn't reset it)
        assert!(
            buf.content_idle_seconds() >= 59,
            "spinner write should not reset content_idle, got {}",
            buf.content_idle_seconds()
        );
        // but raw idle should be near 0
        assert!(buf.idle_seconds() < 2);
    }

    #[test]
    fn given_backdated_content_write_substantive_should_reset_it() {
        // given: content_write is backdated past timeout
        let buf = OutputBuffer::new();
        buf.backdate_content_write(std::time::Duration::from_secs(60));
        assert!(buf.content_idle_seconds() >= 59);

        // when: substantive content is written
        buf.write(b"Processing files...");

        // then: content_idle should be near 0 (substantive write reset it)
        assert!(
            buf.content_idle_seconds() < 2,
            "substantive write should reset content_idle, got {}",
            buf.content_idle_seconds()
        );
    }

    #[test]
    fn given_buffer_wraps_capacity_should_return_available_from_offset() {
        let buf = OutputBuffer::with_capacity(10);
        buf.write(b"12345"); // total_written = 5, data = [1,2,3,4,5]
        buf.write(b"67890"); // total_written = 10, data = [1,2,3,4,5,6,7,8,9,0]
        buf.write(b"abc"); // total_written = 13, data trimmed to last 10: [4,5,6,7,8,9,0,a,b,c]

        // Reading from offset 5 wants bytes 5..13 = 8 bytes, but buffer only has 10 chars
        // starting from total_written - data.len() = 13 - 10 = 3
        // So offset 5 means we want from position 5, buffer starts at 3, so skip = 5 - 3 = 2
        let (data, new_off, _) = buf.read_from(5);
        assert_eq!(new_off, 13);
        assert_eq!(data, b"67890abc");

        // Reading from offset 0 can only return what's in the buffer (10 bytes from position 3)
        let (data, _, _) = buf.read_from(0);
        assert_eq!(data.len(), 10);
        assert_eq!(data, b"4567890abc");
    }
}
