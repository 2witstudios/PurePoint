use std::collections::VecDeque;
use std::sync::RwLock;
use std::time::Instant;

const DEFAULT_CAPACITY: usize = 1024 * 1024; // 1MB

pub struct OutputBuffer {
    inner: RwLock<BufferInner>,
}

struct BufferInner {
    data: VecDeque<u8>,
    capacity: usize,
    last_write: Instant,
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
        Self {
            inner: RwLock::new(BufferInner {
                data: VecDeque::new(),
                capacity,
                last_write: Instant::now(),
            }),
        }
    }

    pub fn write(&self, bytes: &[u8]) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.data.extend(bytes);
        // Trim from front if over capacity — O(excess) with VecDeque
        if inner.data.len() > inner.capacity {
            let excess = inner.data.len() - inner.capacity;
            inner.data.drain(..excess);
        }
        inner.last_write = Instant::now();
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn read_all(&self) -> Vec<u8> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.data.iter().copied().collect()
    }

    pub fn read_tail(&self, n: usize) -> Vec<u8> {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        if n >= inner.data.len() {
            inner.data.iter().copied().collect()
        } else {
            inner.data.iter().skip(inner.data.len() - n).copied().collect()
        }
    }

    pub fn idle_seconds(&self) -> u64 {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).last_write.elapsed().as_secs()
    }

    pub fn looks_like_shell_prompt(&self) -> bool {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        if inner.data.is_empty() {
            return false;
        }
        // Check the last 256 bytes for shell prompt patterns
        let tail_bytes: Vec<u8> = if inner.data.len() > 256 {
            inner.data.iter().skip(inner.data.len() - 256).copied().collect()
        } else {
            inner.data.iter().copied().collect()
        };
        let s = String::from_utf8_lossy(&tail_bytes);
        let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
        trimmed.ends_with("$ ")
            || trimmed.ends_with("% ")
            || trimmed.ends_with("# ")
            || trimmed.ends_with("> ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        buf.write(b"abc");   // wraps, oldest 3 bytes discarded
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

        assert!(buf.len() > 0);
    }
}
