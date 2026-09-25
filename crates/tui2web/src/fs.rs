//! Synchronous virtual files; no interception of `std::fs`. Snapshots are portable JSON.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_PATH_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    NotFound(String),
    AlreadyExists(String),
    ParentNotFound(String),
    NotEmpty(String),
    WrongKind(String),
    InvalidPath(String),
    InvalidEncoding(String),
    InvalidSnapshot(String),
    LimitExceeded,
}
impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FsError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub is_dir: bool,
    pub len: u64,
}

/// Absolute and relative paths both start at the virtual root. `..` above root is invalid.
/// Renames never overwrite an existing destination. Boolean probes return false for invalid paths;
/// use `metadata` when callers need the typed error.
pub trait Filesystem {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError>;
    fn read_to_string(&self, path: &str) -> Result<String, FsError> {
        String::from_utf8(self.read_file(path)?).map_err(|_| FsError::InvalidEncoding(path.into()))
    }
    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), FsError>;
    fn remove_file(&mut self, path: &str) -> Result<(), FsError>;
    fn remove_dir(&mut self, path: &str) -> Result<(), FsError>;
    fn exists(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
    fn is_file(&self, path: &str) -> bool;
    fn create_dir(&mut self, path: &str) -> Result<(), FsError>;
    fn create_dir_all(&mut self, path: &str) -> Result<(), FsError>;
    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError>;
    fn metadata(&self, path: &str) -> Result<Metadata, FsError>;
    fn list_files(&self) -> Vec<String>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    /// Canonical relative paths; includes the root as `""`, including empty directories.
    pub directories: Vec<String>,
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub struct MemoryFilesystem {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
}
impl Default for MemoryFilesystem {
    fn default() -> Self {
        Self::new()
    }
}
impl MemoryFilesystem {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            dirs: BTreeSet::from([String::new()]),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: 1,
            directories: self.dirs.iter().cloned().collect(),
            files: self
                .files
                .iter()
                .map(|(p, b)| (p.clone(), b.clone()))
                .collect(),
        }
    }
    /// Validate completely before replacing any state.
    pub fn restore(&mut self, snapshot: Snapshot) -> Result<(), FsError> {
        if snapshot.version != 1 {
            return Err(FsError::InvalidSnapshot("unsupported version".into()));
        }
        if snapshot.directories.len() + snapshot.files.len() > MAX_ENTRIES {
            return Err(FsError::LimitExceeded);
        }
        let mut next = Self {
            files: BTreeMap::new(),
            dirs: BTreeSet::new(),
        };
        for path in snapshot.directories {
            if normalise(&path)? != path || !next.dirs.insert(path) {
                return Err(FsError::InvalidSnapshot(
                    "noncanonical or duplicate directory".into(),
                ));
            }
        }
        for (path, bytes) in snapshot.files {
            if normalise(&path)? != path
                || next.dirs.contains(&path)
                || next.files.insert(path, bytes).is_some()
            {
                return Err(FsError::InvalidSnapshot(
                    "noncanonical, duplicate or conflicting file".into(),
                ));
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn validate(&self) -> Result<(), FsError> {
        if self.files.len() + self.dirs.len() > MAX_ENTRIES
            || self.files.values().map(Vec::len).sum::<usize>() > MAX_BYTES
        {
            return Err(FsError::LimitExceeded);
        }
        if !self.dirs.contains("") {
            return Err(FsError::InvalidSnapshot("missing root".into()));
        }
        for path in self.files.keys().chain(self.dirs.iter()) {
            if path.len() > MAX_PATH_BYTES {
                return Err(FsError::LimitExceeded);
            }
            if !path.is_empty() && !self.dirs.contains(parent(path)) {
                return Err(FsError::ParentNotFound(path.clone()));
            }
        }
        Ok(())
    }
    fn require_dir(&self, path: &str) -> Result<(), FsError> {
        if self.dirs.contains(path) {
            Ok(())
        } else if self.files.contains_key(path) {
            Err(FsError::WrongKind(path.into()))
        } else {
            Err(FsError::NotFound(path.into()))
        }
    }
}

fn normalise(path: &str) -> Result<String, FsError> {
    if path.len() > MAX_PATH_BYTES || path.chars().any(|c| c.is_control() || c == '\\') {
        return Err(FsError::InvalidPath(path.into()));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| FsError::InvalidPath(path.into()))?;
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

impl Filesystem for MemoryFilesystem {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError> {
        let p = normalise(path)?;
        if self.dirs.contains(&p) {
            return Err(FsError::WrongKind(p));
        }
        self.files.get(&p).cloned().ok_or(FsError::NotFound(p))
    }
    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        let p = normalise(path)?;
        if self.dirs.contains(&p) {
            return Err(FsError::WrongKind(p));
        }
        if !self.dirs.contains(parent(&p)) {
            return Err(FsError::ParentNotFound(p));
        }
        let used = self.files.values().map(Vec::len).sum::<usize>();
        let old = self.files.get(&p).map_or(0, Vec::len);
        if content.len() > MAX_BYTES
            || used - old + content.len() > MAX_BYTES
            || (!self.files.contains_key(&p) && self.files.len() + self.dirs.len() >= MAX_ENTRIES)
        {
            return Err(FsError::LimitExceeded);
        }
        self.files.insert(p, content.to_vec());
        Ok(())
    }
    fn remove_file(&mut self, path: &str) -> Result<(), FsError> {
        let p = normalise(path)?;
        if self.dirs.contains(&p) {
            return Err(FsError::WrongKind(p));
        }
        self.files
            .remove(&p)
            .map(|_| ())
            .ok_or(FsError::NotFound(p))
    }
    fn remove_dir(&mut self, path: &str) -> Result<(), FsError> {
        let p = normalise(path)?;
        if p.is_empty() {
            return Err(FsError::InvalidPath(p));
        }
        self.require_dir(&p)?;
        if !self.read_dir(&p)?.is_empty() {
            return Err(FsError::NotEmpty(p));
        }
        self.dirs.remove(&p);
        Ok(())
    }
    fn exists(&self, path: &str) -> bool {
        self.metadata(path).is_ok()
    }
    fn is_dir(&self, path: &str) -> bool {
        self.metadata(path).is_ok_and(|m| m.is_dir)
    }
    fn is_file(&self, path: &str) -> bool {
        self.metadata(path).is_ok_and(|m| !m.is_dir)
    }
    fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let p = normalise(path)?;
        if self.exists(&p) {
            return Err(FsError::AlreadyExists(p));
        }
        if !self.dirs.contains(parent(&p)) {
            return Err(FsError::ParentNotFound(p));
        }
        if self.dirs.len() + self.files.len() >= MAX_ENTRIES {
            return Err(FsError::LimitExceeded);
        }
        self.dirs.insert(p);
        Ok(())
    }
    fn create_dir_all(&mut self, path: &str) -> Result<(), FsError> {
        let p = normalise(path)?;
        let mut next = self.clone();
        let mut current = String::new();
        for part in p.split('/').filter(|s| !s.is_empty()) {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(part);
            if next.files.contains_key(&current) {
                return Err(FsError::WrongKind(current));
            }
            next.dirs.insert(current.clone());
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let p = normalise(path)?;
        self.require_dir(&p)?;
        let mut entries = Vec::new();
        for (paths, is_dir) in [
            (self.files.keys().collect::<Vec<_>>(), false),
            (self.dirs.iter().collect::<Vec<_>>(), true),
        ] {
            for child in paths {
                if !child.is_empty() && parent(child) == p {
                    entries.push(DirEntry {
                        name: child.rsplit('/').next().unwrap().into(),
                        is_dir,
                    });
                }
            }
        }
        entries.sort();
        Ok(entries)
    }
    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let p = normalise(path)?;
        if self.dirs.contains(&p) {
            Ok(Metadata {
                is_dir: true,
                len: 0,
            })
        } else if let Some(b) = self.files.get(&p) {
            Ok(Metadata {
                is_dir: false,
                len: b.len() as u64,
            })
        } else {
            Err(FsError::NotFound(p))
        }
    }
    fn list_files(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
        let from = normalise(from)?;
        let to = normalise(to)?;
        if from.is_empty() || to.is_empty() {
            return Err(FsError::InvalidPath("root rename".into()));
        }
        self.metadata(&from)?;
        if from == to {
            return Ok(());
        }
        if to.starts_with(&format!("{from}/")) {
            return Err(FsError::InvalidPath(to));
        }
        if self.exists(&to) {
            return Err(FsError::AlreadyExists(to));
        }
        if !self.dirs.contains(parent(&to)) {
            return Err(FsError::ParentNotFound(to));
        }
        let prefix = format!("{from}/");
        let move_path = |p: &String| {
            if p == &from {
                to.clone()
            } else if let Some(rest) = p.strip_prefix(&prefix) {
                format!("{to}/{rest}")
            } else {
                p.clone()
            }
        };
        let next = Self {
            files: self
                .files
                .iter()
                .map(|(p, b)| (move_path(p), b.clone()))
                .collect(),
            dirs: self.dirs.iter().map(move_path).collect(),
        };
        next.validate()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_encoding() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir_all("/a//./b/..").unwrap();
        fs.write_file("a/./file", &[255]).unwrap();
        assert!(matches!(
            fs.read_to_string("/a/file"),
            Err(FsError::InvalidEncoding(_))
        ));
        assert!(matches!(fs.metadata("../a"), Err(FsError::InvalidPath(_))));
        assert!(fs.write_file("bad\0path", b"").is_err());
        assert!(fs.read_file("a").is_err());
        assert!(fs.remove_dir("/").is_err());
    }
    #[test]
    fn atomic_renames_and_direct_children() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir_all("a/b/empty").unwrap();
        fs.write_file("a/b/file", b"ok").unwrap();
        assert_eq!(
            fs.read_dir("a").unwrap(),
            vec![DirEntry {
                name: "b".into(),
                is_dir: true
            }]
        );
        let before = fs.snapshot();
        for (a, b) in [
            ("/", "root"),
            ("a", "a/b/c"),
            ("a", "missing/c"),
            ("a/b", "a"),
        ] {
            assert!(fs.rename(a, b).is_err());
            assert_eq!(before, fs.snapshot());
        }
        fs.rename("a", "moved").unwrap();
        assert_eq!(fs.read_file("moved/b/file").unwrap(), b"ok");
        assert!(fs.is_dir("moved/b/empty"));
    }
    #[test]
    fn snapshots_are_complete_validated_and_atomic() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir_all("empty/nested").unwrap();
        fs.write_file("binary", &[0, 255]).unwrap();
        let before = fs.snapshot();
        fs.restore(serde_json::from_str(&serde_json::to_string(&before).unwrap()).unwrap())
            .unwrap();
        assert_eq!(fs.snapshot(), before);
        let mut bad = before.clone();
        bad.files.push(("binary".into(), vec![]));
        assert!(fs.restore(bad).is_err());
        let mut bad = before.clone();
        bad.directories.remove(0);
        assert!(fs.restore(bad).is_err());
        let mut bad = before.clone();
        bad.files.push(("x/y".into(), vec![]));
        assert!(fs.restore(bad).is_err());
        assert_eq!(fs.snapshot(), before);
        assert!(fs.write_file("large", &vec![0; MAX_BYTES + 1]).is_err());
        assert_eq!(fs.snapshot(), before);
    }
}
