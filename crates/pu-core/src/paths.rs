use std::path::{Path, PathBuf};

pub fn pu_dir(project_root: &Path) -> PathBuf {
    project_root.join(".pu")
}

pub fn manifest_path(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("manifest.json")
}

pub fn config_path(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("config.yaml")
}

pub fn prompts_dir(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("prompts")
}

pub fn prompt_file(project_root: &Path, agent_id: &str) -> PathBuf {
    prompts_dir(project_root).join(format!("{agent_id}.md"))
}

pub fn global_pu_dir() -> Result<PathBuf, std::io::Error> {
    Ok(home_dir()?.join(".pu"))
}

pub fn daemon_pid_path() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("daemon.pid"))
}

pub fn daemon_socket_path() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("daemon.sock"))
}

pub fn daemon_log_path() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("daemon.log"))
}

pub fn worktree_path(project_root: &Path, worktree_id: &str) -> PathBuf {
    pu_dir(project_root).join("worktrees").join(worktree_id)
}

pub fn templates_dir(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("templates")
}

pub fn global_templates_dir() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("templates"))
}

pub fn agents_dir(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("agents")
}

pub fn global_agents_dir() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("agents"))
}

pub fn swarms_dir(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("swarms")
}

pub fn global_swarms_dir() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("swarms"))
}

pub fn schedules_dir(project_root: &Path) -> PathBuf {
    pu_dir(project_root).join("schedules")
}

pub fn global_schedules_dir() -> Result<PathBuf, std::io::Error> {
    Ok(global_pu_dir()?.join("schedules"))
}

fn home_dir() -> Result<PathBuf, std::io::Error> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "$HOME not set"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn given_project_root_should_build_pu_dir() {
        let root = Path::new("/projects/myapp");
        assert_eq!(pu_dir(root), PathBuf::from("/projects/myapp/.pu"));
    }

    #[test]
    fn given_project_root_should_build_manifest_path() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            manifest_path(root),
            PathBuf::from("/projects/myapp/.pu/manifest.json")
        );
    }

    #[test]
    fn given_project_root_should_build_config_path() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            config_path(root),
            PathBuf::from("/projects/myapp/.pu/config.yaml")
        );
    }

    #[test]
    fn given_project_root_should_build_prompt_file_path() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            prompt_file(root, "ag-abc"),
            PathBuf::from("/projects/myapp/.pu/prompts/ag-abc.md")
        );
    }

    #[test]
    fn given_project_root_should_build_worktree_path() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            worktree_path(root, "wt-xyz"),
            PathBuf::from("/projects/myapp/.pu/worktrees/wt-xyz")
        );
    }

    #[test]
    fn given_project_root_should_build_templates_dir() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            templates_dir(root),
            PathBuf::from("/projects/myapp/.pu/templates")
        );
    }

    #[test]
    fn given_global_templates_dir_should_live_under_home_pu() {
        let path = global_templates_dir().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/templates"),
            "unexpected path: {path:?}"
        );
    }

    #[test]
    fn given_daemon_pid_path_should_live_under_home_pu() {
        let path = daemon_pid_path().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/daemon.pid"),
            "unexpected path: {path:?}"
        );
    }

    #[test]
    fn given_daemon_socket_path_should_live_under_home_pu() {
        let path = daemon_socket_path().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/daemon.sock"),
            "unexpected path: {path:?}"
        );
    }

    #[test]
    fn given_daemon_log_path_should_live_under_home_pu() {
        let path = daemon_log_path().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/daemon.log"),
            "unexpected path: {path:?}"
        );
    }

    #[test]
    fn given_project_root_should_build_agents_dir() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            agents_dir(root),
            PathBuf::from("/projects/myapp/.pu/agents")
        );
    }

    #[test]
    fn given_global_agents_dir_should_live_under_home_pu() {
        let path = global_agents_dir().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/agents"),
            "unexpected path: {path:?}"
        );
    }

    #[test]
    fn given_project_root_should_build_swarms_dir() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            swarms_dir(root),
            PathBuf::from("/projects/myapp/.pu/swarms")
        );
    }

    #[test]
    fn given_global_swarms_dir_should_live_under_home_pu() {
        let path = global_swarms_dir().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/swarms"),
            "unexpected path: {path:?}"
        );
    }

    #[test]
    fn given_project_root_should_build_schedules_dir() {
        let root = Path::new("/projects/myapp");
        assert_eq!(
            schedules_dir(root),
            PathBuf::from("/projects/myapp/.pu/schedules")
        );
    }

    #[test]
    fn given_global_schedules_dir_should_live_under_home_pu() {
        let path = global_schedules_dir().unwrap();
        assert!(
            path.to_string_lossy().contains(".pu/schedules"),
            "unexpected path: {path:?}"
        );
    }
}
