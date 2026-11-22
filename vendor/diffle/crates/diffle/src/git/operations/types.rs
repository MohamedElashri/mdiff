use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum Action {
    Merge { revision: String },
    CherryPick { revision: String, mainline: Option<u32> },
    Revert { revision: String, mainline: Option<u32> },
    Rebase { upstream: String },
    RebaseInteractive { upstream: String, todo: Vec<RebaseItem> },
    Recover { operation: String, command: RecoveryCommand },
    ResolveText { path: String, content: String, expected: String },
    Init { path: String, branch: String },
    Clone { path: String, url: String },
    BranchCreate { name: String, start: String },
    BranchSwitch { name: String },
    BranchDelete { name: String },
    TagCreate { name: String, start: String },
    TagDelete { name: String },
    RemoteAdd { name: String, url: String },
    RemoteRemove { name: String },
    Stage { path: String },
    Unstage { path: String },
    Hunk { path: String, staged: bool, hunk: usize },
    Commit { message: String, amend: bool },
    Restore { path: String },
    Discard { path: String },
    Fetch { remote: String },
    Pull { remote: String, branch: String },
    Push { remote: String, branch: String, force: bool },
    WorktreeCreate { path: String, branch: String },
    WorktreeLock { path: String, reason: String },
    WorktreeUnlock { path: String },
    WorktreeRemove { path: String },
    WorktreePrune,
    StashSave { message: String, untracked: bool },
    StashApply { oid: String },
    StashPop { oid: String },
    StashDrop { oid: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RecoveryCommand {
    Continue,
    Skip,
    Abort,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RebaseItem {
    pub oid: String,
    pub command: RebaseCommand,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RebaseCommand {
    Pick,
    Drop,
    Squash,
    Fixup,
}
impl Action {
    pub fn advanced(&self) -> bool {
        matches!(
            self,
            Self::Merge { .. }
                | Self::CherryPick { .. }
                | Self::Revert { .. }
                | Self::Rebase { .. }
                | Self::RebaseInteractive { .. }
                | Self::Recover { .. }
                | Self::ResolveText { .. }
        )
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Merge { .. } => "Merge revision",
            Self::CherryPick { .. } => "Cherry-pick commit",
            Self::Revert { .. } => "Revert commit",
            Self::Rebase { .. } => "Rebase branch",
            Self::RebaseInteractive { .. } => "Run interactive rebase",
            Self::ResolveText { .. } => "Save and stage resolution",
            Self::Recover { command: RecoveryCommand::Continue, .. } => "Continue Git operation",
            Self::Recover { command: RecoveryCommand::Skip, .. } => "Skip current commit",
            Self::Recover { command: RecoveryCommand::Abort, .. } => "Abort Git operation",
            Self::Init { .. } => "Initialize repository",
            Self::Clone { .. } => "Clone repository",
            Self::BranchCreate { .. } => "Create branch",
            Self::BranchSwitch { .. } => "Switch branch",
            Self::BranchDelete { .. } => "Delete merged branch",
            Self::TagCreate { .. } => "Create lightweight tag",
            Self::TagDelete { .. } => "Delete tag",
            Self::RemoteAdd { .. } => "Add remote",
            Self::RemoteRemove { .. } => "Remove remote",
            Self::Stage { .. } => "Stage file",
            Self::Unstage { .. } => "Unstage file",
            Self::Hunk { staged: false, .. } => "Stage hunk",
            Self::Hunk { staged: true, .. } => "Unstage hunk",
            Self::Commit { amend: false, .. } => "Commit staged changes",
            Self::Commit { amend: true, .. } => "Amend commit",
            Self::Restore { .. } => "Restore file from index",
            Self::Discard { .. } => "Delete untracked file",
            Self::Fetch { .. } => "Fetch remote",
            Self::Pull { .. } => "Fast-forward pull",
            Self::Push { force: false, .. } => "Push branch",
            Self::Push { force: true, .. } => "Push with exact lease",
            Self::WorktreeCreate { .. } => "Create worktree",
            Self::WorktreeLock { .. } => "Lock worktree",
            Self::WorktreeUnlock { .. } => "Unlock worktree",
            Self::WorktreeRemove { .. } => "Remove clean worktree",
            Self::WorktreePrune => "Prune missing worktree registrations",
            Self::StashSave { .. } => "Save stash",
            Self::StashApply { .. } => "Apply stash",
            Self::StashPop { .. } => "Pop stash",
            Self::StashDrop { .. } => "Drop stash",
        }
    }
    pub fn network(&self) -> bool {
        matches!(self, Self::Fetch { .. } | Self::Pull { .. } | Self::Push { .. } | Self::Clone { .. })
    }
    pub fn index_only(&self) -> bool {
        matches!(self, Self::Stage { .. } | Self::Unstage { .. } | Self::Hunk { .. })
    }
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::ResolveText { path, .. }
            | Self::Stage { path }
            | Self::Unstage { path }
            | Self::Hunk { path, .. }
            | Self::Restore { path }
            | Self::Discard { path } => Some(path),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub root: PathBuf,
    pub git: PathBuf,
    pub common: PathBuf,
    pub inodes: Vec<(u64, u64)>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub stash_fingerprint: String,
    pub identity: Identity,
    pub fingerprint: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub operations: Vec<String>,
    pub conflicts: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub path: String,
    pub old_path: Option<String>,
    pub index: char,
    pub worktree: char,
    pub untracked: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub fingerprint: String,
    pub text: String,
    pub hunks: Vec<String>,
    pub partial: bool,
}
#[derive(Clone, Debug)]
pub struct Step {
    pub args: Vec<String>,
    pub input: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub destination: Option<PathBuf>,
    pub before: Snapshot,
    pub action: Action,
    pub steps: Vec<Step>,
    pub preview: String,
    pub patch: Option<Patch>,
    pub remote: Option<RemoteProof>,
    pub extra: Vec<(PathBuf, String)>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProof {
    pub remote: String,
    pub branch: String,
    pub url_hash: String,
    pub expected: Option<String>,
    pub intended: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub status: String,
    pub message: String,
    pub after: Option<Snapshot>,
}
