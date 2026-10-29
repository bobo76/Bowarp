use super::*;
use crate::history::HistoryEntry;

fn key(program: &str, args: &[&str], windows: bool) -> String {
    let args: Vec<String> = args.iter().map(|&arg| arg.into()).collect();
    shell_key(Some(program), &args, windows)
}

#[test]
fn shell_keys_from_program_and_args() {
    assert_eq!(key("/usr/bin/bash", &[], false), "bash");
    assert_eq!(key("/bin/zsh", &["-l"], false), "zsh");
    assert_eq!(
        key(r"C:\Program Files\PowerShell\7\pwsh.exe", &[], true),
        "pwsh"
    );
    assert_eq!(
        key(
            r"C:\WINDOWS\System32\WindowsPowerShell\v1.0\PowerShell.EXE",
            &[],
            true
        ),
        "powershell"
    );
    assert_eq!(
        key(r"C:\Program Files\Git\bin\bash.exe", &["-i", "-l"], true),
        "git-bash"
    );
    assert_eq!(
        key(r"C:\Program Files\Git\usr\bin\bash.exe", &[], true),
        "git-bash"
    );
    assert_eq!(key(r"C:\Windows\System32\bash.exe", &[], true), "wsl");
    // Outside Windows a `git` folder means nothing.
    assert_eq!(key("/opt/git/bin/bash", &[], false), "bash");
    assert_eq!(
        key(r"C:\Windows\System32\wsl.exe", &["-d", "Ubuntu"], true),
        "wsl:Ubuntu"
    );
    assert_eq!(
        key("wsl", &["--distribution", "Debian"], true),
        "wsl:Debian"
    );
    assert_eq!(key("wsl.exe", &[], true), "wsl");
    assert_eq!(key("wsl.exe", &["-d"], true), "wsl");
    assert_eq!(shell_key(None, &[], false), "shell");
    assert_eq!(key("", &[], false), "shell");
}

#[test]
fn shell_names_prefer_the_launcher_then_known_names() {
    let known = [KnownShell {
        id: "wsl:Ubuntu",
        label: "Ubuntu",
        icon: LauncherIcon::Ubuntu,
    }];
    assert_eq!(
        shell_name("wsl:Ubuntu", &known),
        ("Ubuntu".into(), LauncherIcon::Ubuntu)
    );
    assert_eq!(
        shell_name("wsl:Debian", &known),
        ("Debian".into(), LauncherIcon::Shell)
    );
    assert_eq!(
        shell_name("pwsh", &[]),
        ("PowerShell".into(), LauncherIcon::Shell)
    );
    assert_eq!(shell_name("wsl", &[]), ("WSL".into(), LauncherIcon::Shell));
    assert_eq!(shell_name("nu", &[]), ("nu".into(), LauncherIcon::Shell));
}

fn history(entries: &[(&str, Option<&str>)]) -> CommandHistory {
    let mut history = CommandHistory::new(10);
    for (time, &(command, shell)) in entries.iter().enumerate() {
        let entry = HistoryEntry::new(command, None, time as u64);
        history.add(match shell {
            Some(shell) => entry.in_shell(shell),
            None => entry,
        });
    }
    history
}

const WINDOWS_MENU: [KnownShell; 4] = [
    KnownShell {
        id: "git-bash",
        label: "Git Bash",
        icon: LauncherIcon::GitBash,
    },
    KnownShell {
        id: "pwsh",
        label: "PowerShell",
        icon: LauncherIcon::PowerShell,
    },
    KnownShell {
        id: "powershell",
        label: "Windows PowerShell",
        icon: LauncherIcon::PowerShell,
    },
    KnownShell {
        id: "wsl:Arch",
        label: "Arch",
        icon: LauncherIcon::Linux,
    },
];

fn labels(chips: &[ShellChip]) -> Vec<&str> {
    chips.iter().map(|chip| chip.label.as_str()).collect()
}

fn own(key: &str) -> HistoryScope {
    HistoryScope::Shell(key.into())
}

#[test]
fn chips_are_hidden_while_only_the_own_shell_has_commands() {
    let menu = &WINDOWS_MENU;
    assert!(history_chips(&history(&[]), "pwsh", &own("pwsh"), menu).is_empty());
    let single = history(&[("ls", Some("pwsh")), ("dir", Some("pwsh"))]);
    assert!(history_chips(&single, "pwsh", &own("pwsh"), menu).is_empty());
    // Another tab's shell: its own chip shows even without commands.
    assert_eq!(
        labels(&history_chips(&single, "git-bash", &own("git-bash"), menu)),
        ["All", "Git Bash", "PowerShell"]
    );
    // Commands saved before the upgrade show under "All" only.
    let legacy = history(&[("ls", None)]);
    assert_eq!(
        labels(&history_chips(&legacy, "pwsh", &own("pwsh"), menu)),
        ["All", "PowerShell"]
    );
}

#[test]
fn chips_follow_the_menu_then_unknown_shells_by_label() {
    let history = history(&[
        ("a", Some("wsl:Zorin")),
        ("b", Some("powershell")),
        ("c", Some("nu")),
        ("d", Some("git-bash")),
        ("e", Some("wsl:Arch")),
    ]);
    let chips = history_chips(&history, "pwsh", &own("pwsh"), &WINDOWS_MENU);
    assert_eq!(
        labels(&chips),
        [
            "All",
            "Git Bash",
            "PowerShell",
            "Windows PowerShell",
            "Arch",
            "nu",
            "Zorin"
        ]
    );
    assert_eq!(chips[0].icon, None);
    assert_eq!(chips[1].icon, Some(LauncherIcon::GitBash));
    assert_eq!(chips[6].icon, Some(LauncherIcon::Shell));
    assert_eq!(chips[6].scope, own("wsl:Zorin"));
    // A selected shell keeps its chip after its last command is deleted.
    let one = self::history(&[("a", Some("pwsh"))]);
    assert_eq!(
        labels(&history_chips(&one, "pwsh", &own("zsh"), &WINDOWS_MENU)),
        ["All", "PowerShell", "Zsh"]
    );
}

fn alt() -> HeldModifiers {
    HeldModifiers {
        alt: true,
        ..HeldModifiers::default()
    }
}

#[test]
fn alt_letter_picks_a_chip_and_cycles_through_collisions() {
    let history = history(&[
        ("a", Some("powershell")),
        ("b", Some("git-bash")),
        ("c", Some("wsl:Arch")),
        ("d", None),
    ]);
    let chips = history_chips(&history, "pwsh", &own("pwsh"), &WINDOWS_MENU);
    assert_eq!(
        labels(&chips),
        [
            "All",
            "Git Bash",
            "PowerShell",
            "Windows PowerShell",
            "Arch"
        ]
    );
    let switch =
        |selected: &HistoryScope, key: &str, held| switch_chip(&chips, selected, key, held);
    assert_eq!(switch(&own("pwsh"), "g", alt()), Some(1));
    assert_eq!(switch(&own("pwsh"), "G", alt()), Some(1));
    // Shift is allowed.
    let alt_shift = HeldModifiers {
        shift: true,
        ..alt()
    };
    assert_eq!(switch(&own("pwsh"), "g", alt_shift), Some(1));
    // "All" first, then Arch, then back.
    assert_eq!(switch(&own("pwsh"), "a", alt()), Some(0));
    assert_eq!(switch(&HistoryScope::All, "a", alt()), Some(4));
    assert_eq!(switch(&own("wsl:Arch"), "a", alt()), Some(0));
    // The only P chip, selected already, stays selected; Windows PowerShell is W.
    assert_eq!(switch(&own("pwsh"), "p", alt()), Some(2));
    assert_eq!(switch(&own("git-bash"), "p", alt()), Some(2));
    assert_eq!(switch(&own("pwsh"), "w", alt()), Some(3));
    // A letter no chip has, other keys and other modifiers don't switch.
    assert_eq!(switch(&own("pwsh"), "b", alt()), None);
    assert_eq!(switch(&own("pwsh"), "left", alt()), None);
    assert_eq!(switch(&own("pwsh"), "g", HeldModifiers::default()), None);
    let ctrl_alt = HeldModifiers {
        control: true,
        ..alt()
    };
    assert_eq!(switch(&own("pwsh"), "g", ctrl_alt), None);
    let alt_super = HeldModifiers {
        platform: true,
        ..alt()
    };
    assert_eq!(switch(&own("pwsh"), "g", alt_super), None);
    // Hidden chips: nothing to switch to.
    assert_eq!(switch_chip(&[], &own("pwsh"), "a", alt()), None);
}
