//! In-memory Git implementation for running TUI applications in the browser.
//!
//! Provides a [`GitRepository`] trait that abstracts common git operations,
//! plus an [`InMemoryGitRepository`] implementation that operates entirely
//! on a [`Filesystem`](crate::fs::Filesystem) without requiring a real git
//! binary or `libgit2`.  This makes it possible to run applications like
//! *hunky* in a WASM environment where neither is available.
//!
//! ## Supported operations
//!
//! | Operation         | Description |
//! |-------------------|-------------|
//! | `init`            | Initialise a new repository |
//! | `status`          | List changed / staged / untracked files |
//! | `diff_unstaged`   | Unified diff of unstaged working-directory changes |
//! | `diff_staged`     | Unified diff of staged (index) changes |
//! | `diff_commit`     | Unified diff introduced by a specific commit |
//! | `stage_file`      | Stage a file (add to index) |
//! | `unstage_file`    | Remove a file from the index |
//! | `commit`          | Record a new commit with a message |
//! | `log`             | List recent commits |

use std::collections::BTreeMap;
use std::fmt;

use crate::fs::{Filesystem, MemoryFilesystem};

// ── Error types ──────────────────────────────────────────────────────────────

/// Errors produced by [`GitRepository`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitError {
    /// The repository has not been initialised.
    NotInitialised,
    /// Nothing to commit (empty staging area).
    NothingToCommit,
    /// A general-purpose error with a human-readable message.
    Other(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::NotInitialised => write!(f, "repository not initialised"),
            GitError::NothingToCommit => write!(f, "nothing to commit"),
            GitError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GitError {}

// ── Data types ───────────────────────────────────────────────────────────────

/// The status of a file relative to HEAD and the staging area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Untracked,
}

impl fmt::Display for FileStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileStatus::Added => write!(f, "Added"),
            FileStatus::Modified => write!(f, "Modified"),
            FileStatus::Deleted => write!(f, "Deleted"),
            FileStatus::Untracked => write!(f, "Untracked"),
        }
    }
}

/// An entry in the output of [`GitRepository::status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub path: String,
    /// Status relative to the staging area / working directory.
    pub status: FileStatus,
    /// `true` when the change is staged (in the index).
    pub staged: bool,
}

/// A hunk inside a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub old_start: usize,
    pub new_start: usize,
    /// Lines including the diff prefix (`+`, `-`, or ` `).
    pub lines: Vec<String>,
}

/// Per-file diff information returned by diff operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<DiffHunk>,
}

/// Metadata for a commit in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    /// Full hex-encoded SHA-like identifier.
    pub sha: String,
    /// Abbreviated identifier (first 7 characters).
    pub short_sha: String,
    /// First line of the commit message.
    pub summary: String,
    /// Author name.
    pub author: String,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstraction over git operations.
///
/// The trait is object-safe so it can be used behind `dyn GitRepository`.
pub trait GitRepository {
    /// Return the working-directory status: changed, staged, and untracked files.
    fn status(&self) -> Result<Vec<StatusEntry>, GitError>;

    /// Produce a unified diff of *unstaged* working-directory changes
    /// (index → working tree).
    fn diff_unstaged(&self) -> Result<Vec<FileDiff>, GitError>;

    /// Produce a unified diff of *staged* changes (HEAD → index).
    fn diff_staged(&self) -> Result<Vec<FileDiff>, GitError>;

    /// Produce a unified diff introduced by a specific commit.
    fn diff_commit(&self, sha: &str) -> Result<Vec<FileDiff>, GitError>;

    /// Stage a file (add to the index).
    fn stage_file(&mut self, path: &str) -> Result<(), GitError>;

    /// Remove a file from the index (unstage).
    fn unstage_file(&mut self, path: &str) -> Result<(), GitError>;

    /// Create a new commit with the given message.  Returns the commit SHA.
    fn commit(&mut self, message: &str, author: &str) -> Result<String, GitError>;

    /// Return the most recent commits (newest first), up to `max_count`.
    fn log(&self, max_count: usize) -> Result<Vec<CommitInfo>, GitError>;
}

// ── In-memory implementation ─────────────────────────────────────────────────

/// Snapshot of file contents at a point in time.
type TreeSnapshot = BTreeMap<String, Vec<u8>>;

/// An in-memory commit record.
#[derive(Debug, Clone)]
struct Commit {
    sha: String,
    message: String,
    author: String,
    /// Snapshot of the full tree at this commit.
    tree: TreeSnapshot,
}

/// A fully in-memory [`GitRepository`] that operates on a
/// [`MemoryFilesystem`].
///
/// The implementation maintains:
/// - The **HEAD** tree (snapshot at the last commit)
/// - The **index** (staging area)
/// - A linear commit history
///
/// Diff generation uses a simple line-by-line comparison.
#[derive(Debug, Clone)]
pub struct InMemoryGitRepository {
    /// The underlying filesystem (working tree).
    fs: MemoryFilesystem,
    /// HEAD tree snapshot.
    head: TreeSnapshot,
    /// Staging area (index).
    index: TreeSnapshot,
    /// Linear commit history, newest last.
    commits: Vec<Commit>,
    /// Monotonic counter for generating pseudo-SHA identifiers.
    next_id: u64,
}

impl InMemoryGitRepository {
    /// Initialise a new repository over the given filesystem.
    pub fn new(fs: MemoryFilesystem) -> Self {
        InMemoryGitRepository {
            fs,
            head: BTreeMap::new(),
            index: BTreeMap::new(),
            commits: Vec::new(),
            next_id: 1,
        }
    }

    /// Return a shared reference to the underlying filesystem.
    pub fn filesystem(&self) -> &MemoryFilesystem {
        &self.fs
    }

    /// Return a mutable reference to the underlying filesystem.
    pub fn filesystem_mut(&mut self) -> &mut MemoryFilesystem {
        &mut self.fs
    }

    // ── internal helpers ─────────────────────────────────────────────────

    /// Generate a deterministic hex-string identifier.
    fn make_sha(&mut self) -> String {
        let id = self.next_id;
        self.next_id += 1;
        format!("{id:016x}")
    }

    /// Build a snapshot of the current working tree from the filesystem.
    fn working_tree(&self) -> TreeSnapshot {
        let mut tree = BTreeMap::new();
        for path in self.fs.list_files() {
            if let Ok(data) = self.fs.read_file(&path) {
                tree.insert(path, data);
            }
        }
        tree
    }

    /// Compute the unified diff between two snapshots.
    fn diff_trees(old: &TreeSnapshot, new: &TreeSnapshot) -> Vec<FileDiff> {
        let mut diffs = Vec::new();
        let mut all_paths: std::collections::BTreeSet<&String> = std::collections::BTreeSet::new();
        all_paths.extend(old.keys());
        all_paths.extend(new.keys());

        for path in all_paths {
            let old_content = old.get(path);
            let new_content = new.get(path);

            match (old_content, new_content) {
                (None, Some(new_data)) => {
                    // Added file.
                    let new_str = String::from_utf8_lossy(new_data);
                    let hunks = diff_added(&new_str);
                    diffs.push(FileDiff {
                        path: path.clone(),
                        status: FileStatus::Added,
                        hunks,
                    });
                }
                (Some(old_data), None) => {
                    // Deleted file.
                    let old_str = String::from_utf8_lossy(old_data);
                    let hunks = diff_deleted(&old_str);
                    diffs.push(FileDiff {
                        path: path.clone(),
                        status: FileStatus::Deleted,
                        hunks,
                    });
                }
                (Some(old_data), Some(new_data)) => {
                    if old_data != new_data {
                        let old_str = String::from_utf8_lossy(old_data);
                        let new_str = String::from_utf8_lossy(new_data);
                        let hunks = diff_modified(&old_str, &new_str);
                        diffs.push(FileDiff {
                            path: path.clone(),
                            status: FileStatus::Modified,
                            hunks,
                        });
                    }
                }
                (None, None) => {}
            }
        }

        diffs
    }
}

impl GitRepository for InMemoryGitRepository {
    fn status(&self) -> Result<Vec<StatusEntry>, GitError> {
        let work = self.working_tree();
        let mut entries = Vec::new();

        // Gather all known paths.
        let mut all_paths: std::collections::BTreeSet<&String> =
            std::collections::BTreeSet::new();
        all_paths.extend(self.head.keys());
        all_paths.extend(self.index.keys());
        all_paths.extend(work.keys());

        for path in all_paths {
            let in_head = self.head.contains_key(path);
            let in_index = self.index.contains_key(path);
            let in_work = work.contains_key(path);

            // Staged changes (HEAD → index).
            match (in_head, in_index) {
                (false, true) => entries.push(StatusEntry {
                    path: path.clone(),
                    status: FileStatus::Added,
                    staged: true,
                }),
                (true, true) if self.head.get(path) != self.index.get(path) => {
                    entries.push(StatusEntry {
                        path: path.clone(),
                        status: FileStatus::Modified,
                        staged: true,
                    });
                }
                (true, false) => entries.push(StatusEntry {
                    path: path.clone(),
                    status: FileStatus::Deleted,
                    staged: true,
                }),
                _ => {}
            }

            // Unstaged changes (index → working tree) or (HEAD → working tree
            // for untracked).
            let baseline = if in_index {
                self.index.get(path)
            } else if in_head {
                self.head.get(path)
            } else {
                None
            };

            match (baseline, in_work) {
                (None, true) if !in_head && !in_index => {
                    entries.push(StatusEntry {
                        path: path.clone(),
                        status: FileStatus::Untracked,
                        staged: false,
                    });
                }
                (Some(base), true) if Some(base) != work.get(path) => {
                    entries.push(StatusEntry {
                        path: path.clone(),
                        status: FileStatus::Modified,
                        staged: false,
                    });
                }
                (Some(_), false) if !entries.iter().any(|e| e.path == *path && e.staged) => {
                    entries.push(StatusEntry {
                        path: path.clone(),
                        status: FileStatus::Deleted,
                        staged: false,
                    });
                }
                _ => {}
            }
        }

        Ok(entries)
    }

    fn diff_unstaged(&self) -> Result<Vec<FileDiff>, GitError> {
        let work = self.working_tree();
        // Base is the index if it has the file, otherwise HEAD.
        let mut base = self.head.clone();
        for (k, v) in &self.index {
            base.insert(k.clone(), v.clone());
        }
        // Remove files that were staged as deleted.
        for k in self.head.keys() {
            if !self.index.contains_key(k)
                && self
                    .commits
                    .last()
                    .map_or(false, |_| !self.index.contains_key(k))
            {
                // If index explicitly doesn't have this file but HEAD does,
                // it was staged as deleted – still use HEAD as the base so
                // that working-tree additions show up.
            }
        }
        Ok(Self::diff_trees(&base, &work))
    }

    fn diff_staged(&self) -> Result<Vec<FileDiff>, GitError> {
        Ok(Self::diff_trees(&self.head, &self.index))
    }

    fn diff_commit(&self, sha: &str) -> Result<Vec<FileDiff>, GitError> {
        let commit = self
            .commits
            .iter()
            .find(|c| c.sha == sha)
            .ok_or_else(|| GitError::Other(format!("commit not found: {sha}")))?;

        // Find the parent (previous commit).
        let parent_tree: TreeSnapshot = self
            .commits
            .iter()
            .zip(self.commits.iter().skip(1))
            .find(|(_, cur)| cur.sha == sha)
            .map(|(prev, _)| prev.tree.clone())
            .unwrap_or_default();

        Ok(Self::diff_trees(&parent_tree, &commit.tree))
    }

    fn stage_file(&mut self, path: &str) -> Result<(), GitError> {
        let work = self.working_tree();
        if let Some(data) = work.get(path) {
            self.index.insert(path.to_string(), data.clone());
        } else if self.head.contains_key(path) {
            // File was deleted in working tree – record the deletion in the
            // index by removing it.
            self.index.remove(path);
        } else {
            return Err(GitError::Other(format!("file not found: {path}")));
        }
        Ok(())
    }

    fn unstage_file(&mut self, path: &str) -> Result<(), GitError> {
        if self.head.contains_key(path) {
            // Revert index to HEAD version.
            self.index
                .insert(path.to_string(), self.head[path].clone());
        } else {
            // File didn't exist in HEAD – remove from index entirely.
            self.index.remove(path);
        }
        Ok(())
    }

    fn commit(&mut self, message: &str, author: &str) -> Result<String, GitError> {
        if self.index == self.head {
            return Err(GitError::NothingToCommit);
        }
        let sha = self.make_sha();
        let commit = Commit {
            sha: sha.clone(),
            message: message.to_string(),
            author: author.to_string(),
            tree: self.index.clone(),
        };
        self.head = self.index.clone();
        self.commits.push(commit);
        Ok(sha)
    }

    fn log(&self, max_count: usize) -> Result<Vec<CommitInfo>, GitError> {
        let infos: Vec<CommitInfo> = self
            .commits
            .iter()
            .rev()
            .take(max_count)
            .map(|c| {
                let short = if c.sha.len() >= 7 {
                    c.sha[..7].to_string()
                } else {
                    c.sha.clone()
                };
                CommitInfo {
                    sha: c.sha.clone(),
                    short_sha: short,
                    summary: c.message.lines().next().unwrap_or("").to_string(),
                    author: c.author.clone(),
                }
            })
            .collect();
        Ok(infos)
    }
}

// ── Diff helpers ─────────────────────────────────────────────────────────────

/// Produce hunks for a newly-added file (all lines are `+`).
fn diff_added(content: &str) -> Vec<DiffHunk> {
    let lines: Vec<String> = content.lines().map(|l| format!("+{l}\n")).collect();
    if lines.is_empty() {
        return Vec::new();
    }
    vec![DiffHunk {
        old_start: 0,
        new_start: 1,
        lines,
    }]
}

/// Produce hunks for a deleted file (all lines are `-`).
fn diff_deleted(content: &str) -> Vec<DiffHunk> {
    let lines: Vec<String> = content.lines().map(|l| format!("-{l}\n")).collect();
    if lines.is_empty() {
        return Vec::new();
    }
    vec![DiffHunk {
        old_start: 1,
        new_start: 0,
        lines,
    }]
}

/// Produce hunks for a modified file using a simple LCS-based line diff.
fn diff_modified(old: &str, new: &str) -> Vec<DiffHunk> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let edit_script = lcs_diff(&old_lines, &new_lines);

    // Group consecutive edits into hunks with up to 3 context lines.
    let context = 3;
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut i = 0;

    while i < edit_script.len() {
        // Skip leading context lines until we hit a change.
        if matches!(edit_script[i], Edit::Equal(_, _)) {
            i += 1;
            continue;
        }

        // Find the start of the change region with context.
        let change_start = i;

        // Walk backwards to include up to `context` preceding Equal lines.
        let ctx_before_start = {
            let mut s = change_start;
            let mut ctx = 0;
            while s > 0 && ctx < context {
                if matches!(edit_script[s - 1], Edit::Equal(_, _)) {
                    s -= 1;
                    ctx += 1;
                } else {
                    break;
                }
            }
            s
        };

        // Find end of this change group (including bridged gaps).
        let mut change_end = change_start;
        while change_end < edit_script.len() {
            if matches!(edit_script[change_end], Edit::Equal(_, _)) {
                // Count how many equal lines follow.
                let mut eq_count = 0;
                let mut j = change_end;
                while j < edit_script.len() && matches!(edit_script[j], Edit::Equal(_, _)) {
                    eq_count += 1;
                    j += 1;
                }
                // If the gap is small enough and there are more changes after,
                // merge them into the same hunk.
                if eq_count <= context * 2 && j < edit_script.len() {
                    change_end = j;
                } else {
                    break;
                }
            } else {
                change_end += 1;
            }
        }

        // Include up to `context` trailing Equal lines.
        let ctx_after_end = {
            let mut e = change_end;
            let mut ctx = 0;
            while e < edit_script.len() && ctx < context {
                if matches!(edit_script[e], Edit::Equal(_, _)) {
                    e += 1;
                    ctx += 1;
                } else {
                    break;
                }
            }
            e
        };

        // Determine old_start / new_start from the first edit in the hunk.
        let (old_start, new_start) = match &edit_script[ctx_before_start] {
            Edit::Equal(o, n) => (*o + 1, *n + 1),
            Edit::Insert(_, n) => (if *n > 0 { *n } else { 0 }, *n + 1),
            Edit::Delete(o, _) => (*o + 1, if *o > 0 { *o } else { 0 }),
        };

        let mut lines = Vec::new();
        for edit in &edit_script[ctx_before_start..ctx_after_end] {
            match edit {
                Edit::Equal(o, _) => lines.push(format!(" {}\n", old_lines[*o])),
                Edit::Delete(o, _) => lines.push(format!("-{}\n", old_lines[*o])),
                Edit::Insert(_, n) => lines.push(format!("+{}\n", new_lines[*n])),
            }
        }

        hunks.push(DiffHunk {
            old_start,
            new_start,
            lines,
        });

        i = ctx_after_end;
    }

    hunks
}

// ── Minimal LCS diff ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
enum Edit {
    Equal(usize, usize),  // (old_idx, new_idx)
    Delete(usize, usize), // (old_idx, new_idx – positional context)
    Insert(usize, usize), // (old_idx – positional context, new_idx)
}

/// Compute a line-level edit script using the classic LCS dynamic-programming
/// algorithm.  Good enough for the typical diff sizes encountered in a TUI.
fn lcs_diff<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Edit> {
    let m = old.len();
    let n = new.len();

    // Build LCS table.
    let mut table = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            if old[i] == new[j] {
                table[i][j] = table[i + 1][j + 1] + 1;
            } else {
                table[i][j] = table[i + 1][j].max(table[i][j + 1]);
            }
        }
    }

    // Backtrack to produce the edit script.
    let mut edits = Vec::new();
    let mut i = 0;
    let mut j = 0;
    while i < m || j < n {
        if i < m && j < n && old[i] == new[j] {
            edits.push(Edit::Equal(i, j));
            i += 1;
            j += 1;
        } else if j < n && (i >= m || table[i][j + 1] >= table[i + 1][j]) {
            edits.push(Edit::Insert(i, j));
            j += 1;
        } else {
            edits.push(Edit::Delete(i, j));
            i += 1;
        }
    }

    edits
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::Filesystem;

    fn setup() -> InMemoryGitRepository {
        let fs = MemoryFilesystem::new();
        InMemoryGitRepository::new(fs)
    }

    #[test]
    fn status_empty_repo() {
        let repo = setup();
        let st = repo.status().unwrap();
        assert!(st.is_empty());
    }

    #[test]
    fn status_untracked_file() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("hello.txt", b"world")
            .unwrap();
        let st = repo.status().unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].path, "hello.txt");
        assert_eq!(st[0].status, FileStatus::Untracked);
        assert!(!st[0].staged);
    }

    #[test]
    fn stage_and_commit() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("a.txt", b"hello")
            .unwrap();
        repo.stage_file("a.txt").unwrap();

        // Should show as staged Added.
        let st = repo.status().unwrap();
        let staged: Vec<_> = st.iter().filter(|e| e.staged).collect();
        assert_eq!(staged.len(), 1);
        assert_eq!(staged[0].status, FileStatus::Added);

        let sha = repo.commit("initial", "test").unwrap();
        assert!(!sha.is_empty());

        // After commit, status should be clean.
        let st = repo.status().unwrap();
        let relevant: Vec<_> = st
            .iter()
            .filter(|e| e.status != FileStatus::Untracked)
            .collect();
        assert!(relevant.is_empty(), "expected clean status after commit");
    }

    #[test]
    fn nothing_to_commit() {
        let mut repo = setup();
        let err = repo.commit("empty", "test").unwrap_err();
        assert_eq!(err, GitError::NothingToCommit);
    }

    #[test]
    fn diff_staged_shows_additions() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("f.txt", b"line1\nline2\n")
            .unwrap();
        repo.stage_file("f.txt").unwrap();

        let diffs = repo.diff_staged().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Added);
        assert!(!diffs[0].hunks.is_empty());
        assert!(diffs[0].hunks[0].lines.iter().all(|l| l.starts_with('+')));
    }

    #[test]
    fn diff_unstaged_shows_modifications() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("f.txt", b"line1\n")
            .unwrap();
        repo.stage_file("f.txt").unwrap();
        repo.commit("init", "test").unwrap();

        // Modify the file in the working tree.
        repo.filesystem_mut()
            .write_file("f.txt", b"line1\nline2\n")
            .unwrap();

        let diffs = repo.diff_unstaged().unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].status, FileStatus::Modified);
        assert!(!diffs[0].hunks.is_empty());
    }

    #[test]
    fn log_returns_commits_newest_first() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("a.txt", b"v1")
            .unwrap();
        repo.stage_file("a.txt").unwrap();
        repo.commit("first", "alice").unwrap();

        repo.filesystem_mut()
            .write_file("a.txt", b"v2")
            .unwrap();
        repo.stage_file("a.txt").unwrap();
        repo.commit("second", "bob").unwrap();

        let log = repo.log(10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].summary, "second");
        assert_eq!(log[0].author, "bob");
        assert_eq!(log[1].summary, "first");
        assert_eq!(log[1].author, "alice");
    }

    #[test]
    fn unstage_reverts_to_head() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("f.txt", b"original")
            .unwrap();
        repo.stage_file("f.txt").unwrap();
        repo.commit("init", "test").unwrap();

        // Modify and stage.
        repo.filesystem_mut()
            .write_file("f.txt", b"changed")
            .unwrap();
        repo.stage_file("f.txt").unwrap();

        // Staged diff should show a change.
        assert!(!repo.diff_staged().unwrap().is_empty());

        // Unstage should revert index to HEAD.
        repo.unstage_file("f.txt").unwrap();
        assert!(repo.diff_staged().unwrap().is_empty());
    }

    #[test]
    fn diff_commit_shows_changes() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("f.txt", b"v1\n")
            .unwrap();
        repo.stage_file("f.txt").unwrap();
        let sha1 = repo.commit("first", "test").unwrap();

        repo.filesystem_mut()
            .write_file("f.txt", b"v2\n")
            .unwrap();
        repo.stage_file("f.txt").unwrap();
        let sha2 = repo.commit("second", "test").unwrap();

        // First commit should show added file.
        let d1 = repo.diff_commit(&sha1).unwrap();
        assert_eq!(d1.len(), 1);
        assert_eq!(d1[0].status, FileStatus::Added);

        // Second commit should show modification.
        let d2 = repo.diff_commit(&sha2).unwrap();
        assert_eq!(d2.len(), 1);
        assert_eq!(d2[0].status, FileStatus::Modified);
    }

    #[test]
    fn diff_modified_produces_correct_hunks() {
        let hunks = diff_modified("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(hunks.len(), 1);
        let lines = &hunks[0].lines;
        assert!(lines.iter().any(|l| l.starts_with("-b")));
        assert!(lines.iter().any(|l| l.starts_with("+B")));
    }

    #[test]
    fn file_deletion_status() {
        let mut repo = setup();
        repo.filesystem_mut()
            .write_file("f.txt", b"data")
            .unwrap();
        repo.stage_file("f.txt").unwrap();
        repo.commit("add", "test").unwrap();

        // Delete in working tree.
        repo.filesystem_mut().remove_file("f.txt").unwrap();

        let st = repo.status().unwrap();
        let deleted: Vec<_> = st
            .iter()
            .filter(|e| e.status == FileStatus::Deleted)
            .collect();
        assert!(!deleted.is_empty());
    }
}
