use crate::output_buffer::OutputBuffer;
use pu_core::types::AgentStatus;

const IDLE_TIMEOUT_SECS: u64 = 30;

pub fn effective_status(exit_code: Option<i32>, buffer: &OutputBuffer) -> AgentStatus {
    match exit_code {
        Some(0) => AgentStatus::Completed,
        Some(_) => AgentStatus::Failed,
        None => {
            if buffer.looks_like_shell_prompt() || buffer.idle_seconds() > IDLE_TIMEOUT_SECS {
                AgentStatus::Idle
            } else {
                AgentStatus::Running
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
    fn given_exit_code_zero_should_return_completed() {
        let buf = OutputBuffer::new();
        let status = effective_status(Some(0), &buf);
        assert_eq!(status, AgentStatus::Completed);
    }

    #[test]
    fn given_exit_code_nonzero_should_return_failed() {
        let buf = OutputBuffer::new();
        let status = effective_status(Some(1), &buf);
        assert_eq!(status, AgentStatus::Failed);
    }

    #[test]
    fn given_exit_code_signal_should_return_failed() {
        let buf = OutputBuffer::new();
        // 128 + signal = killed by signal
        let status = effective_status(Some(137), &buf);
        assert_eq!(status, AgentStatus::Failed);
    }

    #[test]
    fn given_running_with_shell_prompt_should_return_idle() {
        let buf = OutputBuffer::new();
        buf.write(b"agent done\nuser@host $ ");
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Idle);
    }

    #[test]
    fn given_running_with_recent_output_should_return_running() {
        let buf = OutputBuffer::new();
        buf.write(b"Processing files...");
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Running);
    }

    #[test]
    fn given_running_with_empty_buffer_should_return_running() {
        let buf = OutputBuffer::new();
        let status = effective_status(None, &buf);
        assert_eq!(status, AgentStatus::Running);
    }

    // Note: idle timeout (30s) is hard to test in unit tests without time manipulation.
    // That path is covered by integration tests.
}
