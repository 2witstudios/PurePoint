use pu_core::types::AgentStatus;

pub fn effective_status(exit_code: Option<i32>) -> AgentStatus {
    match exit_code {
        Some(_) => AgentStatus::Broken,
        None => AgentStatus::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pu_core::types::AgentStatus;

    #[test]
    fn given_exit_code_zero_should_return_broken() {
        let status = effective_status(Some(0));
        assert_eq!(status, AgentStatus::Broken);
    }

    #[test]
    fn given_exit_code_nonzero_should_return_broken() {
        let status = effective_status(Some(1));
        assert_eq!(status, AgentStatus::Broken);
    }

    #[test]
    fn given_exit_code_signal_should_return_broken() {
        let status = effective_status(Some(137));
        assert_eq!(status, AgentStatus::Broken);
    }

    #[test]
    fn given_no_exit_code_should_return_running() {
        let status = effective_status(None);
        assert_eq!(status, AgentStatus::Running);
    }
}
