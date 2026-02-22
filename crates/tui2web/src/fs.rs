//! Virtual filesystem abstraction for running TUI applications in the browser.
//!
//! Provides a [`Filesystem`] trait that abstracts file operations, plus an
//! [`MemoryFilesystem`] implementation backed by in-memory storage.  When
//! running under WebAssembly the memory filesystem can optionally be
//! persisted to `localStorage` via the JavaScript bridge in `web/main.js`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// ── Error types ──────────────────────────────────────────────────────────────

/// Errors produced by [`Filesystem`] operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    /// The requested path was not found.
    NotFound(String),
    /// The path already exists.
    AlreadyExists(String),
    /// A parent directory in the path does not exist.
    ParentNotFound(String),
    /// The operation expected a file but found a directory, or vice-versa.
    WrongKind(String),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::NotFound(p) => write!(f, "not found: {p}"),
            FsError::AlreadyExists(p) => write!(f, "already exists: {p}"),
            FsError::ParentNotFound(p) => write!(f, "parent directory not found: {p}"),
            FsError::WrongKind(p) => write!(f, "wrong kind: {p}"),
        }
    }
}

impl std::error::Error for FsError {}

// ── Data types ───────────────────────────────────────────────────────────────

/// Entry returned by [`Filesystem::read_dir`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Name of the entry (not the full path).
    pub name: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
}

/// Metadata about a file or directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// `true` when the path is a directory.
    pub is_dir: bool,
    /// Size in bytes (always 0 for directories).
    pub len: u64,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Abstraction over filesystem operations.
///
/// Implementations must treat paths as forward-slash separated, UTF-8 strings.
/// A leading `/` is optional; paths are normalised internally.
pub trait Filesystem {
    /// Read the entire contents of a file.
    fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError>;

    /// Read a file as a UTF-8 string (convenience wrapper).
    fn read_to_string(&self, path: &str) -> Result<String, FsError> {
        let bytes = self.read_file(path)?;
        String::from_utf8(bytes).map_err(|_| FsError::WrongKind(path.to_string()))
    }

    /// Create or overwrite a file with the given contents.
    /// Parent directories must already exist.
    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), FsError>;

    /// Remove a file.  Returns an error if the path is a directory or does not exist.
    fn remove_file(&mut self, path: &str) -> Result<(), FsError>;

    /// Remove a directory.  Returns an error if the directory is not empty.
    fn remove_dir(&mut self, path: &str) -> Result<(), FsError>;

    /// Check whether a path exists (file or directory).
    fn exists(&self, path: &str) -> bool;

    /// Check whether a path is a directory.
    fn is_dir(&self, path: &str) -> bool;

    /// Check whether a path is a file.
    fn is_file(&self, path: &str) -> bool;

    /// Create a single directory.  The parent must already exist.
    fn create_dir(&mut self, path: &str) -> Result<(), FsError>;

    /// Recursively create a directory and all missing parents.
    fn create_dir_all(&mut self, path: &str) -> Result<(), FsError>;

    /// List the immediate children of a directory.
    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError>;

    /// Return metadata for a path.
    fn metadata(&self, path: &str) -> Result<Metadata, FsError>;

    /// List every file path in the filesystem (non-recursive convenience).
    fn list_files(&self) -> Vec<String>;

    /// Rename / move a file or directory.
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError>;
}

// ── In-memory implementation ─────────────────────────────────────────────────

/// A fully in-memory [`Filesystem`].
///
/// Files are stored in a sorted map keyed by normalised path, and directories
/// are tracked separately so that empty directories are preserved.
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
    /// Create a new, empty filesystem.  The root directory `/` is created
    /// implicitly.
    pub fn new() -> Self {
        let mut dirs = BTreeSet::new();
        dirs.insert(String::new()); // root
        MemoryFilesystem {
            files: BTreeMap::new(),
            dirs,
        }
    }

    /// Serialise the entire filesystem to a flat `Vec` of `(path, contents)`
    /// pairs.  Useful for persisting to `localStorage`.
    pub fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        self.files.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Restore the filesystem from a snapshot created by [`snapshot`].
    pub fn restore(&mut self, entries: Vec<(String, Vec<u8>)>) {
        self.files.clear();
        self.dirs.clear();
        self.dirs.insert(String::new()); // root

        for (path, content) in entries {
            let norm = normalise(&path);
            // Ensure all parent directories exist.
            let mut prefix = String::new();
            for part in norm.split('/') {
                if !prefix.is_empty() || !part.is_empty() {
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(part);
                }
                // Don't insert the file itself as a dir.
                if prefix != norm {
                    self.dirs.insert(prefix.clone());
                }
            }
            self.files.insert(norm, content);
        }
    }
}

/// Normalise a path: strip leading `/`, collapse duplicate `/`.
fn normalise(path: &str) -> String {
    path.trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Return the parent of a normalised path (empty string = root).
fn parent(path: &str) -> Option<String> {
    if path.is_empty() {
        return None; // root has no parent
    }
    match path.rfind('/') {
        Some(pos) => Some(path[..pos].to_string()),
        None => Some(String::new()), // parent is root
    }
}

impl Filesystem for MemoryFilesystem {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, FsError> {
        let norm = normalise(path);
        self.files
            .get(&norm)
            .cloned()
            .ok_or_else(|| FsError::NotFound(norm))
    }

    fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), FsError> {
        let norm = normalise(path);
        if self.dirs.contains(&norm) {
            return Err(FsError::WrongKind(norm));
        }
        // Check parent exists.
        if let Some(p) = parent(&norm) {
            if !p.is_empty() && !self.dirs.contains(&p) {
                return Err(FsError::ParentNotFound(norm));
            }
        }
        self.files.insert(norm, content.to_vec());
        Ok(())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), FsError> {
        let norm = normalise(path);
        if self.dirs.contains(&norm) {
            return Err(FsError::WrongKind(norm));
        }
        self.files
            .remove(&norm)
            .map(|_| ())
            .ok_or_else(|| FsError::NotFound(norm))
    }

    fn remove_dir(&mut self, path: &str) -> Result<(), FsError> {
        let norm = normalise(path);
        if !self.dirs.contains(&norm) {
            return Err(FsError::NotFound(norm));
        }
        // Check non-empty.
        let prefix = if norm.is_empty() {
            String::new()
        } else {
            format!("{norm}/")
        };
        let has_children = self
            .files
            .keys()
            .any(|k| k.starts_with(&prefix) && k != &norm)
            || self
                .dirs
                .iter()
                .any(|d| d.starts_with(&prefix) && d != &norm);
        if has_children {
            return Err(FsError::AlreadyExists(format!("{norm} is not empty")));
        }
        self.dirs.remove(&norm);
        Ok(())
    }

    fn exists(&self, path: &str) -> bool {
        let norm = normalise(path);
        self.files.contains_key(&norm) || self.dirs.contains(&norm)
    }

    fn is_dir(&self, path: &str) -> bool {
        let norm = normalise(path);
        self.dirs.contains(&norm)
    }

    fn is_file(&self, path: &str) -> bool {
        let norm = normalise(path);
        self.files.contains_key(&norm)
    }

    fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let norm = normalise(path);
        if self.dirs.contains(&norm) || self.files.contains_key(&norm) {
            return Err(FsError::AlreadyExists(norm));
        }
        if let Some(p) = parent(&norm) {
            if !p.is_empty() && !self.dirs.contains(&p) {
                return Err(FsError::ParentNotFound(norm));
            }
        }
        self.dirs.insert(norm);
        Ok(())
    }

    fn create_dir_all(&mut self, path: &str) -> Result<(), FsError> {
        let norm = normalise(path);
        if norm.is_empty() {
            return Ok(()); // root always exists
        }
        let parts: Vec<&str> = norm.split('/').collect();
        let mut current = String::new();
        for part in &parts {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(part);
            if self.files.contains_key(&current) {
                return Err(FsError::WrongKind(current));
            }
            self.dirs.insert(current.clone());
        }
        Ok(())
    }

    fn read_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError> {
        let norm = normalise(path);
        if !self.dirs.contains(&norm) {
            return Err(FsError::NotFound(norm));
        }
        let prefix = if norm.is_empty() {
            String::new()
        } else {
            format!("{norm}/")
        };

        let mut entries = BTreeSet::new();

        for key in self.files.keys() {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if !rest.is_empty() {
                    if let Some(name) = rest.split('/').next() {
                        entries.insert(DirEntry {
                            name: name.to_string(),
                            is_dir: false,
                        });
                    }
                }
            } else if prefix.is_empty() && !key.contains('/') && !key.is_empty() {
                entries.insert(DirEntry {
                    name: key.clone(),
                    is_dir: false,
                });
            }
        }

        for dir in &self.dirs {
            if let Some(rest) = dir.strip_prefix(&prefix) {
                if !rest.is_empty() && !rest.contains('/') {
                    // Override the is_dir flag if we already have this name from files.
                    entries.replace(DirEntry {
                        name: rest.to_string(),
                        is_dir: true,
                    });
                }
            } else if prefix.is_empty() && !dir.contains('/') && !dir.is_empty() {
                entries.replace(DirEntry {
                    name: dir.clone(),
                    is_dir: true,
                });
            }
        }

        Ok(entries.into_iter().collect())
    }

    fn metadata(&self, path: &str) -> Result<Metadata, FsError> {
        let norm = normalise(path);
        if self.dirs.contains(&norm) {
            Ok(Metadata { is_dir: true, len: 0 })
        } else if let Some(data) = self.files.get(&norm) {
            Ok(Metadata {
                is_dir: false,
                len: data.len() as u64,
            })
        } else {
            Err(FsError::NotFound(norm))
        }
    }

    fn list_files(&self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsError> {
        let from_norm = normalise(from);
        let to_norm = normalise(to);

        if self.files.contains_key(&from_norm) {
            // Rename a file.
            if let Some(p) = parent(&to_norm) {
                if !p.is_empty() && !self.dirs.contains(&p) {
                    return Err(FsError::ParentNotFound(to_norm));
                }
            }
            let data = self.files.remove(&from_norm).unwrap();
            self.files.insert(to_norm, data);
            Ok(())
        } else if self.dirs.contains(&from_norm) {
            // Rename a directory (and all children).
            let old_prefix = if from_norm.is_empty() {
                String::new()
            } else {
                format!("{from_norm}/")
            };
            let new_prefix = if to_norm.is_empty() {
                String::new()
            } else {
                format!("{to_norm}/")
            };

            // Collect affected paths.
            let file_moves: Vec<(String, String)> = self
                .files
                .keys()
                .filter(|k| k.starts_with(&old_prefix))
                .map(|k| {
                    let rest = &k[old_prefix.len()..];
                    (k.clone(), format!("{new_prefix}{rest}"))
                })
                .collect();
            let dir_moves: Vec<(String, String)> = self
                .dirs
                .iter()
                .filter(|d| **d == from_norm || d.starts_with(&old_prefix))
                .map(|d| {
                    if *d == from_norm {
                        (d.clone(), to_norm.clone())
                    } else {
                        let rest = &d[old_prefix.len()..];
                        (d.clone(), format!("{new_prefix}{rest}"))
                    }
                })
                .collect();

            for (old, new) in file_moves {
                let data = self.files.remove(&old).unwrap();
                self.files.insert(new, data);
            }
            for (old, new) in dir_moves {
                self.dirs.remove(&old);
                self.dirs.insert(new);
            }

            Ok(())
        } else {
            Err(FsError::NotFound(from_norm))
        }
    }
}

// We need Ord/PartialOrd for BTreeSet.
impl PartialOrd for DirEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DirEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_read_file() {
        let mut fs = MemoryFilesystem::new();
        fs.write_file("hello.txt", b"world").unwrap();
        assert_eq!(fs.read_file("hello.txt").unwrap(), b"world");
        assert_eq!(fs.read_to_string("hello.txt").unwrap(), "world");
    }

    #[test]
    fn write_requires_parent_directory() {
        let mut fs = MemoryFilesystem::new();
        let err = fs.write_file("a/b.txt", b"data").unwrap_err();
        assert!(matches!(err, FsError::ParentNotFound(_)));
    }

    #[test]
    fn create_dir_all_and_write() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir_all("a/b/c").unwrap();
        fs.write_file("a/b/c/f.txt", b"ok").unwrap();
        assert_eq!(fs.read_file("a/b/c/f.txt").unwrap(), b"ok");
        assert!(fs.is_dir("a"));
        assert!(fs.is_dir("a/b"));
        assert!(fs.is_dir("a/b/c"));
    }

    #[test]
    fn remove_file_and_exists() {
        let mut fs = MemoryFilesystem::new();
        fs.write_file("f.txt", b"x").unwrap();
        assert!(fs.exists("f.txt"));
        assert!(fs.is_file("f.txt"));
        fs.remove_file("f.txt").unwrap();
        assert!(!fs.exists("f.txt"));
    }

    #[test]
    fn read_dir_lists_children() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir("src").unwrap();
        fs.write_file("src/a.rs", b"").unwrap();
        fs.write_file("src/b.rs", b"").unwrap();
        fs.create_dir("src/sub").unwrap();

        let entries = fs.read_dir("src").unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a.rs"));
        assert!(names.contains(&"b.rs"));
        assert!(names.contains(&"sub"));
        assert!(entries.iter().find(|e| e.name == "sub").unwrap().is_dir);
    }

    #[test]
    fn metadata_works() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir("d").unwrap();
        fs.write_file("d/f.txt", b"hello").unwrap();

        let md = fs.metadata("d").unwrap();
        assert!(md.is_dir);
        assert_eq!(md.len, 0);

        let mf = fs.metadata("d/f.txt").unwrap();
        assert!(!mf.is_dir);
        assert_eq!(mf.len, 5);
    }

    #[test]
    fn leading_slash_normalisation() {
        let mut fs = MemoryFilesystem::new();
        fs.write_file("/root.txt", b"data").unwrap();
        assert_eq!(fs.read_file("root.txt").unwrap(), b"data");
        assert_eq!(fs.read_file("/root.txt").unwrap(), b"data");
    }

    #[test]
    fn list_files_returns_all() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir("src").unwrap();
        fs.write_file("src/main.rs", b"fn main() {}").unwrap();
        fs.write_file("README.md", b"# hi").unwrap();
        let files = fs.list_files();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"README.md".to_string()));
        assert!(files.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn rename_file() {
        let mut fs = MemoryFilesystem::new();
        fs.write_file("old.txt", b"content").unwrap();
        fs.rename("old.txt", "new.txt").unwrap();
        assert!(!fs.exists("old.txt"));
        assert_eq!(fs.read_file("new.txt").unwrap(), b"content");
    }

    #[test]
    fn snapshot_and_restore() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir("src").unwrap();
        fs.write_file("src/lib.rs", b"pub mod x;").unwrap();
        fs.write_file("README.md", b"hi").unwrap();

        let snap = fs.snapshot();

        let mut fs2 = MemoryFilesystem::new();
        fs2.restore(snap);

        assert_eq!(fs2.read_file("src/lib.rs").unwrap(), b"pub mod x;");
        assert_eq!(fs2.read_file("README.md").unwrap(), b"hi");
        assert!(fs2.is_dir("src"));
    }

    #[test]
    fn remove_dir_non_empty_fails() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir("d").unwrap();
        fs.write_file("d/f.txt", b"x").unwrap();
        assert!(fs.remove_dir("d").is_err());
    }

    #[test]
    fn remove_dir_empty_succeeds() {
        let mut fs = MemoryFilesystem::new();
        fs.create_dir("d").unwrap();
        fs.remove_dir("d").unwrap();
        assert!(!fs.exists("d"));
    }
}
