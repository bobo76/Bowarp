use super::*;

fn names(entries: &[DirEntry]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

#[test]
fn folders_come_first_then_names_ignoring_case() {
    let mut entries = vec![
        DirEntry::new("b.md", EntryKind::File),
        DirEntry::new("Zeta", EntryKind::Folder),
        DirEntry::new(".hidden", EntryKind::File),
        DirEntry::new("alpha", EntryKind::Folder),
        DirEntry::new("A.txt", EntryKind::File),
        DirEntry::new(".git", EntryKind::Folder),
        DirEntry::new("a.txt", EntryKind::File),
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
                DirEntry::new("src", EntryKind::Folder),
                DirEntry::new("unread", EntryKind::Folder),
                DirEntry::new("README.md", EntryKind::File),
            ],
        ),
        (
            PathBuf::from("/r/src"),
            vec![DirEntry::new("lib.rs", EntryKind::File)],
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
    assert_eq!(rows[0].kind, EntryKind::Folder);
}

#[test]
fn entry_names_use_the_final_component_or_the_root() {
    assert_eq!(
        entry_name(Path::new("project").join("notes.md").as_path()),
        "notes.md"
    );
    assert_eq!(
        entry_name(Path::new("/")),
        Path::new("/").display().to_string()
    );
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
    assert_eq!(
        entry_folder(Path::new("/r/a.md"), EntryKind::File),
        Path::new("/r")
    );
    assert_eq!(
        entry_folder(Path::new("/r/src"), EntryKind::Folder),
        Path::new("/r/src")
    );
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
        ShellSyntax::Wsl,
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
        quote_path(path, ShellSyntax::Posix),
        Err(TypeError::NotUtf8)
    );
}

#[test]
fn parent_folder_stops_at_the_root() {
    assert_eq!(parent_folder(Path::new("/r/a")), Some(Path::new("/r")));
    assert_eq!(parent_folder(Path::new("/r")), Some(Path::new("/")));
    assert_eq!(parent_folder(Path::new("/")), None);
    assert_eq!(parent_folder(Path::new("")), None);
    assert_eq!(parent_folder(Path::new("a")), None);
}

#[cfg(windows)]
#[test]
fn parent_folder_stops_at_a_drive_root() {
    assert_eq!(parent_folder(Path::new(r"C:\r")), Some(Path::new(r"C:\")));
    assert_eq!(parent_folder(Path::new(r"C:\")), None);
    assert_eq!(parent_folder(Path::new(r"\\server\share\")), None);
}

#[test]
fn wsl_paths_use_the_distro_form() {
    let wsl = ShellSyntax::Wsl;
    assert_eq!(
        quote(r"C:\Users\Public", wsl).unwrap(),
        "/mnt/c/Users/Public"
    );
    assert_eq!(quote(r"D:\my dir", wsl).unwrap(), "'/mnt/d/my dir'");
    assert_eq!(quote(r"C:\", wsl).unwrap(), "/mnt/c/");
    assert_eq!(quote(r"\\?\C:\Users", wsl).unwrap(), "/mnt/c/Users");
    assert_eq!(quote(r"\\wsl$\Ubuntu\home\me", wsl).unwrap(), "/home/me");
    assert_eq!(
        quote(r"\\?\UNC\wsl.localhost\Ubuntu\etc", wsl).unwrap(),
        "/etc"
    );
    assert_eq!(quote(r"\\wsl$\Ubuntu", wsl).unwrap(), "/");
    assert_eq!(quote("/home/me", wsl).unwrap(), "/home/me");
    assert_eq!(quote(r"\\server\share", wsl), Err(TypeError::OutsideWsl));
    assert_eq!(quote(r"\\wsl$", wsl), Err(TypeError::OutsideWsl));
    assert_eq!(
        cd_command(Path::new(r"C:\Users"), wsl).unwrap(),
        "cd /mnt/c/Users\r"
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
    assert_eq!(ShellSyntax::of_program("wsl.exe"), ShellSyntax::Wsl);
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
        let mut folders: Vec<PathBuf> = folders_to_reread(&paths, &watched).into_iter().collect();
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
        validate_name_for("C:x", Platform::Windows),
        Err(NameError::WindowsCharacter)
    );
    assert_eq!(
        validate_name_for("a?.md", Platform::Windows),
        Err(NameError::WindowsCharacter)
    );
    assert_eq!(
        validate_name_for("a\"b", Platform::Windows),
        Err(NameError::WindowsCharacter)
    );
    for platform in [Platform::Linux, Platform::MacOs] {
        assert_eq!(validate_name_for("C:x", platform), Ok("C:x"));
    }
    assert_eq!(
        validate_name_for("notes.md", Platform::Windows),
        Ok("notes.md")
    );
}
