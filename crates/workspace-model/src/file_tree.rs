//! The file explorer's tree: folder entries sorted folders first, the folders expanded under
//! each root, the rows shown for a root, and the names and paths its menu works with. Folder
//! contents are read by the caller; nothing here touches the file system.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::Platform;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    File,
    Folder,
}

/// One entry of a folder listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    /// A folder, or a symlink to one.
    pub kind: EntryKind,
}

impl DirEntry {
    pub fn new(name: impl Into<String>, kind: EntryKind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }
}

/// Folders first, then files, each by name ignoring case (ties by the exact name, so the
/// order is stable).
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        b.kind
            .cmp(&a.kind)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// What a file holds, for its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Markdown,
    Image,
    Code,
    /// JSON, YAML, TOML and other structured data.
    Data,
    Archive,
    Text,
    Other,
}

/// The kind of file `name` is, from its extension.
pub fn file_kind(name: &str) -> FileKind {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return FileKind::Other;
    };
    if stem.is_empty() {
        // A dotfile such as `.gitignore` has no extension.
        return FileKind::Other;
    }
    match extension.to_ascii_lowercase().as_str() {
        "md" | "markdown" | "mdx" => FileKind::Markdown,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tif" | "tiff"
        | "avif" => FileKind::Image,
        "rs" | "c" | "h" | "cpp" | "hpp" | "cc" | "cs" | "java" | "kt" | "go" | "py" | "rb"
        | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "html" | "css" | "scss" | "sh" | "bash"
        | "zsh" | "fish" | "ps1" | "swift" | "lua" | "php" | "sql" => FileKind::Code,
        "json" | "jsonc" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" | "conf" | "lock"
        | "csv" => FileKind::Data,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "7z" | "rar" => FileKind::Archive,
        "txt" | "log" | "rst" => FileKind::Text,
        _ => FileKind::Other,
    }
}

/// One row of the tree as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    /// 0 for the root.
    pub depth: usize,
    pub kind: EntryKind,
    pub expanded: bool,
}

/// The folders expanded in the explorer, kept per root so going back to a folder shows it
/// as it was left. A root starts expanded the first time it is shown.
#[derive(Clone, Debug, Default)]
pub struct ExpandedFolders {
    by_root: HashMap<PathBuf, HashSet<PathBuf>>,
}

impl ExpandedFolders {
    /// The folders expanded under `root`, `root` itself included when it is.
    pub fn of(&mut self, root: &Path) -> &HashSet<PathBuf> {
        self.entry(root)
    }

    pub fn is_expanded(&mut self, root: &Path, folder: &Path) -> bool {
        self.entry(root).contains(folder)
    }

    /// Expand `folder` under `root` if it is collapsed, collapse it otherwise. Returns whether
    /// it is now expanded.
    pub fn toggle(&mut self, root: &Path, folder: &Path) -> bool {
        let expanded = self.entry(root);
        if expanded.remove(folder) {
            return false;
        }
        expanded.insert(folder.to_path_buf());
        true
    }

    pub fn expand(&mut self, root: &Path, folder: &Path) {
        self.entry(root).insert(folder.to_path_buf());
    }

    /// Follow a folder renamed from `from` to `to`: it and the folders under it stay as they
    /// were, under every root.
    pub fn rename(&mut self, from: &Path, to: &Path) {
        for expanded in self.by_root.values_mut() {
            let moved: Vec<PathBuf> = expanded
                .iter()
                .filter(|folder| folder.starts_with(from))
                .cloned()
                .collect();
            for folder in moved {
                expanded.remove(&folder);
                if let Ok(rest) = folder.strip_prefix(from) {
                    expanded.insert(to.join(rest));
                }
            }
        }
    }

    fn entry(&mut self, root: &Path) -> &mut HashSet<PathBuf> {
        self.by_root
            .entry(root.to_path_buf())
            .or_insert_with(|| HashSet::from([root.to_path_buf()]))
    }
}

/// The rows shown for `root`: the root, then the entries of each expanded folder under it,
/// depth first. `listing` gives a folder's sorted entries, `None` when it was not read (its
/// rows are then left out).
pub fn visible_rows<'a>(
    root: &Path,
    expanded: &HashSet<PathBuf>,
    listing: impl Fn(&Path) -> Option<&'a [DirEntry]>,
) -> Vec<TreeRow> {
    let name = entry_name(root);
    let root_expanded = expanded.contains(root);
    let mut rows = vec![TreeRow {
        path: root.to_path_buf(),
        name,
        depth: 0,
        kind: EntryKind::Folder,
        expanded: root_expanded,
    }];
    if root_expanded {
        push_children(root, 1, expanded, &listing, &mut rows);
    }
    rows
}

fn push_children<'a>(
    folder: &Path,
    depth: usize,
    expanded: &HashSet<PathBuf>,
    listing: &impl Fn(&Path) -> Option<&'a [DirEntry]>,
    rows: &mut Vec<TreeRow>,
) {
    let Some(entries) = listing(folder) else {
        return;
    };
    for entry in entries {
        let path = folder.join(&entry.name);
        let is_expanded = entry.kind == EntryKind::Folder && expanded.contains(&path);
        rows.push(TreeRow {
            path: path.clone(),
            name: entry.name.clone(),
            depth,
            kind: entry.kind,
            expanded: is_expanded,
        });
        if is_expanded {
            push_children(&path, depth + 1, expanded, listing, rows);
        }
    }
}

/// Which of `rows` (as [`visible_rows`] gives them) git ignores: an entry whose name is in its
/// folder's ignored names (`ignored_in`, `None` when not known), and everything under an
/// ignored folder. The root never is.
pub fn ignored_rows<'a>(
    rows: &[TreeRow],
    ignored_in: impl Fn(&Path) -> Option<&'a HashSet<String>>,
) -> Vec<bool> {
    // Whether the row at each depth of the current branch is ignored.
    let mut branch: Vec<bool> = Vec::new();
    rows.iter()
        .map(|row| {
            branch.truncate(row.depth);
            let ignored = row.depth > 0
                && (branch.last() == Some(&true)
                    || row
                        .path
                        .parent()
                        .and_then(&ignored_in)
                        .is_some_and(|names| names.contains(&row.name)));
            branch.push(ignored);
            ignored
        })
        .collect()
}

/// Whether a batch of changed `paths` touches a `.gitignore`, which can change what git
/// ignores in every folder under it.
pub fn touches_gitignore(paths: &[PathBuf]) -> bool {
    paths
        .iter()
        .any(|path| path.file_name().is_some_and(|name| name == ".gitignore"))
}

/// `path` relative to `root` (`.` for the root itself), or the whole path when it is not
/// under `root`.
pub fn relative_path(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rest) if rest.as_os_str().is_empty() => ".".to_string(),
        Ok(rest) => rest.display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// The folder an entry's "cd to directory" and "Open in new tab" use: the folder itself, or
/// a file's parent.
pub fn entry_folder(path: &Path, kind: EntryKind) -> &Path {
    if kind == EntryKind::Folder {
        return path;
    }
    path.parent().unwrap_or(path)
}

/// An entry's name, or its path when it has no final component (such as a drive root).
pub fn entry_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The folder the explorer's "Parent folder (cd ..)" goes to from `root`; `None` at a
/// filesystem or drive root.
pub fn parent_folder(root: &Path) -> Option<&Path> {
    root.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

/// The changed folders to read again for a batch of changed `paths`: a change names an entry,
/// so the folder is the watched one holding it, or the entry itself when it is a watched
/// folder. `watched` maps each watched real path to the folders showing it.
pub fn folders_to_reread(
    paths: &[PathBuf],
    watched: &HashMap<PathBuf, Vec<PathBuf>>,
) -> HashSet<PathBuf> {
    paths
        .iter()
        .flat_map(|path| [Some(path.as_path()), path.parent()])
        .flatten()
        .filter_map(|folder| watched.get(folder))
        .flatten()
        .cloned()
        .collect()
}

/// How the shell a tab runs parses a typed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellSyntax {
    /// bash, zsh, fish, sh, Git Bash.
    Posix,
    /// A WSL distro's shell: POSIX, with Windows paths in the distro's form.
    Wsl,
    /// pwsh and Windows PowerShell.
    PowerShell,
    /// cmd.exe.
    Cmd,
}

impl ShellSyntax {
    /// The syntax of `program`, a name or path with or without `.exe`.
    pub fn of_program(program: &str) -> Self {
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let name = name.to_ascii_lowercase();
        match name.strip_suffix(".exe").unwrap_or(&name) {
            "pwsh" | "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            "wsl" => Self::Wsl,
            _ => Self::Posix,
        }
    }
}

/// Why a path can't be typed at a prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeError {
    NotUtf8,
    /// Control characters would act as keys (Ctrl+C, Enter) when typed.
    ControlCharacter,
    /// cmd.exe expands `%` even inside quotes and has no way to escape `"` in them.
    CmdSpecial,
    /// A network path other than a WSL distro's has no path inside WSL.
    OutsideWsl,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotUtf8 => "The path isn't valid UTF-8",
            Self::ControlCharacter => "The path holds control characters",
            Self::CmdSpecial => "The path holds % or \", which cmd can't take",
            Self::OutsideWsl => "The path can't be reached from WSL",
        })
    }
}

/// `path` as text that is safe to type at a prompt.
pub fn typeable(path: &Path) -> Result<&str, TypeError> {
    let text = path.to_str().ok_or(TypeError::NotUtf8)?;
    if text.chars().any(char::is_control) {
        return Err(TypeError::ControlCharacter);
    }
    Ok(text)
}

/// `path` as one word for `shell`.
pub fn quote_path(path: &Path, shell: ShellSyntax) -> Result<String, TypeError> {
    let text = typeable(path)?;
    match shell {
        ShellSyntax::Posix => Ok(quote_posix(text)),
        ShellSyntax::PowerShell => Ok(quote_powershell(text)),
        ShellSyntax::Cmd => quote_cmd(text),
        ShellSyntax::Wsl => Ok(quote_posix(&wsl_path(text)?)),
    }
}

/// `text`, a Windows path, as WSL sees it under its default `/mnt` mount root: `C:\x` is
/// `/mnt/c/x` and `\\wsl$\<distro>\x` (or `\\wsl.localhost\...`) is `/x`. Other paths are
/// kept as they are.
fn wsl_path(text: &str) -> Result<String, TypeError> {
    let text = match text.strip_prefix(r"\\?\") {
        Some(rest) => match rest.strip_prefix(r"UNC\") {
            Some(unc) => format!(r"\\{unc}"),
            None => rest.to_string(),
        },
        None => text.to_string(),
    };
    if let Some(unc) = text.strip_prefix(r"\\") {
        let mut parts = unc.splitn(3, '\\');
        let host = parts.next().unwrap_or_default();
        let in_distro = parts.next().is_some_and(|distro| !distro.is_empty());
        if !in_distro
            || !(host.eq_ignore_ascii_case("wsl$") || host.eq_ignore_ascii_case("wsl.localhost"))
        {
            return Err(TypeError::OutsideWsl);
        }
        return Ok(format!(
            "/{}",
            parts.next().unwrap_or_default().replace('\\', "/")
        ));
    }
    let mut chars = text.chars();
    if let (Some(drive), Some(':')) = (chars.next(), chars.next())
        && drive.is_ascii_alphabetic()
    {
        let rest = chars.as_str().replace('\\', "/");
        return Ok(format!("/mnt/{}{rest}", drive.to_ascii_lowercase()));
    }
    Ok(text)
}

/// The line "cd to directory" types for `folder`, Enter included.
pub fn cd_command(folder: &Path, shell: ShellSyntax) -> Result<String, TypeError> {
    let quoted = quote_path(folder, shell)?;
    Ok(match shell {
        ShellSyntax::Posix | ShellSyntax::Wsl => format!("cd {quoted}\r"),
        // `cd` takes wildcards (`[`, `]`, `*`) in PowerShell.
        ShellSyntax::PowerShell => format!("Set-Location -LiteralPath {quoted}\r"),
        // `/d` also changes the drive.
        ShellSyntax::Cmd => format!("cd /d {quoted}\r"),
    })
}

/// `text` as one word for a POSIX shell: as is when it holds no special character, else in
/// single quotes (a quote inside becomes `'\''`).
fn quote_posix(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c));
    if plain {
        return text.to_string();
    }
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// PowerShell's single quotes: the ASCII one and the typographic ones it also accepts.
const POWERSHELL_QUOTES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'];

/// `text` as one word for PowerShell: as is when plain, else in single quotes, where nothing
/// is expanded (a quote inside is doubled).
fn quote_powershell(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || r"\/:._-".contains(c));
    if plain {
        return text.to_string();
    }
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for c in text.chars() {
        if POWERSHELL_QUOTES.contains(&c) {
            quoted.push(c);
        }
        quoted.push(c);
    }
    quoted.push('\'');
    quoted
}

/// `text` as one word for cmd.exe: in double quotes when it holds a space or a special
/// character; refused when it holds `%` or `"`.
fn quote_cmd(text: &str) -> Result<String, TypeError> {
    if text.contains(['%', '"']) {
        return Err(TypeError::CmdSpecial);
    }
    let plain = !text.is_empty()
        && !text
            .chars()
            .any(|c| c.is_whitespace() || "&()[]{}^=;!'+,`~$#@<>|".contains(c));
    if plain {
        return Ok(text.to_string());
    }
    Ok(format!("\"{text}\""))
}

/// Why a name typed for a new or renamed entry is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    /// `.` or `..`.
    Reserved,
    /// Holds a path separator or a NUL character.
    Separator,
    /// Holds a character Windows doesn't allow in names (`:` would also leave the folder).
    WindowsCharacter,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "A name is required",
            Self::Reserved => "\".\" and \"..\" can't be used as names",
            Self::Separator => "A name can't contain / or \\",
            Self::WindowsCharacter => "A name can't contain : < > \" | ? or *",
        })
    }
}

/// The name to create or rename to: `name` without surrounding spaces, refused when empty,
/// `.`/`..`, or holding a path separator (or, on Windows, a character it doesn't allow).
pub fn validate_name(name: &str) -> Result<&str, NameError> {
    validate_name_for(name, Platform::CURRENT)
}

fn validate_name_for(name: &str, platform: Platform) -> Result<&str, NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameError::Reserved);
    }
    if name.contains(['/', '\\', '\0']) {
        return Err(NameError::Separator);
    }
    if platform == Platform::Windows && name.contains([':', '<', '>', '"', '|', '?', '*']) {
        return Err(NameError::WindowsCharacter);
    }
    Ok(name)
}

#[cfg(test)]
mod tests;
