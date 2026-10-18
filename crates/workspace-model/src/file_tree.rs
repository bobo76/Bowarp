//! The file explorer's tree: folder entries sorted folders first, the folders expanded under
//! each root, the rows shown for a root, and the names and paths its menu works with. Folder
//! contents are read by the caller; nothing here touches the file system.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// One entry of a folder listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    /// A folder, or a symlink to one.
    pub is_dir: bool,
}

impl DirEntry {
    pub fn new(name: impl Into<String>, is_dir: bool) -> Self {
        Self {
            name: name.into(),
            is_dir,
        }
    }
}

/// Folders first, then files, each by name ignoring case (ties by the exact name, so the
/// order is stable).
pub fn sort_entries(entries: &mut [DirEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
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
    pub is_dir: bool,
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
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into(),
    );
    let root_expanded = expanded.contains(root);
    let mut rows = vec![TreeRow {
        path: root.to_path_buf(),
        name,
        depth: 0,
        is_dir: true,
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
        let is_expanded = entry.is_dir && expanded.contains(&path);
        rows.push(TreeRow {
            path: path.clone(),
            name: entry.name.clone(),
            depth,
            is_dir: entry.is_dir,
            expanded: is_expanded,
        });
        if is_expanded {
            push_children(&path, depth + 1, expanded, listing, rows);
        }
    }
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
pub fn entry_folder(path: &Path, is_dir: bool) -> &Path {
    if is_dir {
        return path;
    }
    path.parent().unwrap_or(path)
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
    /// bash, zsh, fish, sh, Git Bash, WSL.
    Posix,
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
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotUtf8 => "The path isn't valid UTF-8",
            Self::ControlCharacter => "The path holds control characters",
            Self::CmdSpecial => "The path holds % or \", which cmd can't take",
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
    }
}

/// The line "cd to directory" types for `folder`, Enter included.
pub fn cd_command(folder: &Path, shell: ShellSyntax) -> Result<String, TypeError> {
    let quoted = quote_path(folder, shell)?;
    Ok(match shell {
        ShellSyntax::Posix => format!("cd {quoted}\r"),
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

/// What "Attach as context" types at a prompt for `path`, followed by a space to keep typing:
/// a Claude Code file mention (`@path`, `@"path"` when it holds a space or a quote) when
/// `mention`, else the path quoted for `shell`. A path holding `"` can't be mentioned, so
/// it is typed as is.
pub fn context_reference(
    path: &Path,
    shell: ShellSyntax,
    mention: bool,
) -> Result<String, TypeError> {
    if !mention {
        return Ok(format!("{} ", quote_path(path, shell)?));
    }
    let text = typeable(path)?;
    if text.contains('"') {
        return Ok(format!("{text} "));
    }
    if text.chars().any(|c| c.is_whitespace() || c == '\'') {
        return Ok(format!("@\"{text}\" "));
    }
    Ok(format!("@{text} "))
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
    validate_name_for(name, cfg!(windows))
}

fn validate_name_for(name: &str, windows: bool) -> Result<&str, NameError> {
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
    if windows && name.contains([':', '<', '>', '"', '|', '?', '*']) {
        return Err(NameError::WindowsCharacter);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[DirEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn folders_come_first_then_names_ignoring_case() {
        let mut entries = vec![
            DirEntry::new("b.md", false),
            DirEntry::new("Zeta", true),
            DirEntry::new(".hidden", false),
            DirEntry::new("alpha", true),
            DirEntry::new("A.txt", false),
            DirEntry::new(".git", true),
            DirEntry::new("a.txt", false),
        ];
        sort_entries(&mut entries);
        assert_eq!(
            names(&entries),
            [".git", "alpha", "Zeta", ".hidden", "A.txt", "a.txt", "b.md"]
        );
    }

    #[test]
    fn file_kinds_follow_the_extension() {
        assert_eq!(file_kind("README.md"), FileKind::Markdown);
        assert_eq!(file_kind("shot.PNG"), FileKind::Image);
        assert_eq!(file_kind("main.rs"), FileKind::Code);
        assert_eq!(file_kind("Cargo.toml"), FileKind::Data);
        assert_eq!(file_kind("a.tar.gz"), FileKind::Archive);
        assert_eq!(file_kind("notes.txt"), FileKind::Text);
        assert_eq!(file_kind(".gitignore"), FileKind::Other);
        assert_eq!(file_kind("Makefile"), FileKind::Other);
    }

    #[test]
    fn a_root_starts_expanded_and_keeps_its_own_state() {
        let mut expanded = ExpandedFolders::default();
        let (one, two) = (Path::new("/one"), Path::new("/two"));
        assert!(expanded.is_expanded(one, one));
        assert!(expanded.toggle(one, Path::new("/one/src")));
        assert!(!expanded.toggle(one, one));
        assert!(expanded.is_expanded(two, two));
        assert!(!expanded.is_expanded(two, Path::new("/one/src")));
        assert!(!expanded.is_expanded(one, one));
        assert!(expanded.is_expanded(one, Path::new("/one/src")));
    }

    #[test]
    fn renaming_a_folder_keeps_it_and_its_subfolders_expanded() {
        let mut expanded = ExpandedFolders::default();
        let root = Path::new("/r");
        expanded.expand(root, Path::new("/r/old"));
        expanded.expand(root, Path::new("/r/old/inner"));
        expanded.expand(root, Path::new("/r/older"));
        expanded.rename(Path::new("/r/old"), Path::new("/r/new"));
        let folders = expanded.of(root);
        assert!(folders.contains(Path::new("/r/new")));
        assert!(folders.contains(Path::new("/r/new/inner")));
        assert!(folders.contains(Path::new("/r/older")));
        assert!(!folders.contains(Path::new("/r/old")));
    }

    #[test]
    fn rows_show_expanded_folders_depth_first() {
        let root = Path::new("/r");
        let listings: HashMap<PathBuf, Vec<DirEntry>> = HashMap::from([
            (
                PathBuf::from("/r"),
                vec![
                    DirEntry::new("src", true),
                    DirEntry::new("unread", true),
                    DirEntry::new("README.md", false),
                ],
            ),
            (
                PathBuf::from("/r/src"),
                vec![DirEntry::new("lib.rs", false)],
            ),
        ]);
        let expanded: HashSet<PathBuf> = ["/r", "/r/src", "/r/unread"]
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let rows = visible_rows(root, &expanded, |folder| {
            listings.get(folder).map(Vec::as_slice)
        });
        let shown: Vec<(&str, usize, bool)> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.depth, row.expanded))
            .collect();
        assert_eq!(
            shown,
            [
                ("r", 0, true),
                ("src", 1, true),
                ("lib.rs", 2, false),
                ("unread", 1, true),
                ("README.md", 1, false),
            ]
        );
        assert_eq!(rows[2].path, Path::new("/r/src/lib.rs"));
    }

    #[test]
    fn a_collapsed_root_shows_only_itself() {
        let rows = visible_rows(Path::new("/r"), &HashSet::new(), |_| Some(&[][..]));
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].expanded);
    }

    #[test]
    fn relative_paths_are_from_the_root() {
        let root = Path::new("/home/me/project");
        assert_eq!(
            relative_path(root, Path::new("/home/me/project/src/a.rs")),
            "src/a.rs"
        );
        assert_eq!(relative_path(root, root), ".");
        assert_eq!(relative_path(root, Path::new("/etc/hosts")), "/etc/hosts");
    }

    #[test]
    fn an_entry_folder_is_a_file_parent() {
        assert_eq!(entry_folder(Path::new("/r/a.md"), false), Path::new("/r"));
        assert_eq!(entry_folder(Path::new("/r/src"), true), Path::new("/r/src"));
    }

    fn quote(path: &str, shell: ShellSyntax) -> Result<String, TypeError> {
        quote_path(Path::new(path), shell)
    }

    #[test]
    fn posix_quoting() {
        let posix = ShellSyntax::Posix;
        assert_eq!(quote("/home/me/src", posix).unwrap(), "/home/me/src");
        assert_eq!(
            quote("/home/me/my dir", posix).unwrap(),
            "'/home/me/my dir'"
        );
        assert_eq!(quote("/tmp/it's", posix).unwrap(), r"'/tmp/it'\''s'");
        assert_eq!(quote("/tmp/$HOME", posix).unwrap(), "'/tmp/$HOME'");
        assert_eq!(quote("", posix).unwrap(), "''");
    }

    #[test]
    fn powershell_quoting_expands_nothing() {
        let pwsh = ShellSyntax::PowerShell;
        assert_eq!(quote(r"C:\src", pwsh).unwrap(), r"C:\src");
        assert_eq!(
            quote(r"C:\Program Files", pwsh).unwrap(),
            r"'C:\Program Files'"
        );
        assert_eq!(quote(r"C:\$(calc)", pwsh).unwrap(), r"'C:\$(calc)'");
        assert_eq!(quote(r"C:\a`b", pwsh).unwrap(), r"'C:\a`b'");
        assert_eq!(quote(r"C:\it's", pwsh).unwrap(), r"'C:\it''s'");
        assert_eq!(
            quote("C:\\it\u{2019}s", pwsh).unwrap(),
            "'C:\\it\u{2019}\u{2019}s'"
        );
    }

    #[test]
    fn cmd_quoting_refuses_what_it_cant_escape() {
        let cmd = ShellSyntax::Cmd;
        assert_eq!(quote(r"C:\src", cmd).unwrap(), r"C:\src");
        assert_eq!(
            quote(r"C:\Program Files", cmd).unwrap(),
            r#""C:\Program Files""#
        );
        assert_eq!(quote(r"C:\a&b", cmd).unwrap(), r#""C:\a&b""#);
        assert_eq!(quote(r"C:\%PATH%", cmd), Err(TypeError::CmdSpecial));
        assert_eq!(quote(r#"C:\a"b"#, cmd), Err(TypeError::CmdSpecial));
    }

    #[test]
    fn control_characters_are_never_typed() {
        for shell in [
            ShellSyntax::Posix,
            ShellSyntax::PowerShell,
            ShellSyntax::Cmd,
        ] {
            for path in [
                "/r/a\x03touch pwned\r",
                "/r/a\nb",
                "/r/\x1b[A",
                "/r/\x15",
                "/r/\x7f",
            ] {
                assert_eq!(quote(path, shell), Err(TypeError::ControlCharacter));
                assert_eq!(
                    cd_command(Path::new(path), shell),
                    Err(TypeError::ControlCharacter)
                );
                for mention in [false, true] {
                    assert_eq!(
                        context_reference(Path::new(path), shell, mention),
                        Err(TypeError::ControlCharacter)
                    );
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_refused() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"/r/a\xffb"));
        assert_eq!(typeable(path), Err(TypeError::NotUtf8));
        assert_eq!(
            context_reference(path, ShellSyntax::Posix, true),
            Err(TypeError::NotUtf8)
        );
    }

    #[test]
    fn cd_commands_follow_the_shell() {
        let folder = Path::new("/r/my dir");
        assert_eq!(
            cd_command(folder, ShellSyntax::Posix).unwrap(),
            "cd '/r/my dir'\r"
        );
        assert_eq!(
            cd_command(folder, ShellSyntax::PowerShell).unwrap(),
            "Set-Location -LiteralPath '/r/my dir'\r"
        );
        assert_eq!(
            cd_command(folder, ShellSyntax::Cmd).unwrap(),
            "cd /d \"/r/my dir\"\r"
        );
    }

    #[test]
    fn shell_syntax_from_the_program() {
        assert_eq!(
            ShellSyntax::of_program("/usr/bin/pwsh"),
            ShellSyntax::PowerShell
        );
        assert_eq!(
            ShellSyntax::of_program(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            ShellSyntax::PowerShell
        );
        assert_eq!(ShellSyntax::of_program("CMD.EXE"), ShellSyntax::Cmd);
        assert_eq!(ShellSyntax::of_program("/bin/bash"), ShellSyntax::Posix);
        assert_eq!(ShellSyntax::of_program("wsl.exe"), ShellSyntax::Posix);
    }

    #[test]
    fn context_references_mention_in_claude_code_and_quote_in_shells() {
        let refer = |path: &str, mention| {
            context_reference(Path::new(path), ShellSyntax::Posix, mention).unwrap()
        };
        assert_eq!(refer("/r/a.md", true), "@/r/a.md ");
        assert_eq!(refer("/r/my notes.md", true), "@\"/r/my notes.md\" ");
        assert_eq!(refer("/r/my notes.md", false), "'/r/my notes.md' ");
        // A `"` would end the mention early: the path is typed as is.
        assert_eq!(refer("/r/say \"hi\".md", true), "/r/say \"hi\".md ");
    }

    #[test]
    fn changed_paths_reread_their_watched_folder() {
        let watched: HashMap<PathBuf, Vec<PathBuf>> = HashMap::from([
            (PathBuf::from("/r"), vec![PathBuf::from("/r")]),
            // A folder shown through a symlink and its real path.
            (
                PathBuf::from("/real/lib"),
                vec![PathBuf::from("/r/lib"), PathBuf::from("/r/link")],
            ),
        ]);
        let changed = |paths: &[&str]| {
            let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let mut folders: Vec<PathBuf> =
                folders_to_reread(&paths, &watched).into_iter().collect();
            folders.sort();
            folders
        };
        assert_eq!(changed(&["/r/a.md"]), [Path::new("/r")]);
        assert_eq!(
            changed(&["/real/lib/x.rs", "/r/b.md"]),
            [Path::new("/r"), Path::new("/r/lib"), Path::new("/r/link")]
        );
        // The watched folder itself changed (removed, replaced).
        assert_eq!(
            changed(&["/real/lib"]),
            [Path::new("/r/lib"), Path::new("/r/link")]
        );
        assert!(changed(&["/elsewhere/a", "/"]).is_empty());
    }

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(validate_name("  notes.md "), Ok("notes.md"));
        assert_eq!(validate_name(".env"), Ok(".env"));
        assert_eq!(validate_name("   "), Err(NameError::Empty));
        assert_eq!(validate_name(".."), Err(NameError::Reserved));
        assert_eq!(validate_name("a/b"), Err(NameError::Separator));
        assert_eq!(validate_name(r"a\b"), Err(NameError::Separator));
        assert_eq!(validate_name("a\0b"), Err(NameError::Separator));
    }

    #[test]
    fn windows_names_refuse_drive_and_reserved_characters() {
        assert_eq!(
            validate_name_for("C:x", true),
            Err(NameError::WindowsCharacter)
        );
        assert_eq!(
            validate_name_for("a?.md", true),
            Err(NameError::WindowsCharacter)
        );
        assert_eq!(
            validate_name_for("a\"b", true),
            Err(NameError::WindowsCharacter)
        );
        assert_eq!(validate_name_for("C:x", false), Ok("C:x"));
        assert_eq!(validate_name_for("notes.md", true), Ok("notes.md"));
    }
}
