//! Worker-local source evidence, independent of bridge authority and execution.
//!
//! Only inspects an existing repository. Never provisions, fetches, creates worktrees,
//! synchronizes dirty files, or proves checkout/dependency readiness.

use std::ffi::OsString;
use std::os::unix::{ffi::OsStringExt, fs::MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::oneshot;

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_LIMIT: usize = 16 * 1024;

/// Evidence that a precise commit object was locally accessible at verification time.
///
/// Private fields and no serde implementation deliberately prevent wire input from
/// constructing trusted evidence. This is not authorization, an immutable snapshot,
/// an integrity/fsck proof, or a promise of reproducibility. Locally configured object
/// alternates may supply objects. Missing trees, blobs, submodules, ignored assets,
/// dependencies and toolchains remain the caller's readiness obligation.
///
/// The caller must resolve the export locally, hold its authority barrier, check exact
/// export identity and current run lookup, and revalidate source/authority before any
/// worktree or launch effects. Filesystem boundary checks are sampled, not atomic;
/// same-account repository mutation remains outside this evidence's trust boundary.
pub struct VerifiedSource {
    canonical_root: PathBuf,
    exact_commit: String,
}

impl VerifiedSource {
    /// Receiver-selected canonical worktree root, equal to Git's actual top-level.
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }
    /// Full lowercase object ID of the actual locally present commit object.
    pub fn exact_commit(&self) -> &str {
        &self.exact_commit
    }
}

/// Path/content-free errors suitable for a future remote protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SourceVerificationError {
    #[error("source commit must be a full lowercase hexadecimal object ID")]
    InvalidCommit,
    #[error("source root must be the canonical Git worktree top-level")]
    InvalidRoot,
    #[error("source repository metadata changed during verification")]
    RepositoryChanged,
    #[error("exact source commit object is not locally available")]
    CommitUnavailable,
    #[error("source Git inspection failed")]
    GitFailed,
    #[error("source Git inspection exceeded its output limit")]
    OutputLimit,
    #[error("source Git inspection timed out")]
    Timeout,
}

type Result<T> = std::result::Result<T, SourceVerificationError>;

/// Verify a receiver-selected canonical root and an exact existing commit only.
///
/// Branches/tags/revision syntax, abbreviations, uppercase and option-shaped inputs
/// reject. Git must support `--no-lazy-fetch`; unsupported versions fail closed.
/// Each Git child has a five-second deadline and 16 KiB per-stream output limit.
/// Cancellation signals its supervisor to kill and reap the isolated child; this
/// requires the Tokio runtime to remain alive for cleanup. Runtime/process teardown
/// retains `kill_on_drop` as a fallback, not a daemon-crash supervision guarantee.
pub async fn verify_existing_commit(
    receiver_selected_canonical_project_root: &Path,
    exact_commit: &str,
) -> Result<VerifiedSource> {
    if !matches!(exact_commit.len(), 40 | 64)
        || !exact_commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(SourceVerificationError::InvalidCommit);
    }
    let root = receiver_selected_canonical_project_root;
    let canonical = tokio::fs::canonicalize(root)
        .await
        .map_err(|_| SourceVerificationError::InvalidRoot)?;
    if canonical.as_os_str() != root.as_os_str() {
        return Err(SourceVerificationError::InvalidRoot);
    }
    let before = boundary(root).await?;
    let output = run_git(
        root,
        &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
        format!("{exact_commit}\n").into_bytes(),
    )
    .await?;
    // Type alone is insufficient: Git can resolve a deceptive all-hex ref name.
    let expected = format!("{exact_commit} commit\n");
    if output != expected.as_bytes() {
        return Err(SourceVerificationError::CommitUnavailable);
    }
    let after = boundary(root).await?;
    if before != after {
        return Err(SourceVerificationError::RepositoryChanged);
    }
    Ok(VerifiedSource {
        canonical_root: canonical,
        exact_commit: exact_commit.to_owned(),
    })
}

#[derive(PartialEq, Eq)]
struct DirectoryIdentity {
    path: PathBuf,
    device: u64,
    inode: u64,
}

async fn directory_identity(path: &Path) -> Result<DirectoryIdentity> {
    let canonical = tokio::fs::canonicalize(path)
        .await
        .map_err(|_| SourceVerificationError::InvalidRoot)?;
    let metadata = tokio::fs::metadata(&canonical)
        .await
        .map_err(|_| SourceVerificationError::InvalidRoot)?;
    if !metadata.is_dir() {
        return Err(SourceVerificationError::InvalidRoot);
    }
    Ok(DirectoryIdentity {
        path: canonical,
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

async fn git_path(root: &Path, option: &str) -> Result<PathBuf> {
    let mut bytes = run_git(
        root,
        &["rev-parse", "--path-format=absolute", option],
        Vec::new(),
    )
    .await?;
    if bytes.pop() != Some(b'\n') || bytes.is_empty() {
        return Err(SourceVerificationError::InvalidRoot);
    }
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

async fn boundary(root: &Path) -> Result<[DirectoryIdentity; 3]> {
    // --show-toplevel also rejects bare repositories and .git-internal directories.
    if git_path(root, "--show-toplevel").await?.as_os_str() != root.as_os_str() {
        return Err(SourceVerificationError::InvalidRoot);
    }
    let root_identity = directory_identity(root).await?;
    if root_identity.path.as_os_str() != root.as_os_str() {
        return Err(SourceVerificationError::InvalidRoot);
    }
    let git_dir = directory_identity(&git_path(root, "--absolute-git-dir").await?).await?;
    let common_dir = directory_identity(&git_path(root, "--git-common-dir").await?).await?;
    Ok([root_identity, git_dir, common_dir])
}

fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    // Enumerate keys, removing every inherited Git override (including future ones).
    // Keep the worker's locally trusted executable search path; no remote callbacks.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_PROTOCOL_FROM_USER", "0")
        .env("LC_ALL", "C")
        .args([
            "--no-pager",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "--no-optional-locks",
            "-c",
            "protocol.allow=never",
            "-c",
            "credential.helper=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
        ])
        .args(args);
    command
}

async fn read_bounded(mut stream: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let count = stream
            .read(&mut chunk)
            .await
            .map_err(|_| SourceVerificationError::GitFailed)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > OUTPUT_LIMIT {
            return Err(SourceVerificationError::OutputLimit);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

async fn run_git(root: &Path, args: &[&str], input: Vec<u8>) -> Result<Vec<u8>> {
    supervise(git_command(root, args), input, GIT_TIMEOUT).await
}

// The supervisor owns the child even if the request future is dropped. Never detach
// wait_with_output (unbounded allocation) or merely abandon the child's wait future.
async fn supervise(mut command: Command, input: Vec<u8>, timeout: Duration) -> Result<Vec<u8>> {
    let (mut sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        if sender.is_closed() {
            return;
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = sender.send(Err(SourceVerificationError::GitFailed));
                return;
            }
        };
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let result = {
            let inspect = async {
                let write = async {
                    stdin
                        .write_all(&input)
                        .await
                        .map_err(|_| SourceVerificationError::GitFailed)?;
                    drop(stdin);
                    Ok(())
                };
                let wait = async {
                    child
                        .wait()
                        .await
                        .map_err(|_| SourceVerificationError::GitFailed)
                };
                let (_, stdout, _stderr, status) =
                    tokio::try_join!(write, read_bounded(stdout), read_bounded(stderr), wait)?;
                if !status.success() {
                    return Err(SourceVerificationError::GitFailed);
                }
                Ok(stdout)
            };
            tokio::select! {
                biased;
                _ = sender.closed() => None,
                result = tokio::time::timeout(timeout, inspect) => Some(result.unwrap_or(Err(SourceVerificationError::Timeout))),
            }
        };
        // kill() waits for/reaps the direct child. Already-reaped success is harmless.
        if result.as_ref().is_none_or(|result| result.is_err()) {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(result) = result {
            let _ = sender.send(result);
        }
    });
    receiver
        .await
        .unwrap_or(Err(SourceVerificationError::GitFailed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::{TempDir, tempdir};

    fn git(root: &Path, args: &[&str]) -> String {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args([
                "-c",
                "user.name=Source Test",
                "-c",
                "user.email=source@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "scratch Git failed: {:?}", args);
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }

    fn repo() -> (TempDir, PathBuf, String) {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q"]);
        std::fs::write(root.join("tracked"), "original\n").unwrap();
        git(&root, &["add", "tracked"]);
        git(&root, &["commit", "-qm", "original"]);
        let commit = git(&root, &["rev-parse", "HEAD"]);
        (dir, root, commit)
    }

    #[tokio::test]
    async fn given_existing_commit_should_return_exact_local_evidence_without_mutations() {
        // given
        let (_dir, root, commit) = repo();
        std::fs::write(root.join("tracked"), "dirty\n").unwrap();
        let index = std::fs::read(root.join(".git/index")).unwrap();
        // when
        let verified = verify_existing_commit(&root, &commit).await.unwrap();
        // then
        assert_eq!(verified.canonical_root(), root);
        assert_eq!(verified.exact_commit(), commit);
        assert_eq!(git(&root, &["rev-parse", "HEAD"]), commit);
        assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
        assert_eq!(
            std::fs::read_to_string(root.join("tracked")).unwrap(),
            "dirty\n"
        );
    }

    #[tokio::test]
    async fn given_revision_aliases_should_reject_without_resolution() {
        // given
        let (_dir, root, commit) = repo();
        let inputs = [
            "HEAD",
            "main",
            "--help",
            "",
            &commit[..12],
            &commit.to_uppercase(),
            &format!("{commit}^{{commit}}"),
            "gggggggggggggggggggggggggggggggggggggggg",
            &format!("{commit}\n"),
        ];
        // when / then
        for input in inputs {
            assert_eq!(
                verify_existing_commit(&root, input).await.err(),
                Some(SourceVerificationError::InvalidCommit)
            );
        }
    }

    #[tokio::test]
    async fn given_absent_object_with_hex_branch_should_reject_resolved_commit() {
        // given
        let (_dir, root, commit) = repo();
        let absent = "1111111111111111111111111111111111111111";
        git(&root, &["branch", absent, &commit]);
        // when / then
        assert_eq!(
            verify_existing_commit(&root, absent).await.err(),
            Some(SourceVerificationError::CommitUnavailable)
        );
    }

    #[tokio::test]
    async fn given_noncommit_objects_should_reject_without_tag_peeling() {
        // given
        let (_dir, root, _commit) = repo();
        git(&root, &["tag", "-am", "tag", "annotated"]);
        let ids = [
            git(&root, &["rev-parse", "annotated"]),
            git(&root, &["rev-parse", "HEAD^{tree}"]),
            git(&root, &["rev-parse", "HEAD:tracked"]),
        ];
        // when / then
        for id in ids {
            assert_eq!(
                verify_existing_commit(&root, &id).await.err(),
                Some(SourceVerificationError::CommitUnavailable)
            );
        }
    }

    #[tokio::test]
    async fn given_detached_head_and_replacements_should_inspect_original_object_only() {
        // given
        let (_dir, root, commit) = repo();
        git(&root, &["checkout", "--detach", "-q", &commit]);
        let blob = git(&root, &["rev-parse", "HEAD:tracked"]);
        git(&root, &["replace", "-f", &blob, &commit]);
        let absent = "2222222222222222222222222222222222222222";
        git(
            &root,
            &["update-ref", &format!("refs/replace/{absent}"), &commit],
        );
        // when / then
        assert_eq!(
            verify_existing_commit(&root, &commit)
                .await
                .unwrap()
                .exact_commit(),
            commit
        );
        assert_eq!(
            verify_existing_commit(&root, &blob).await.err(),
            Some(SourceVerificationError::CommitUnavailable)
        );
        assert_eq!(
            verify_existing_commit(&root, absent).await.err(),
            Some(SourceVerificationError::CommitUnavailable)
        );
        // A replacement tree must not hide the actual commit's type either.
        let tree = git(&root, &["rev-parse", "HEAD^{tree}"]);
        git(&root, &["replace", "-f", &commit, &tree]);
        assert_eq!(
            verify_existing_commit(&root, &commit)
                .await
                .unwrap()
                .exact_commit(),
            commit
        );
    }

    #[tokio::test]
    async fn given_subdirectory_bare_or_root_alias_should_reject() {
        // given
        let (dir, root, commit) = repo();
        let subdir = root.join("nested");
        std::fs::create_dir(&subdir).unwrap();
        let bare_dir = tempdir().unwrap();
        let bare = bare_dir.path().canonicalize().unwrap();
        git(&bare, &["init", "--bare", "-q"]);
        let alias_dir = tempdir().unwrap();
        let alias = alias_dir.path().join("alias");
        std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
        // when / then
        for path in [
            &subdir,
            &bare,
            &alias,
            &root.join("nested/.."),
            &root.join("."),
            &root.join(".git"),
        ] {
            assert!(verify_existing_commit(path, &commit).await.is_err());
        }
    }

    #[tokio::test]
    async fn given_existing_linked_worktree_should_verify_its_own_metadata_boundary() {
        // given: test fixture only creates a worktree; verification does not.
        let (_dir, root, commit) = repo();
        let linked_dir = tempdir().unwrap();
        let linked = linked_dir.path().canonicalize().unwrap().join("linked");
        git(
            &root,
            &[
                "worktree",
                "add",
                "--detach",
                "-q",
                linked.to_str().unwrap(),
                &commit,
            ],
        );
        let before = std::fs::read(linked.join(".git")).unwrap();
        // when
        let verified = verify_existing_commit(&linked, &commit).await.unwrap();
        // then
        assert_eq!(verified.canonical_root(), linked);
        assert_eq!(std::fs::read(linked.join(".git")).unwrap(), before);
    }

    #[tokio::test]
    async fn given_missing_tree_should_report_commit_evidence_without_readiness_claim() {
        // given
        let (_dir, root, commit) = repo();
        let tree = git(&root, &["rev-parse", "HEAD^{tree}"]);
        std::fs::remove_file(root.join(".git/objects").join(&tree[..2]).join(&tree[2..])).unwrap();
        // when / then
        assert_eq!(
            verify_existing_commit(&root, &commit)
                .await
                .unwrap()
                .exact_commit(),
            commit
        );
    }

    #[tokio::test]
    async fn given_sha256_repository_should_accept_full_commit_if_supported() {
        // given
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let status = Command::new("git")
            .current_dir(&root)
            .args(["init", "-q", "--object-format=sha256"])
            .status()
            .unwrap();
        if !status.success() {
            eprintln!("installed Git lacks SHA256 repository support");
            return;
        }
        git(&root, &["commit", "--allow-empty", "-qm", "sha256"]);
        let commit = git(&root, &["rev-parse", "HEAD"]);
        // when / then
        assert_eq!(commit.len(), 64);
        // Forty hex characters pass SHA1 input shape but are only an abbreviation
        // in this repository. Git resolves them to a different, full SHA256 ID.
        assert_eq!(
            verify_existing_commit(&root, &commit[..40]).await.err(),
            Some(SourceVerificationError::CommitUnavailable)
        );
        assert_eq!(
            verify_existing_commit(&root, &commit)
                .await
                .unwrap()
                .exact_commit(),
            commit
        );
    }

    fn helper_command(mode: &str, directory: &Path) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "mesh_source::tests::isolated_child",
                "--nocapture",
            ])
            .env("PU_SOURCE_TEST_MODE", mode)
            .env("PU_SOURCE_TEST_DIR", directory);
        command
    }

    // Runs only in isolated child harnesses; no test mutates process-global Git env.
    #[test]
    #[ignore = "isolated child fixture"]
    fn isolated_child() {
        let mode = std::env::var("PU_SOURCE_TEST_MODE").unwrap();
        let dir = PathBuf::from(std::env::var_os("PU_SOURCE_TEST_DIR").unwrap());
        match mode.as_str() {
            "environment" => {
                let root = dir.join("selected");
                let commit = std::fs::read_to_string(dir.join("commit")).unwrap();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let verified = runtime
                    .block_on(verify_existing_commit(&root, &commit))
                    .unwrap();
                assert_eq!(verified.canonical_root(), root);
                assert_eq!(verified.exact_commit(), commit);
            }
            "boundary" => {
                let root = dir.join("selected");
                let commit = std::fs::read_to_string(dir.join("commit")).unwrap();
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                assert_eq!(
                    runtime
                        .block_on(verify_existing_commit(&root, &commit))
                        .err(),
                    Some(SourceVerificationError::RepositoryChanged)
                );
            }
            "sleep" => {
                std::fs::write(dir.join("pid.tmp"), std::process::id().to_string()).unwrap();
                std::fs::rename(dir.join("pid.tmp"), dir.join("pid")).unwrap();
                std::thread::sleep(Duration::from_secs(30));
            }
            "stdout" | "stderr" => {
                use std::io::Write;
                let bytes = vec![b'x'; OUTPUT_LIMIT * 8];
                if mode == "stdout" {
                    std::io::stdout().write_all(&bytes).unwrap();
                } else {
                    std::io::stderr().write_all(&bytes).unwrap();
                }
            }
            _ => panic!("unknown fixture mode"),
        }
    }

    #[tokio::test]
    async fn given_hostile_inherited_git_environment_should_use_selected_repository_only() {
        // given
        let dir = tempdir().unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let root = parent.join("selected");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "--allow-empty", "-qm", "selected"]);
        let commit = git(&root, &["rev-parse", "HEAD"]);
        std::fs::write(parent.join("commit"), &commit).unwrap();
        let (_other_dir, other, _other_commit) = repo();
        let mut command = helper_command("environment", &parent);
        command
            .env("GIT_DIR", other.join(".git"))
            .env("GIT_WORK_TREE", &other)
            .env("GIT_COMMON_DIR", other.join(".git"))
            .env("GIT_INDEX_FILE", parent.join("poison-index"))
            .env("GIT_OBJECT_DIRECTORY", other.join(".git/objects"))
            .env(
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                other.join(".git/objects"),
            )
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.bare")
            .env("GIT_CONFIG_VALUE_0", "true")
            .env("GIT_CONFIG_PARAMETERS", "invalid")
            .env("GIT_CONFIG_GLOBAL", parent.join("poison-config"))
            .env("GIT_CONFIG_SYSTEM", parent.join("poison-config"))
            .env("GIT_NAMESPACE", "poison")
            .env("GIT_TRACE", parent.join("poison-trace"))
            .env("GIT_OPTIONAL_LOCKS", "1")
            .env("GIT_NO_LAZY_FETCH", "0")
            .env("GIT_EXEC_PATH", parent.join("missing"));
        // when / then
        assert!(supervise(command, Vec::new(), GIT_TIMEOUT).await.is_ok());
        assert!(!parent.join("poison-index").exists());
        assert!(!parent.join("poison-trace").exists());
    }

    #[tokio::test]
    async fn given_excessive_child_output_should_fail_with_bounded_buffers() {
        // given / when / then
        let dir = tempdir().unwrap();
        for mode in ["stdout", "stderr"] {
            assert_eq!(
                supervise(helper_command(mode, dir.path()), Vec::new(), GIT_TIMEOUT)
                    .await
                    .err(),
                Some(SourceVerificationError::OutputLimit)
            );
        }
    }

    async fn await_pid(dir: &Path) -> u32 {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(pid) = tokio::fs::read_to_string(dir.join("pid")).await {
                    break pid.parse().unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    async fn await_reaped(pid: u32) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                // Only this scratch helper PID is queried; never signal a daemon.
                let result = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None);
                if result == Err(nix::errno::Errno::ESRCH) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("scratch child should be killed and reaped");
    }

    #[tokio::test]
    async fn given_stalled_child_should_timeout_and_reap() {
        // given
        let dir = tempdir().unwrap();
        let command = helper_command("sleep", dir.path());
        // when
        let result = supervise(command, Vec::new(), Duration::from_millis(500)).await;
        // then
        assert_eq!(result.err(), Some(SourceVerificationError::Timeout));
        await_reaped(await_pid(dir.path()).await).await;
    }

    #[tokio::test]
    async fn given_cancelled_request_should_kill_and_reap_its_child() {
        // given
        let dir = tempdir().unwrap();
        let command = helper_command("sleep", dir.path());
        let request = tokio::spawn(supervise(command, Vec::new(), GIT_TIMEOUT));
        let pid = await_pid(dir.path()).await;
        // when
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        // then
        await_reaped(pid).await;
    }

    #[tokio::test]
    async fn given_promisor_missing_object_should_never_attempt_local_remote_helper() {
        // given: a local marker-only helper, with no real network endpoint.
        use std::os::unix::fs::PermissionsExt;
        let (_dir, root, commit) = repo();
        let absent = "3333333333333333333333333333333333333333";
        let helper = root.join("marker-helper");
        let marker = root.join("fetch-attempted");
        std::fs::write(&helper, "#!/bin/sh\nprintf attempted > \"$1\"\nexit 1\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        git(&root, &["config", "core.repositoryformatversion", "1"]);
        git(&root, &["config", "extensions.partialClone", "origin"]);
        git(&root, &["config", "remote.origin.promisor", "true"]);
        git(&root, &["config", "protocol.ext.allow", "always"]);
        git(
            &root,
            &[
                "config",
                "remote.origin.url",
                &format!("ext::{} {}", helper.display(), marker.display()),
            ],
        );
        // Prove this fixture actually attempts lazy fetching without our safeguards.
        let status = Command::new("git")
            .current_dir(&root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("GIT_NO_LAZY_FETCH")
            .env_remove("GIT_ALLOW_PROTOCOL")
            .args(["cat-file", "-e", absent])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success());
        assert!(
            marker.exists(),
            "promisor control must attempt the marker helper"
        );
        std::fs::remove_file(&marker).unwrap();
        // when
        let result = verify_existing_commit(&root, absent).await;
        // then
        assert_eq!(
            result.err(),
            Some(SourceVerificationError::CommitUnavailable)
        );
        assert!(!marker.exists());
        assert_eq!(
            verify_existing_commit(&root, &commit)
                .await
                .unwrap()
                .exact_commit(),
            commit
        );
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn given_git_metadata_replaced_during_verification_should_reject_evidence() {
        // given: a worker-local test-only Git wrapper pauses the actual object query.
        // No callback/executable parameter is exposed by the production source API.
        use std::os::unix::fs::PermissionsExt;
        let (_repo_dir, original, commit) = repo();
        let dir = tempdir().unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let selected = parent.join("selected");
        let replacement = parent.join("replacement");
        git(
            &parent,
            &[
                "clone",
                "-q",
                "--no-hardlinks",
                original.to_str().unwrap(),
                selected.to_str().unwrap(),
            ],
        );
        git(
            &parent,
            &[
                "clone",
                "-q",
                "--no-hardlinks",
                original.to_str().unwrap(),
                replacement.to_str().unwrap(),
            ],
        );
        std::fs::write(parent.join("commit"), commit).unwrap();
        let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|path| path.join("git"))
            .find(|path| path.is_file())
            .unwrap()
            .canonicalize()
            .unwrap();
        let bin = parent.join("bin");
        std::fs::create_dir(&bin).unwrap();
        let wrapper = bin.join("git");
        // Temp paths are generated locally. Quote the executable to keep this fixture
        // valid when the locally installed Git lives under a path containing spaces.
        let quoted_git = real_git.to_str().unwrap().replace('\'', "'\\''");
        std::fs::write(&wrapper, format!(
            "#!/bin/sh\nfor arg do\n if [ \"$arg\" = cat-file ]; then\n  printf ready > \"$PU_SOURCE_TEST_DIR/ready\"\n  while [ ! -f \"$PU_SOURCE_TEST_DIR/continue\" ]; do /bin/sleep 0.01; done\n fi\ndone\nexec '{quoted_git}' \"$@\"\n"
        )).unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        let mut command = helper_command("boundary", &parent);
        command.env("PATH", std::env::join_paths(paths).unwrap());
        let verification = tokio::spawn(supervise(command, Vec::new(), GIT_TIMEOUT));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !parent.join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // when: preserve the same commit but replace .git's directory identity.
        std::fs::rename(selected.join(".git"), parent.join("old-git")).unwrap();
        std::fs::rename(replacement.join(".git"), selected.join(".git")).unwrap();
        std::fs::write(parent.join("continue"), "continue").unwrap();
        // then: child asserts the specific RepositoryChanged error.
        assert!(verification.await.unwrap().is_ok());
    }
}
