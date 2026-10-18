use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon/agent-workspace.ico");
    git_commit();

    // Embeds the .exe icon shown by Explorer and the taskbar. Only for native Windows builds.
    #[cfg(windows)]
    {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("assets/app-icon/agent-workspace.ico");
        resource
            .compile()
            .expect("failed to embed the Windows icon");
    }
}

/// Sets `AGENTWS_GIT_COMMIT` to the short commit hash for the About dialog when building from a
/// git checkout. Without git or a checkout (a source tarball) it sets nothing.
fn git_commit() {
    // Only this workspace's own checkout: a tarball unpacked inside another repository must not
    // report that repository's commit. `.git` is a folder, or a file in a linked worktree.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if !workspace_root.join(".git").exists() {
        return;
    }
    let Some(commit) = git(&workspace_root, &["rev-parse", "--short", "HEAD"]) else {
        return;
    };
    println!("cargo:rustc-env=AGENTWS_GIT_COMMIT={commit}");

    // Rerun when HEAD moves (checkout, commit) but not on every build. `--git-path` puts each
    // file in the worktree's or the common git folder. `logs/HEAD` is appended on every move of
    // HEAD, so it also catches a commit on a branch that is only in `packed-refs`; watching
    // `packed-refs` itself would recompile after every fetch or gc. Cargo reruns on every build
    // for a missing path, so only existing files are named.
    let mut files = vec!["HEAD".to_owned(), "logs/HEAD".to_owned()];
    files.extend(git(&workspace_root, &["symbolic-ref", "-q", "HEAD"]));
    let mut args = vec!["rev-parse"];
    for file in &files {
        args.extend(["--git-path", file.as_str()]);
    }
    let Some(paths) = git(&workspace_root, &args) else {
        return;
    };
    for path in paths.lines().map(|path| workspace_root.join(path)) {
        if path.is_file() {
            rerun_if_changed(&path);
        }
    }
}

fn rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

/// A git command's trimmed output, or `None` when git is missing or fails.
/// Runs in `dir`, the folder checked for `.git`, not the build script's own folder.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}
