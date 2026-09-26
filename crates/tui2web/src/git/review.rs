//! HEAD-anchored text selections for adapted Git review UIs.
use super::*;
use crate::fs::Snapshot;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Operation {
    Delete {
        line: usize,
        text: String,
    },
    Insert {
        at: usize,
        occurrence: usize,
        text: String,
    },
    EmptyFile {
        exists: bool,
    },
}

/// Opaque, exact identity of a HEAD-relative line change. Refresh after a stale-selection error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeId {
    path: String,
    generation: u64,
    operation: Operation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewSource {
    Worktree,
    IndexOnly,
}
#[derive(Debug, Clone)]
pub struct ReviewLine {
    pub text: String,
    pub change: Option<ChangeId>,
    pub staged: bool,
}
#[derive(Debug, Clone)]
pub struct ReviewHunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<ReviewLine>,
    pub source: ReviewSource,
}
#[derive(Debug, Clone)]
pub struct ReviewFile {
    pub path: String,
    pub status: FileStatus,
    pub hunks: Vec<ReviewHunk>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Plan {
    exists: bool,
    operations: Vec<Operation>,
}

/// Complete simulation state, not a real Git repository archive. Restore validates atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositorySnapshot {
    version: u32,
    filesystem: Snapshot,
    head: TreeSnapshot,
    index: TreeSnapshot,
    index_plans: BTreeMap<String, Plan>,
    commits: Vec<Commit>,
    next_id: u64,
}

fn text<'a>(bytes: Option<&'a Vec<u8>>, path: &str) -> Result<&'a str, GitError> {
    std::str::from_utf8(bytes.map_or(&[], Vec::as_slice))
        .map_err(|_| GitError::BinaryFile(path.into()))
}
fn tokens(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn plan(base: &str, target: &str, base_exists: bool, exists: bool) -> Plan {
    let old = tokens(base);
    let new = tokens(target);
    let script = if old.len().saturating_mul(new.len()) > 1_000_000 {
        (0..new.len())
            .map(|i| Edit::Insert(0, i))
            .chain((0..old.len()).map(|i| Edit::Delete(i, new.len())))
            .collect()
    } else {
        lcs_diff(&old, &new)
    };
    let mut occurrences = BTreeMap::new();
    let mut operations = Vec::new();
    for edit in script {
        match edit {
            Edit::Equal(..) => {}
            Edit::Delete(line, _) => operations.push(Operation::Delete {
                line,
                text: old[line].into(),
            }),
            Edit::Insert(at, new_line) => {
                let occurrence = occurrences.entry((at, new[new_line])).or_insert(0);
                operations.push(Operation::Insert {
                    at,
                    occurrence: *occurrence,
                    text: new[new_line].into(),
                });
                *occurrence += 1;
            }
        }
    }
    if operations.is_empty() && base_exists != exists {
        operations.push(Operation::EmptyFile { exists });
    }
    Plan { exists, operations }
}

fn apply(base: &str, plan: &Plan) -> Result<Option<Vec<u8>>, GitError> {
    let lines = tokens(base);
    let mut seen = BTreeSet::new();
    for op in &plan.operations {
        if !seen.insert(op) {
            return Err(GitError::Other("duplicate snapshot edit".into()));
        }
        match op {
            Operation::Delete { line, text }
                if lines.get(*line).copied() != Some(text.as_str()) =>
            {
                return Err(GitError::Other("invalid snapshot deletion".into()))
            }
            Operation::Insert { at, text, .. }
                if *at > lines.len() || text.is_empty() || tokens(text).len() != 1 =>
            {
                return Err(GitError::Other("invalid snapshot insertion".into()))
            }
            Operation::EmptyFile { exists }
                if !base.is_empty() || *exists != plan.exists || plan.operations.len() != 1 =>
            {
                return Err(GitError::Other("invalid empty-file edit".into()))
            }
            _ => {}
        }
    }
    let mut output = Vec::new();
    let mut inserts: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    let mut deletes = BTreeSet::new();
    for op in &plan.operations {
        match op {
            Operation::Insert { at, text, .. } => inserts.entry(*at).or_default().push(text),
            Operation::Delete { line, .. } => {
                deletes.insert(*line);
            }
            Operation::EmptyFile { .. } => {}
        }
    }
    for at in 0..=lines.len() {
        if let Some(insertions) = inserts.get(&at) {
            for insertion in insertions {
                output.extend_from_slice(insertion.as_bytes());
            }
        }
        if at < lines.len() && !deletes.contains(&at) {
            output.extend_from_slice(lines[at].as_bytes());
        }
        if output.len() > crate::fs::MAX_BYTES {
            return Err(GitError::Other("repository file quota exceeded".into()));
        }
    }
    if !plan.exists && !output.is_empty() {
        return Err(GitError::Other("absent file has content".into()));
    }
    Ok(plan.exists.then_some(output))
}

struct Row {
    old: usize,
    new: usize,
    text: String,
    operation: Option<Operation>,
}

fn rows(base: &str, plan: &Plan) -> Vec<Row> {
    let lines = tokens(base);
    let mut rows = Vec::new();
    let mut new = 0;
    let mut inserts: BTreeMap<usize, Vec<&Operation>> = BTreeMap::new();
    let mut deletes = BTreeMap::new();
    for operation in &plan.operations {
        match operation {
            Operation::Insert { at, .. } => inserts.entry(*at).or_default().push(operation),
            Operation::Delete { line, .. } => {
                deletes.insert(*line, operation);
            }
            Operation::EmptyFile { exists } => rows.push(Row {
                old: 0,
                new: 0,
                text: format!("{}[empty file]\n", if *exists { '+' } else { '-' }),
                operation: Some(operation.clone()),
            }),
        }
    }
    for old in 0..=lines.len() {
        if let Some(ops) = inserts.get(&old) {
            for op in ops {
                if let Operation::Insert { text, .. } = op {
                    rows.push(Row {
                        old,
                        new,
                        text: format!("+{text}"),
                        operation: Some((*op).clone()),
                    });
                    new += 1;
                }
            }
        }
        if old < lines.len() {
            let deletion = deletes.get(&old);
            rows.push(Row {
                old,
                new,
                text: format!(
                    "{}{line}",
                    if deletion.is_some() { '-' } else { ' ' },
                    line = lines[old]
                ),
                operation: deletion.map(|op| (*op).clone()),
            });
            if deletion.is_none() {
                new += 1;
            }
        }
    }
    rows
}

impl InMemoryGitRepository {
    fn work_plan(&self, path: &str) -> Result<Plan, GitError> {
        let work = self.working_tree();
        Ok(plan(
            text(self.head.get(path), path)?,
            text(work.get(path), path)?,
            self.head.contains_key(path),
            work.contains_key(path),
        ))
    }
    fn index_plan(&self, path: &str) -> Result<Plan, GitError> {
        if let Some(plan) = self.index_plans.get(path) {
            return Ok(plan.clone());
        }
        Ok(plan(
            text(self.head.get(path), path)?,
            text(self.index.get(path), path)?,
            self.head.contains_key(path),
            self.index.contains_key(path),
        ))
    }
    pub(super) fn record_index_plan(&mut self, path: &str) -> Result<(), GitError> {
        self.index_plans.remove(path);
        // Whole-file binary staging remains available; text review reports a typed error.
        if let (Ok(base), Ok(index)) = (
            text(self.head.get(path), path),
            text(self.index.get(path), path),
        ) {
            let plan = plan(
                base,
                index,
                self.head.contains_key(path),
                self.index.contains_key(path),
            );
            if self.head.get(path) != self.index.get(path) {
                self.index_plans.insert(path.into(), plan);
            }
        }
        Ok(())
    }
    fn id(&self, path: &str, operation: &Operation) -> ChangeId {
        ChangeId {
            path: path.into(),
            generation: self.next_id,
            operation: operation.clone(),
        }
    }
    fn hunks(
        &self,
        path: &str,
        plan: &Plan,
        index: &Plan,
        source: ReviewSource,
    ) -> Result<Vec<ReviewHunk>, GitError> {
        let rows = rows(text(self.head.get(path), path)?, plan);
        let mut spans: Vec<(usize, usize)> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if row.operation.is_none() {
                continue;
            }
            let start = i.saturating_sub(3);
            let end = (i + 4).min(rows.len());
            if let Some(previous) = spans.last_mut().filter(|p| p.1 >= start) {
                previous.1 = end;
            } else {
                spans.push((start, end));
            }
        }
        let final_op = plan.operations.last();
        Ok(spans
            .into_iter()
            .map(|(start, end)| {
                let section = &rows[start..end];
                let old_count = section.iter().filter(|r| !r.text.starts_with('+')).count();
                let new_count = section.iter().filter(|r| !r.text.starts_with('-')).count();
                ReviewHunk {
                    old_start: rows[start].old + usize::from(old_count != 0),
                    new_start: rows[start].new + usize::from(new_count != 0),
                    source,
                    lines: section
                        .iter()
                        .map(|row| {
                            let staged = row.operation.as_ref().is_some_and(|op| {
                                source == ReviewSource::IndexOnly
                                    || index.operations.contains(op)
                                        && (Some(op) != final_op || plan.exists == index.exists)
                            });
                            ReviewLine {
                                text: row.text.clone(),
                                change: row.operation.as_ref().map(|op| self.id(path, op)),
                                staged,
                            }
                        })
                        .collect(),
                }
            })
            .collect())
    }
    /// HEAD→worktree review plus separately tagged staged changes no longer in the worktree.
    /// Insertions retain HEAD-gap/content/occurrence identity as adjacent different lines change.
    pub fn review(&self) -> Result<Vec<ReviewFile>, GitError> {
        let work = self.working_tree();
        let paths: BTreeSet<_> = self
            .head
            .keys()
            .chain(work.keys())
            .chain(self.index.keys())
            .collect();
        let mut files = Vec::new();
        for path in paths {
            if self.head.get(path) == work.get(path) && self.head.get(path) == self.index.get(path)
            {
                continue;
            }
            let work_plan = self.work_plan(path)?;
            let index = self.index_plan(path)?;
            let mut hunks = self.hunks(path, &work_plan, &index, ReviewSource::Worktree)?;
            let extra = Plan {
                exists: index.exists,
                operations: index
                    .operations
                    .iter()
                    .filter(|op| !work_plan.operations.contains(op))
                    .cloned()
                    .collect(),
            };
            hunks.extend(self.hunks(path, &extra, &index, ReviewSource::IndexOnly)?);
            let status = if !self.head.contains_key(path) {
                FileStatus::Added
            } else if !work.contains_key(path) {
                FileStatus::Deleted
            } else {
                FileStatus::Modified
            };
            files.push(ReviewFile {
                path: path.clone(),
                status,
                hunks,
            });
        }
        Ok(files)
    }
    fn validate_id(&self, id: &ChangeId, in_work: bool) -> Result<Plan, GitError> {
        if id.generation != self.next_id {
            return Err(GitError::StaleSelection);
        }
        let plan = if in_work {
            self.work_plan(&id.path)?
        } else {
            self.index_plan(&id.path)?
        };
        if !plan.operations.contains(&id.operation) {
            return Err(GitError::StaleSelection);
        }
        Ok(plan)
    }
    fn set_index_plan(&mut self, path: &str, plan: Plan) -> Result<(), GitError> {
        let value = apply(text(self.head.get(path), path)?, &plan)?;
        let mut next = self.index.clone();
        match value {
            Some(bytes) => {
                next.insert(path.into(), bytes);
            }
            None => {
                next.remove(path);
            }
        }
        validate_tree(&next)?;
        self.index = next;
        if self.index.get(path) == self.head.get(path) {
            self.index_plans.remove(path);
        } else {
            self.index_plans.insert(path.into(), plan);
        }
        Ok(())
    }
    pub fn stage_change(&mut self, id: &ChangeId) -> Result<(), GitError> {
        let work = self.validate_id(id, true)?;
        if matches!(id.operation, Operation::EmptyFile { .. }) {
            return self.set_index_plan(&id.path, work);
        }
        let mut index = self.index_plan(&id.path)?;
        index
            .operations
            .retain(|op| !matches!(op, Operation::EmptyFile { .. }));
        if let Operation::Insert { at, .. } = &id.operation {
            index.operations.retain(|op| {
                !matches!(op, Operation::Insert { at: gap, .. } if gap == at)
                    || work.operations.contains(op)
            });
        }
        if !index.operations.contains(&id.operation) {
            index.operations.push(id.operation.clone());
        }
        if let Operation::Insert { at, .. } = &id.operation {
            let selected = index.operations.clone();
            index
                .operations
                .retain(|op| !matches!(op, Operation::Insert { at: gap, .. } if gap == at));
            index.operations.extend(
                work.operations
                    .iter()
                    .filter(|op| {
                        matches!(op, Operation::Insert { at: gap, .. } if gap == at)
                            && selected.contains(op)
                    })
                    .cloned(),
            );
        }
        index.exists = true;
        let bytes = apply(text(self.head.get(&id.path), &id.path)?, &index)?.unwrap_or_default();
        index.exists = !bytes.is_empty() || work.exists;
        self.set_index_plan(&id.path, index)
    }
    pub fn unstage_change(&mut self, id: &ChangeId) -> Result<(), GitError> {
        let mut index = self.validate_id(id, false)?;
        index.operations.retain(|op| op != &id.operation);
        index.exists = true;
        let bytes = apply(text(self.head.get(&id.path), &id.path)?, &index)?.unwrap_or_default();
        index.exists = !bytes.is_empty() || self.head.contains_key(&id.path);
        self.set_index_plan(&id.path, index)
    }
    /// Atomic toggle: any unstaged selected line stages the hunk; otherwise unstage all selections.
    pub fn toggle_hunk(&mut self, hunk: &ReviewHunk) -> Result<bool, GitError> {
        let ids: Vec<_> = hunk
            .lines
            .iter()
            .filter_map(|line| line.change.as_ref())
            .collect();
        if ids.is_empty() {
            return Err(GitError::StaleSelection);
        }
        let current = self.review()?;
        let states: Vec<_> = ids
            .iter()
            .map(|id| {
                current
                    .iter()
                    .flat_map(|f| &f.hunks)
                    .filter(|h| h.source == hunk.source)
                    .flat_map(|h| &h.lines)
                    .find(|l| l.change.as_ref() == Some(*id))
                    .map(|l| l.staged)
                    .ok_or(GitError::StaleSelection)
            })
            .collect::<Result<_, _>>()?;
        let stage = !states.iter().all(|s| *s);
        let mut next = self.clone();
        for id in ids {
            if stage {
                next.stage_change(id)?;
            } else {
                next.unstage_change(id)?;
            }
        }
        *self = next;
        Ok(stage)
    }
    /// Reverse one HEAD→worktree edit without changing the index.
    pub fn discard_change(&mut self, id: &ChangeId) -> Result<(), GitError> {
        let mut work = self.validate_id(id, true)?;
        work.operations.retain(|op| op != &id.operation);
        work.exists = true;
        let bytes = apply(text(self.head.get(&id.path), &id.path)?, &work)?.unwrap_or_default();
        work.exists = !bytes.is_empty() || self.head.contains_key(&id.path);
        let value = apply(text(self.head.get(&id.path), &id.path)?, &work)?;
        let mut fs = self.fs.clone();
        match value {
            Some(bytes) => {
                if let Some((parent, _)) = id.path.rsplit_once('/') {
                    fs.create_dir_all(parent)
                        .map_err(|e| GitError::Other(e.to_string()))?;
                }
                fs.write_file(&id.path, &bytes)
                    .map_err(|e| GitError::Other(e.to_string()))?;
            }
            None => {
                if fs.is_file(&id.path) {
                    fs.remove_file(&id.path)
                        .map_err(|e| GitError::Other(e.to_string()))?;
                }
            }
        }
        self.fs = fs;
        Ok(())
    }
    pub fn snapshot(&self) -> RepositorySnapshot {
        RepositorySnapshot {
            version: 1,
            filesystem: self.fs.snapshot(),
            head: self.head.clone(),
            index: self.index.clone(),
            index_plans: self.index_plans.clone(),
            commits: self.commits.clone(),
            next_id: self.next_id,
        }
    }
    pub fn restore(&mut self, snapshot: RepositorySnapshot) -> Result<(), GitError> {
        if snapshot.version != 1
            || snapshot.commits.len() > 64
            || snapshot.next_id != snapshot.commits.len() as u64 + 1
        {
            return Err(GitError::Other(
                "invalid repository snapshot version/history".into(),
            ));
        }
        let encoded = serde_json::to_vec(&snapshot).map_err(|e| GitError::Other(e.to_string()))?;
        if encoded.len() > 16 * 1024 * 1024 {
            return Err(GitError::Other("repository snapshot exceeds 16 MiB".into()));
        }
        let mut fs = MemoryFilesystem::new();
        fs.restore(snapshot.filesystem)
            .map_err(|e| GitError::Other(e.to_string()))?;
        validate_tree(&snapshot.head)?;
        validate_tree(&snapshot.index)?;
        for (i, commit) in snapshot.commits.iter().enumerate() {
            if commit.sha != format!("{:016x}", i + 1) {
                return Err(GitError::Other("invalid commit identifier".into()));
            }
            validate_tree(&commit.tree)?;
        }
        if snapshot
            .commits
            .last()
            .map(|c| &c.tree)
            .unwrap_or(&BTreeMap::new())
            != &snapshot.head
        {
            return Err(GitError::Other("HEAD does not match history".into()));
        }
        let next = Self {
            fs,
            head: snapshot.head,
            index: snapshot.index,
            index_plans: snapshot.index_plans,
            commits: snapshot.commits,
            next_id: snapshot.next_id,
        };
        for (path, plan) in &next.index_plans {
            if next.head.get(path) == next.index.get(path) {
                return Err(GitError::Other(
                    "redundant or unknown index provenance path".into(),
                ));
            }
            let value = apply(text(next.head.get(path), path)?, plan)?;
            if value.as_ref() != next.index.get(path) {
                return Err(GitError::Other("index edit provenance mismatch".into()));
            }
        }
        *self = next;
        Ok(())
    }
}

pub(super) fn validate_tree(tree: &TreeSnapshot) -> Result<(), GitError> {
    let mut fs = MemoryFilesystem::new();
    for (path, bytes) in tree {
        if let Some((parent, _)) = path.rsplit_once('/') {
            fs.create_dir_all(parent)
                .map_err(|e| GitError::Other(e.to_string()))?;
        }
        fs.write_file(path, bytes)
            .map_err(|e| GitError::Other(e.to_string()))?;
    }
    if fs.list_files() != tree.keys().cloned().collect::<Vec<_>>() {
        return Err(GitError::Other("noncanonical repository path".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repo(head: &str, work: &str) -> InMemoryGitRepository {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir_all("empty/child").unwrap();
        fs.write_file("file", head.as_bytes()).unwrap();
        let mut repo = InMemoryGitRepository::new(fs);
        repo.stage_file("file").unwrap();
        repo.commit("fixture", "Test").unwrap();
        repo.filesystem_mut()
            .write_file("file", work.as_bytes())
            .unwrap();
        repo
    }
    fn hunk(repo: &InMemoryGitRepository, source: ReviewSource) -> ReviewHunk {
        repo.review()
            .unwrap()
            .into_iter()
            .flat_map(|f| f.hunks)
            .find(|h| h.source == source)
            .unwrap()
    }
    fn selection(repo: &InMemoryGitRepository, text: &str, source: ReviewSource) -> ChangeId {
        repo.review()
            .unwrap()
            .into_iter()
            .flat_map(|f| f.hunks)
            .filter(|h| h.source == source)
            .flat_map(|h| h.lines)
            .find(|line| line.text == text)
            .unwrap()
            .change
            .unwrap()
    }
    fn staged(repo: &InMemoryGitRepository, text: &str) -> bool {
        hunk(repo, ReviewSource::Worktree)
            .lines
            .iter()
            .find(|line| line.text == text)
            .unwrap()
            .staged
    }
    #[test]
    fn hunk_toggle_roundtrips_replacements_and_final_newline() {
        for (head, work) in [
            ("one\nold\nthree\n", "one\nnew\nthree\n"),
            ("one\nold", "one\nnew"),
        ] {
            let mut repo = repo(head, work);
            let current = hunk(&repo, ReviewSource::Worktree);
            assert!(repo.toggle_hunk(&current).unwrap());
            assert_eq!(repo.index["file"], work.as_bytes());
            assert!(!repo.toggle_hunk(&current).unwrap());
            assert_eq!(repo.index["file"], head.as_bytes());
        }
    }
    #[test]
    fn partial_insertion_identity_survives_adjacent_worktree_edits() {
        let mut repo = repo("a\nb\n", "a\nx\ny\nb\n");
        let y = selection(&repo, "+y\n", ReviewSource::Worktree);
        repo.stage_change(&y).unwrap();
        assert_eq!(repo.index["file"], b"a\ny\nb\n");
        assert!(staged(&repo, "+y\n"));
        assert!(!staged(&repo, "+x\n"));
        repo.filesystem_mut()
            .write_file("file", b"a\nz\nx\ny\nb\n")
            .unwrap();
        assert_eq!(selection(&repo, "+y\n", ReviewSource::Worktree), y);
        assert!(staged(&repo, "+y\n"));
        let x = selection(&repo, "+x\n", ReviewSource::Worktree);
        repo.stage_change(&x).unwrap();
        assert_eq!(repo.index["file"], b"a\nx\ny\nb\n");
        repo.filesystem_mut()
            .write_file("file", b"a\nz\nx\nY\nb\n")
            .unwrap();
        assert!(hunk(&repo, ReviewSource::IndexOnly)
            .lines
            .iter()
            .any(|l| l.text == "+y\n" && l.staged));
        let replacement = selection(&repo, "+Y\n", ReviewSource::Worktree);
        repo.stage_change(&replacement).unwrap();
        assert_eq!(repo.index["file"], b"a\nx\nY\nb\n");
        repo.unstage_change(&x).unwrap();
        assert_eq!(repo.index["file"], b"a\nY\nb\n");
    }
    #[test]
    fn duplicate_insertions_retain_the_selected_occurrence() {
        let mut repo = repo("a\nb\n", "a\nsame\nsame\nb\n");
        let additions: Vec<_> = hunk(&repo, ReviewSource::Worktree)
            .lines
            .into_iter()
            .filter(|l| l.text == "+same\n")
            .collect();
        repo.stage_change(additions[1].change.as_ref().unwrap())
            .unwrap();
        let flags: Vec<_> = hunk(&repo, ReviewSource::Worktree)
            .lines
            .into_iter()
            .filter(|l| l.text == "+same\n")
            .map(|l| l.staged)
            .collect();
        assert_eq!(flags, vec![false, true]);
        let snapshot = repo.snapshot();
        let mut restored = InMemoryGitRepository::new(MemoryFilesystem::new());
        restored.restore(snapshot).unwrap();
        restored
            .unstage_change(additions[1].change.as_ref().unwrap())
            .unwrap();
        assert_eq!(restored.index["file"], b"a\nb\n");
    }
    #[test]
    fn reverted_worktree_keeps_index_only_edits_reviewable() {
        let mut repo = repo("a\nold\n", "a\nnew\n");
        repo.stage_file("file").unwrap();
        repo.filesystem_mut()
            .write_file("file", b"a\nold\n")
            .unwrap();
        let files = repo.review().unwrap();
        assert!(files[0]
            .hunks
            .iter()
            .all(|h| h.source == ReviewSource::IndexOnly));
        let index_only = hunk(&repo, ReviewSource::IndexOnly);
        assert!(!repo.toggle_hunk(&index_only).unwrap());
        assert!(repo.review().unwrap().is_empty());
    }
    #[test]
    fn discard_preserves_index_and_stale_actions_are_atomic() {
        let mut repo = repo("a\nb\n", "a\nx\nb\n");
        let change = selection(&repo, "+x\n", ReviewSource::Worktree);
        repo.stage_change(&change).unwrap();
        repo.discard_change(&change).unwrap();
        assert_eq!(repo.fs.read_file("file").unwrap(), b"a\nb\n");
        assert_eq!(repo.index["file"], b"a\nx\nb\n");
        assert!(matches!(
            repo.stage_change(&change),
            Err(GitError::StaleSelection)
        ));
        repo.commit("staged", "Test").unwrap();
        assert!(matches!(
            repo.unstage_change(&change),
            Err(GitError::StaleSelection)
        ));
        let before = serde_json::to_value(repo.snapshot()).unwrap();
        let hunk = hunk(&repo, ReviewSource::Worktree);
        repo.filesystem_mut()
            .write_file("file", b"a\nx\nb\n")
            .unwrap();
        let after_edit = serde_json::to_value(repo.snapshot()).unwrap();
        assert!(repo.toggle_hunk(&hunk).is_err());
        assert_eq!(serde_json::to_value(repo.snapshot()).unwrap(), after_edit);
        assert_ne!(before, after_edit);
    }
    #[test]
    fn added_deleted_and_empty_files_have_correct_presence() {
        let mut repo = InMemoryGitRepository::new(MemoryFilesystem::new());
        for content in ["", "new\n"] {
            repo.fs.write_file("file", content.as_bytes()).unwrap();
            let current = hunk(&repo, ReviewSource::Worktree);
            assert!(repo.toggle_hunk(&current).unwrap());
            assert_eq!(repo.index["file"], content.as_bytes());
            assert!(!repo.toggle_hunk(&current).unwrap());
            assert!(!repo.index.contains_key("file"));
        }
        repo.stage_file("file").unwrap();
        repo.commit("added", "Test").unwrap();
        repo.fs.remove_file("file").unwrap();
        let deleted = hunk(&repo, ReviewSource::Worktree);
        repo.toggle_hunk(&deleted).unwrap();
        assert!(!repo.index.contains_key("file"));
        repo.toggle_hunk(&deleted).unwrap();
        assert_eq!(repo.index["file"], b"new\n");
        let mut empty = super::tests::repo("", "");
        empty.fs.remove_file("file").unwrap();
        let deleted = hunk(&empty, ReviewSource::Worktree);
        empty.toggle_hunk(&deleted).unwrap();
        assert!(!empty.index.contains_key("file"));
        empty.toggle_hunk(&deleted).unwrap();
        assert_eq!(empty.index["file"], b"");
    }
    #[test]
    fn complete_snapshot_roundtrip_and_rejection_are_atomic() {
        let mut original = repo("head\n", "work\n");
        let insert = selection(&original, "+work\n", ReviewSource::Worktree);
        original.stage_change(&insert).unwrap();
        let snapshot = serde_json::to_value(original.snapshot()).unwrap();
        let mut restored = InMemoryGitRepository::new(MemoryFilesystem::new());
        restored
            .restore(serde_json::from_value(snapshot.clone()).unwrap())
            .unwrap();
        assert_eq!(serde_json::to_value(restored.snapshot()).unwrap(), snapshot);
        assert!(restored.fs.is_dir("empty/child"));
        for field in ["version", "next_id"] {
            let mut bad = snapshot.clone();
            bad[field] = 999.into();
            assert!(restored
                .restore(serde_json::from_value(bad).unwrap())
                .is_err());
        }
        let mut bad = snapshot.clone();
        bad["index"]["file"] = serde_json::json!([42]);
        assert!(restored
            .restore(serde_json::from_value(bad).unwrap())
            .is_err());
        assert_eq!(serde_json::to_value(restored.snapshot()).unwrap(), snapshot);
    }
}
