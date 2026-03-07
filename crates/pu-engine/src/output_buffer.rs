use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::Instant;

use tokio::sync::watch;

const DEFAULT_CAPACITY: usize = 4 * 1024 * 1024; // 4MB

pub struct OutputBuffer {
    inner: RwLock<BufferInner>,
    written_tx: watch::Sender<usize>,
    written_rx: watch::Receiver<usize>,
}

struct BufferInner {
    data: VecDeque<u8>,
    capacity: usize,
    last_write: Instant,
    total_written: usize,
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
        Self {
            inner: RwLock::new(BufferInner {
                data: VecDeque::new(),
                capacity,
                last_write: Instant::now(),
                total_written: 0,
            }),
            written_tx,
            written_rx,
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.data.extend(bytes);
        inner.total_written += bytes.len();
        // Trim from front if over capacity — O(excess) with VecDeque
        if inner.data.len() > inner.capacity {
            let excess = inner.data.len() - inner.capacity;
            inner.data.drain(..excess);
        }
        let total = inner.total_written;
        inner.last_write = Instant::now();
        drop(inner);
        self.written_tx.send_replace(total);
    }

    /// Read bytes written since `offset`. Returns `(bytes, new_offset)`.
    /// Uses read lock + as_slices to avoid blocking concurrent readers.
    pub fn read_from(&self, offset: usize) -> (Vec<u8>, usize) {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        let total = inner.total_written;
        if offset >= total {
            return (vec![], total);
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
        (bytes, total)
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
        let (data, offset) = buf.read_from(0);
        assert_eq!(data, b"abcdef");
        assert_eq!(offset, 6);
    }

    #[test]
    fn given_read_from_offset_should_return_only_new_bytes() {
        let buf = OutputBuffer::new();
        buf.write(b"abc");
        let (_, off1) = buf.read_from(0);
        buf.write(b"def");
        let (data, off2) = buf.read_from(off1);
        assert_eq!(data, b"def");
        assert_eq!(off2, 6);
    }

    #[test]
    fn given_read_from_beyond_total_should_return_empty() {
        let buf = OutputBuffer::new();
        buf.write(b"abc");
        let (data, offset) = buf.read_from(100);
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

    #[test]
    fn given_buffer_wraps_capacity_should_return_available_from_offset() {
        let buf = OutputBuffer::with_capacity(10);
        buf.write(b"12345"); // total_written = 5, data = [1,2,3,4,5]
        buf.write(b"67890"); // total_written = 10, data = [1,2,3,4,5,6,7,8,9,0]
        buf.write(b"abc"); // total_written = 13, data trimmed to last 10: [4,5,6,7,8,9,0,a,b,c]

        // Reading from offset 5 wants bytes 5..13 = 8 bytes, but buffer only has 10 chars
        // starting from total_written - data.len() = 13 - 10 = 3
        // So offset 5 means we want from position 5, buffer starts at 3, so skip = 5 - 3 = 2
        let (data, new_off) = buf.read_from(5);
        assert_eq!(new_off, 13);
        assert_eq!(data, b"67890abc");

        // Reading from offset 0 can only return what's in the buffer (10 bytes from position 3)
        let (data, _) = buf.read_from(0);
        assert_eq!(data.len(), 10);
        assert_eq!(data, b"4567890abc");
    }
}
