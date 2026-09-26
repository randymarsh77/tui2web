# Optional Git simulation

[Project overview](../README.md) · [Virtual filesystem](filesystem.md)

Enable the `git` Cargo feature to use `InMemoryGitRepository`. It provides linear in-memory history, counter-based identifiers, and text diffs. It does not implement Git objects, hashes, branches, remotes, `.git` compatibility, or transport. The core runtime and file editor do not need it.

## Trees and review

The repository owns separate HEAD, index, and working trees. `filesystem_mut()` changes only the working filesystem; `Clone` makes a deep copy.

`GitRepository` supports whole-file stage/unstage, index-to-worktree and HEAD-to-index diffs, commits, and history. `review()` returns HEAD-to-worktree `ReviewFile`, `ReviewHunk`, and `ReviewLine` values with three context lines, staged flags, and opaque `ChangeId` selections.

`ReviewSource::IndexOnly` exposes staged edits absent from the working tree, including when the worktree has reverted to HEAD. Include the source in UI hunk and cache identities.

## Partial selections

`stage_change`, `unstage_change`, `discard_change`, and atomic `toggle_hunk` use IDs rather than display offsets. IDs anchor deletions to HEAD lines and insertions to HEAD gaps, text, and duplicate occurrences. Index provenance preserves partial selections.

Adjacent distinct insertions do not invalidate existing insertion IDs. Reordered identical lines use deterministic occurrence identities. Changing HEAD or removing/replacing an operation makes its selection stale and causes rejection.

Staging an insertion supersedes obsolete index-only insertions at the same HEAD gap while preserving staged groups elsewhere. Discard reverses a selected HEAD-to-worktree edit without changing the index. These are simulation semantics, not libgit2 patch behavior.

## File formats and limits

Line review requires UTF-8 and preserves LF and missing-final-LF data. Empty-file additions and removals use a selectable `[empty file]` metadata line.

Whole-file operations support binary bytes. Text review of a changed binary file returns `GitError::BinaryFile`. LCS tables are bounded; large diffs use replacement edits.

## Repository snapshots

`RepositorySnapshot` version 1 stores the working filesystem, including empty directories, HEAD, index, selection provenance, linear history, and counter. It round-trips through serde. Restore validates atomically, including history/HEAD consistency and edit provenance.

Each tree has the [filesystem quotas](filesystem.md#snapshots). Repository snapshots are limited to 16 MiB serialized JSON and 64 commits. Commits exceeding those limits fail without mutation.

Repository snapshots are separate from runtime filesystem snapshots. Apps must serialize repository state into virtual files or provide an explicit restore policy; the runtime does not persist it automatically.
