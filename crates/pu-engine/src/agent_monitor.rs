use crate::output_buffer::OutputBuffer;
use pu_core::types::AgentStatus;

const IDLE_TIMEOUT_SECS: u64 = 30;

pub fn effective_status(exit_code: Option<i32>, buffer: &OutputBuffer) -> AgentStatus {
    match exit_code {
        Some(_) => AgentStatus::Broken,
        None => {
            if buffer.looks_like_shell_prompt() || buffer.content_idle_seconds() > IDLE_TIMEOUT_SECS
            {
                AgentStatus::Waiting
            } else {
                AgentStatus::Streaming
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_buffer::OutputBuffer;
    use pu_core::types::AgentStatus;

    #[test]
    fn given_exit_code_zero_should_return_broken() {
        let buf = OutputBuffer::new();
        let status = effective_status(Some(0), &buf);
        assert_eq!(status, AgentStatus::Broken);
    }

    #[test]
    fn given_exit_code_nonzero_should_return_broken() {
        let buf = OutputBuffer::new();
        let status = effective_status(Some(1), &buf);
        assert_eq!(status, AgentStatus::Broken);
    }

    #[test]
    fn given_exit_code_signal_should_return_broken() {
        let buf = OutputBuffer::new();
        let status = effective_status(Some(137), &buf);
        assert_eq!(status, AgentStatus::Broken);
    }

    #[test]
    fn given_running_with_shell_prompt_should_return_waiting() {
        let buf = OutputBuffer::new();
        buf.write(b"agent done\nuser@host $ ");
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Waiting);
    }

    #[test]
    fn given_running_with_recent_output_should_return_streaming() {
        let buf = OutputBuffer::new();
        buf.write(b"Processing files...");
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Streaming);
    }

    #[test]
    fn given_running_with_empty_buffer_should_return_streaming() {
        let buf = OutputBuffer::new();
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Streaming);
    }

    #[test]
    fn given_spinner_output_should_use_content_idle_not_raw_idle() {
        // Simulate: real content was written, then only spinner output
        // The effective_status should use content_idle_seconds, not idle_seconds
        let buf = OutputBuffer::new();
        buf.write(b"Working on your task...\n");
        // Write spinner-only output (ANSI + short printable)
        buf.write(b"\x1b[2K\x1b[1G");
        buf.write("✻".as_bytes());
        // Even though idle_seconds() is near 0 (due to spinner write),
        // content_idle_seconds() should also be near 0 (recent real content)
        // So status should be Streaming
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Streaming);
    }
}
