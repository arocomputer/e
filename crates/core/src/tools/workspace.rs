//! Where the file tools read and write, and where `bash` runs.
//!
//! Every file the built-in tools touch goes through a [`Workspace`]. [`Disk`]
//! is the machine's filesystem, with the staged writes and identity checks
//! the tools rely on; [`Memory`] is a tree held in the process, for an
//! embedding that must not reach a disk (the browser build, tests). An agent
//! gets one through `AgentOptions::workspace`; the default is `Disk`.
//!
//! `bash` runs local processes unless the embedding supplies a [`Shell`]
//! through `AgentOptions::shell`; then every command goes there instead, and
//! background processes are refused.
//!
//! Paths arrive absolute: the tools resolve them against the agent's working
//! directory first.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// The file operations the built-in tools need.
pub trait Workspace: Send + Sync + std::fmt::Debug {
    /// Metadata, following a symlink to its target.
    fn metadata(&self, path: &Path) -> io::Result<Metadata>;
    /// Metadata of the entry itself, so a walk can refuse symlinks.
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata>;
    /// A reader over a regular file's bytes.
    fn open(&self, path: &Path) -> io::Result<Box<dyn Read + '_>>;
    /// Replace a file's bytes, creating it when missing. The parent
    /// directory must exist.
    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    /// The entries directly inside a directory, as full paths.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
    /// The absolute path with every `.`, `..`, and symlink resolved. Fails
    /// when the path does not exist.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// An identity every alias of an existing file shares (a Unix device and
    /// inode), for workspaces where one file can have several names.
    fn identity(&self, _path: &Path) -> Option<(u64, u64)> {
        None
    }

    /// The whole file.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.open(path)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// The whole file as UTF-8, failing like `std::fs::read_to_string`.
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let mut text = String::new();
        self.open(path)?.read_to_string(&mut text)?;
        Ok(text)
    }
}

/// Runs `bash` commands for an embedding that has no processes: the browser
/// build hands them to a shell simulated in the page. It should see the same
/// files as the agent's [`Workspace`].
pub trait Shell: crate::rt::MaybeSend + crate::rt::MaybeSync + std::fmt::Debug {
    /// Run `command` in `cwd` and resolve once it exits. Dropping the future
    /// cancels the command.
    fn run(
        &self,
        command: &str,
        cwd: &Path,
    ) -> crate::rt::BoxFuture<'static, io::Result<ShellOutput>>;
}

/// A finished command's output.
#[derive(Clone, Debug, Default)]
pub struct ShellOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// What the tools ask of an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub kind: Kind,
    pub len: u64,
    /// Changes whenever the contents may have; `None` when the platform
    /// cannot say.
    pub stamp: Option<Stamp>,
}

impl Metadata {
    pub fn is_file(&self) -> bool {
        self.kind == Kind::File
    }

    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Symlink,
    /// A FIFO, socket, or device: never read as text.
    Other,
}

/// A version of a file's contents. Freshness checks compare it, with the
/// length, against the one recorded when e last read or wrote the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stamp {
    /// A disk file's modification time.
    Modified(std::time::SystemTime),
    /// A memory file's revision, bumped on every write.
    Revision(u64),
}

/// The machine's filesystem.
#[derive(Clone, Copy, Debug, Default)]
pub struct Disk;

impl Disk {
    fn convert(meta: std::fs::Metadata) -> Metadata {
        let file_type = meta.file_type();
        let kind = if file_type.is_symlink() {
            Kind::Symlink
        } else if file_type.is_dir() {
            Kind::Dir
        } else if file_type.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        Metadata {
            kind,
            len: meta.len(),
            stamp: meta.modified().ok().map(Stamp::Modified),
        }
    }
}

impl Workspace for Disk {
    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        std::fs::metadata(path).map(Self::convert)
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        std::fs::symlink_metadata(path).map(Self::convert)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn Read + '_>> {
        Ok(Box::new(std::fs::File::open(path)?))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        super::staged_write(path, bytes)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(std::fs::read_dir(path)?
            .flatten()
            .map(|entry| entry.path())
            .collect())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }

    #[cfg(unix)]
    fn identity(&self, path: &Path) -> Option<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        Some((metadata.dev(), metadata.ino()))
    }
}

/// A filesystem held in the process: directories and files, no links, no
/// permissions. Paths are absolute and normalized lexically, so `..` never
/// climbs above `/`. Safe to share across threads.
#[derive(Debug)]
pub struct Memory {
    tree: Mutex<Tree>,
}

#[derive(Debug)]
struct Tree {
    nodes: BTreeMap<PathBuf, Node>,
    revision: u64,
}

#[derive(Debug, Clone)]
enum Node {
    Dir,
    File { bytes: Vec<u8>, revision: u64 },
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    /// An empty tree holding only `/`.
    pub fn new() -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(PathBuf::from("/"), Node::Dir);
        Memory {
            tree: Mutex::new(Tree { nodes, revision: 0 }),
        }
    }

    fn tree(&self) -> std::sync::MutexGuard<'_, Tree> {
        self.tree
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Remove a file, or a directory with everything in it.
    pub fn remove_all(&self, path: &Path) -> io::Result<()> {
        let path = normalize(path)?;
        if path == Path::new("/") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove /",
            ));
        }
        let mut tree = self.tree();
        if tree.nodes.remove(&path).is_none() {
            return Err(not_found(&path));
        }
        tree.nodes.retain(|key, _| !key.starts_with(&path));
        Ok(())
    }

    /// Remove an empty directory.
    pub fn remove_dir(&self, path: &Path) -> io::Result<()> {
        let path = normalize(path)?;
        let mut tree = self.tree();
        match tree.nodes.get(&path) {
            None => return Err(not_found(&path)),
            Some(Node::File { .. }) => return Err(not_a_directory(&path)),
            Some(Node::Dir) => {}
        }
        if path == Path::new("/") || tree.children(&path).next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                format!("{}: directory not empty", path.display()),
            ));
        }
        tree.nodes.remove(&path);
        Ok(())
    }

    /// Create one directory whose parent exists.
    pub fn create_dir(&self, path: &Path) -> io::Result<()> {
        let path = normalize(path)?;
        let mut tree = self.tree();
        if tree.nodes.contains_key(&path) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{}: already exists", path.display()),
            ));
        }
        tree.require_dir(parent_of(&path))?;
        tree.nodes.insert(path, Node::Dir);
        Ok(())
    }

    /// Move a file or directory, with everything under it, replacing a file
    /// at the destination.
    pub fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (normalize(from)?, normalize(to)?);
        if from == to {
            return Ok(());
        }
        if to.starts_with(&from) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("cannot move {} into itself", from.display()),
            ));
        }
        let mut tree = self.tree();
        let node = tree
            .nodes
            .get(&from)
            .cloned()
            .ok_or_else(|| not_found(&from))?;
        tree.require_dir(parent_of(&to))?;
        if let Some(Node::Dir) = tree.nodes.get(&to) {
            return Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                format!("{}: is a directory", to.display()),
            ));
        }
        let moved: Vec<(PathBuf, Node)> = tree
            .nodes
            .iter()
            .filter(|(key, _)| key.starts_with(&from) && **key != from)
            .map(|(key, node)| (key.clone(), node.clone()))
            .collect();
        tree.nodes.retain(|key, _| !key.starts_with(&from));
        tree.revision += 1;
        let revision = tree.revision;
        let node = match node {
            Node::File { bytes, .. } => Node::File { bytes, revision },
            dir => dir,
        };
        tree.nodes.insert(to.clone(), node);
        for (key, node) in moved {
            if let Ok(rest) = key.strip_prefix(&from) {
                tree.nodes.insert(to.join(rest), node);
            }
        }
        Ok(())
    }

    /// Every path in the tree, sorted, `/` included.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.tree().nodes.keys().cloned().collect()
    }
}

impl Tree {
    fn children<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.nodes
            .range::<Path, _>((std::ops::Bound::Excluded(dir), std::ops::Bound::Unbounded))
            .map(|(key, _)| key)
            .take_while(move |key| key.starts_with(dir))
            .filter(move |key| key.parent() == Some(dir))
    }

    fn require_dir(&self, path: &Path) -> io::Result<()> {
        match self.nodes.get(path) {
            Some(Node::Dir) => Ok(()),
            Some(Node::File { .. }) => Err(not_a_directory(path)),
            None => Err(not_found(path)),
        }
    }
}

impl Workspace for Memory {
    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        let path = normalize(path)?;
        match self.tree().nodes.get(&path) {
            Some(Node::Dir) => Ok(Metadata {
                kind: Kind::Dir,
                len: 0,
                stamp: None,
            }),
            Some(Node::File { bytes, revision }) => Ok(Metadata {
                kind: Kind::File,
                len: bytes.len() as u64,
                stamp: Some(Stamp::Revision(*revision)),
            }),
            None => Err(not_found(&path)),
        }
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        self.metadata(path)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn Read + '_>> {
        let path = normalize(path)?;
        match self.tree().nodes.get(&path) {
            Some(Node::File { bytes, .. }) => Ok(Box::new(io::Cursor::new(bytes.clone()))),
            Some(Node::Dir) => Err(is_a_directory(&path)),
            None => Err(not_found(&path)),
        }
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let path = normalize(path)?;
        let mut tree = self.tree();
        if let Some(Node::Dir) = tree.nodes.get(&path) {
            return Err(is_a_directory(&path));
        }
        tree.require_dir(parent_of(&path))?;
        tree.revision += 1;
        let revision = tree.revision;
        tree.nodes.insert(
            path,
            Node::File {
                bytes: bytes.to_vec(),
                revision,
            },
        );
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let path = normalize(path)?;
        let mut tree = self.tree();
        match tree.nodes.get(&path) {
            Some(Node::File { .. }) => {
                tree.nodes.remove(&path);
                Ok(())
            }
            Some(Node::Dir) => Err(is_a_directory(&path)),
            None => Err(not_found(&path)),
        }
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let path = normalize(path)?;
        let mut tree = self.tree();
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            match tree.nodes.get(ancestor) {
                Some(Node::Dir) => {}
                Some(Node::File { .. }) => return Err(not_a_directory(ancestor)),
                None => {
                    tree.nodes.insert(ancestor.to_path_buf(), Node::Dir);
                }
            }
        }
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let path = normalize(path)?;
        let tree = self.tree();
        tree.require_dir(&path)?;
        Ok(tree.children(&path).cloned().collect())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let path = normalize(path)?;
        if self.tree().nodes.contains_key(&path) {
            Ok(path)
        } else {
            Err(not_found(&path))
        }
    }
}

/// Resolve `.` and `..` without touching the tree. Relative paths are
/// refused: the tools always resolve against the working directory first.
fn normalize(path: &Path) -> io::Result<PathBuf> {
    // Rooted, not `is_absolute`: the browser build is not a Unix target, and
    // there no path counts as absolute.
    if !path.has_root() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: workspace paths must be absolute", path.display()),
        ));
    }
    let mut normal = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(part) => normal.push(part),
            Component::ParentDir => {
                normal.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    Ok(normal)
}

fn parent_of(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("/"))
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{}: no such file or directory", path.display()),
    )
}

fn not_a_directory(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotADirectory,
        format!("{}: not a directory", path.display()),
    )
}

fn is_a_directory(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::IsADirectory,
        format!("{}: is a directory", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(memory: &Memory, path: &str) -> String {
        memory.read_to_string(Path::new(path)).unwrap()
    }

    #[test]
    fn memory_writes_need_a_parent_directory() {
        let memory = Memory::new();
        let error = memory
            .write(Path::new("/src/main.rs"), b"fn main() {}")
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        memory.create_dir_all(Path::new("/src")).unwrap();
        memory
            .write(Path::new("/src/main.rs"), b"fn main() {}")
            .unwrap();
        assert_eq!(text(&memory, "/src/./main.rs"), "fn main() {}");
    }

    #[test]
    fn memory_stamps_change_with_every_write() {
        let memory = Memory::new();
        let path = Path::new("/notes.txt");
        memory.write(path, b"one").unwrap();
        let first = memory.metadata(path).unwrap().stamp;
        memory.write(path, b"two").unwrap();
        assert_ne!(memory.metadata(path).unwrap().stamp, first);
    }

    #[test]
    fn memory_lists_only_direct_children() {
        let memory = Memory::new();
        memory.create_dir_all(Path::new("/a/b")).unwrap();
        memory.write(Path::new("/a/one.txt"), b"1").unwrap();
        memory.write(Path::new("/a/b/two.txt"), b"2").unwrap();
        memory.write(Path::new("/ab.txt"), b"3").unwrap();
        let children = memory.read_dir(Path::new("/a")).unwrap();
        assert_eq!(
            children,
            vec![PathBuf::from("/a/b"), PathBuf::from("/a/one.txt")]
        );
    }

    #[test]
    fn memory_rename_moves_a_whole_directory() {
        let memory = Memory::new();
        memory.create_dir_all(Path::new("/old/inner")).unwrap();
        memory.write(Path::new("/old/inner/file"), b"x").unwrap();
        memory.rename(Path::new("/old"), Path::new("/new")).unwrap();
        assert_eq!(text(&memory, "/new/inner/file"), "x");
        assert!(memory.metadata(Path::new("/old")).is_err());
        assert!(memory
            .rename(Path::new("/new"), Path::new("/new/inside"))
            .is_err());
    }

    #[test]
    fn memory_paths_never_climb_above_the_root() {
        let memory = Memory::new();
        memory.write(Path::new("/../../top.txt"), b"t").unwrap();
        assert_eq!(text(&memory, "/top.txt"), "t");
        assert!(memory.remove_all(Path::new("/..")).is_err());
    }
}
