//! Values the git service returns (the GitService half of `Models/DiffModel.swift` and
//! `Models/PRModel.swift`). They cross the Tauri boundary as camelCase JSON; the front-end
//! mirrors are in `ui/src/services/git.ts`.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::host::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineType {
    Context,
    Addition,
    Deletion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    #[serde(rename = "type")]
    pub line_type: LineType,
    /// The code without its `+`/`-`/` ` prefix.
    pub content: String,
    pub old_line_no: Option<i64>,
    pub new_line_no: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    /// e.g. `@@ -10,6 +10,8 @@ func login()`.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// An on-disk patch range. `snapshot` names a file the service created and still retains;
/// pages are read from it, so later pages never regenerate git output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSlice {
    pub snapshot: String,
    pub start: u64,
    pub end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub filename: String,
    /// `M`, `A`, `D`, `R`, `U` or `??`.
    pub status_code: String,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
    #[serde(default)]
    pub old_filename: Option<String>,
    #[serde(default)]
    pub is_binary: bool,
    #[serde(default)]
    pub preview_error: Option<String>,
    #[serde(default)]
    pub preview_deferred: bool,
    /// Conflict working-file patches compare against stage 2, or empty if ours deleted the path.
    #[serde(default)]
    pub conflict_baseline: Option<String>,
    /// Commands and immutable disk slices keep bodies out of overview metadata.
    #[serde(default)]
    pub patch_arguments: Option<Vec<String>>,
    #[serde(default)]
    pub patch_slice: Option<PatchSlice>,
    #[serde(default)]
    pub preview_version: String,
}

impl FileDiff {
    pub fn new(
        filename: impl Into<String>,
        status: impl Into<String>,
        added: usize,
        removed: usize,
        hunks: Vec<Hunk>,
    ) -> Self {
        Self {
            filename: filename.into(),
            status_code: status.into(),
            added,
            removed,
            hunks,
            old_filename: None,
            is_binary: false,
            preview_error: None,
            preview_deferred: false,
            conflict_baseline: None,
            patch_arguments: None,
            patch_slice: None,
            preview_version: String::new(),
        }
    }

    pub fn should_collapse(&self) -> bool {
        self.added + self.removed > 200 || self.status_code == "??"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffData {
    pub files: Vec<FileDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPage {
    pub slice: PatchSlice,
    pub hunks: Vec<Hunk>,
    pub next_offset: u64,
    pub old_line: i64,
    pub new_line: i64,
    pub header: String,
    pub has_more: bool,
    pub is_binary: bool,
    pub has_long_lines: bool,
}

/// Lightweight resume state; it never retains rendered hunk bodies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPageCursor {
    pub slice: PatchSlice,
    pub next_offset: u64,
    pub old_line: i64,
    pub new_line: i64,
    pub header: String,
    pub is_binary: bool,
}

impl ReviewPage {
    pub fn continuation(&self) -> ReviewPageCursor {
        ReviewPageCursor {
            slice: self.slice.clone(),
            next_offset: self.next_offset,
            old_line: self.old_line,
            new_line: self.new_line,
            header: self.header.clone(),
            is_binary: self.is_binary,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitInfo {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitReviewSummary {
    pub commit_count: usize,
    pub local_file_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchReview {
    pub comparison_base: String,
    pub available_bases: Vec<String>,
    pub files: Vec<FileDiff>,
    pub commits: Vec<GitCommitInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLocalReview {
    pub staged: Vec<FileDiff>,
    pub unstaged: Vec<FileDiff>,
    pub untracked: Vec<FileDiff>,
    pub error: Option<String>,
    pub staged_error: Option<String>,
    pub unstaged_error: Option<String>,
    pub untracked_error: Option<String>,
}

impl GitLocalReview {
    pub fn unique_path_count(&self) -> usize {
        let all = self
            .staged
            .iter()
            .chain(&self.unstaged)
            .chain(&self.untracked);
        all.map(|file| file.filename.as_str())
            .collect::<HashSet<_>>()
            .len()
    }
}

/// One entry of `gh pr list --json …`; field names match gh's JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestInfo {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub author: PrAuthor,
    pub labels: Vec<PrLabel>,
    #[serde(default)]
    pub review_decision: Option<String>,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub is_draft: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrAuthor {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrLabel {
    pub name: String,
    pub color: String,
}

/// What a git operation reports when it fails (`GitReviewError`, `WorkingTreeDiffError`,
/// `FilePreviewError` and host failures on the Mac). The message is what the Mac shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// `GitReviewError`: shown as is.
    Review(String),
    /// `WorkingTreeDiffError.git`: git's stderr, trimmed.
    WorkingTree(String),
    /// `FilePreviewError.tooLarge`.
    TooLarge,
    /// `FilePreviewError.unsupported`.
    Unsupported,
    /// The caller cancelled the work (Swift's `CancellationError`).
    Cancelled,
    Host(HostError),
    Io(String),
}

impl GitError {
    pub fn review(message: impl Into<String>) -> Self {
        Self::Review(message.into())
    }

    /// A stable name for the front end.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Review(_) => "review",
            Self::WorkingTree(_) => "workingTree",
            Self::TooLarge => "tooLarge",
            Self::Unsupported => "unsupported",
            Self::Cancelled => "cancelled",
            Self::Host(_) => "host",
            Self::Io(_) => "io",
        }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Review(message) => f.write_str(message),
            Self::WorkingTree(message) => f.write_str(message.trim()),
            Self::TooLarge => f.write_str("File is too large to preview (over 1 MB)."),
            Self::Unsupported => f.write_str("This file cannot be previewed."),
            Self::Cancelled => f.write_str("The operation was cancelled."),
            Self::Host(error) => write!(f, "{error}"),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GitError {}

impl From<HostError> for GitError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

impl From<std::io::Error> for GitError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// The JSON shape a failed Tauri command rejects with.
impl Serialize for GitError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("GitError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

pub type GitResult<T> = Result<T, GitError>;
