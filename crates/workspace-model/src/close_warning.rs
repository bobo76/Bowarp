//! Warnings before closing tabs that are busy. A tab is busy while a shell command runs (its
//! OSC 133 `C` mark came without the `D`) or while its agent is Working or waiting for the
//! user; idle prompts, idle agents and exited programs close without a question. The warning
//! is a safeguard, not a notification: no switch turns it off.

/// What an agent tab's status says about closing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentActivity {
    Idle,
    Working,
    /// A permission prompt or question waits for the user's answer.
    Waiting,
}

/// What a tab does now, as far as closing it goes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabActivity {
    /// The tab's program has exited: nothing is left to stop.
    pub exited: bool,
    /// The agent's status; `None` for shells and agents that report no status.
    pub agent: Option<AgentActivity>,
    /// A shell command runs (`C` mark without `D`).
    pub command_running: bool,
    /// The running command's line as the hover card shows it, when the shell reported it.
    pub command: Option<String>,
}

/// Why closing a tab would stop work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusyReason {
    /// A shell command runs, with its line when the shell reported it.
    Command(Option<String>),
    AgentWorking,
    AgentWaiting,
}

impl TabActivity {
    /// Why closing the tab would stop work, or `None` when it can close silently. An agent's
    /// status decides for agent tabs.
    pub fn busy_reason(&self) -> Option<BusyReason> {
        if self.exited {
            return None;
        }
        match self.agent {
            Some(AgentActivity::Working) => return Some(BusyReason::AgentWorking),
            Some(AgentActivity::Waiting) => return Some(BusyReason::AgentWaiting),
            Some(AgentActivity::Idle) | None => {}
        }
        self.command_running
            .then(|| BusyReason::Command(self.command.clone()))
    }
}

/// A busy tab, as the warning lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusyTab {
    pub name: String,
    pub reason: BusyReason,
}

/// What is being closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseTarget {
    /// One tab (its close button, the menu's Close, the close shortcut).
    Tab,
    /// Several tabs at once (a group's tabs, other tabs, tabs below).
    Tabs,
    /// The window and every tab in it.
    Window,
}

/// The question asked before closing busy tabs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseWarning {
    pub title: String,
    /// The sentence above the list (or the whole text for a single tab).
    pub intro: String,
    /// One line per busy tab, the last one counting those left out when there are many.
    pub lines: Vec<String>,
    /// The confirming button's text; the other button is Cancel.
    pub ok_text: &'static str,
}

/// The most busy tabs the warning names; the rest are counted.
pub const MAX_LISTED: usize = 8;

/// The warning for closing `target` while `busy` tabs work, or `None` when none is busy.
pub fn close_warning(target: CloseTarget, busy: &[BusyTab]) -> Option<CloseWarning> {
    let first = busy.first()?;
    Some(match target {
        CloseTarget::Tab => tab_warning(first),
        CloseTarget::Tabs => list_warning(busy, "Close the tabs?", "Close tabs", "the tabs"),
        CloseTarget::Window => {
            list_warning(busy, "Close the window?", "Close window", "the window")
        }
    })
}

fn tab_warning(tab: &BusyTab) -> CloseWarning {
    CloseWarning {
        title: format!("Close \"{}\"?", tab.name),
        intro: format!("{} Closing the tab ends it.", busy_sentence(&tab.reason)),
        lines: Vec::new(),
        ok_text: "Close tab",
    }
}

/// The warning that lists the busy tabs; `closing` is what is being closed, as a noun phrase.
fn list_warning(
    busy: &[BusyTab],
    title: &str,
    ok_text: &'static str,
    closing: &str,
) -> CloseWarning {
    let intro = match busy.len() {
        1 => format!("A tab is still busy. Closing {closing} ends what runs in it:"),
        n => format!("{n} tabs are still busy. Closing {closing} ends what runs in them:"),
    };
    let mut lines: Vec<String> = busy.iter().take(MAX_LISTED).map(busy_list_line).collect();
    if busy.len() > MAX_LISTED {
        lines.push(format!("and {} more", busy.len() - MAX_LISTED));
    }
    CloseWarning {
        title: title.to_string(),
        intro,
        lines,
        ok_text,
    }
}

fn busy_sentence(reason: &BusyReason) -> String {
    match reason {
        BusyReason::Command(Some(command)) => format!("\"{command}\" is still running."),
        BusyReason::Command(None) => "A command is still running.".to_string(),
        BusyReason::AgentWorking => "The agent is still working.".to_string(),
        BusyReason::AgentWaiting => "The agent is waiting for your answer.".to_string(),
    }
}

fn busy_list_line(tab: &BusyTab) -> String {
    let what = match &tab.reason {
        BusyReason::Command(Some(command)) => format!("running \"{command}\""),
        BusyReason::Command(None) => "running a command".to_string(),
        BusyReason::AgentWorking => "agent working".to_string(),
        BusyReason::AgentWaiting => "agent waiting for your answer".to_string(),
    };
    format!("{}: {what}", tab.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(agent: Option<AgentActivity>, command_running: bool) -> TabActivity {
        TabActivity {
            exited: false,
            agent,
            command_running,
            command: command_running.then(|| "cargo build".to_string()),
        }
    }

    fn busy(name: &str, reason: BusyReason) -> BusyTab {
        BusyTab {
            name: name.to_string(),
            reason,
        }
    }

    #[test]
    fn idle_prompts_and_idle_agents_are_not_busy() {
        assert_eq!(activity(None, false).busy_reason(), None);
        assert_eq!(
            activity(Some(AgentActivity::Idle), false).busy_reason(),
            None
        );
    }

    #[test]
    fn a_running_command_is_busy_with_its_line() {
        assert_eq!(
            activity(None, true).busy_reason(),
            Some(BusyReason::Command(Some("cargo build".into())))
        );
        let unnamed = TabActivity {
            command_running: true,
            ..TabActivity::default()
        };
        assert_eq!(unnamed.busy_reason(), Some(BusyReason::Command(None)));
    }

    #[test]
    fn working_and_waiting_agents_are_busy() {
        assert_eq!(
            activity(Some(AgentActivity::Working), false).busy_reason(),
            Some(BusyReason::AgentWorking)
        );
        assert_eq!(
            activity(Some(AgentActivity::Waiting), false).busy_reason(),
            Some(BusyReason::AgentWaiting)
        );
        // The agent's status wins over a command mark.
        assert_eq!(
            activity(Some(AgentActivity::Working), true).busy_reason(),
            Some(BusyReason::AgentWorking)
        );
    }

    #[test]
    fn an_exited_program_is_never_busy() {
        for agent in [
            None,
            Some(AgentActivity::Working),
            Some(AgentActivity::Waiting),
        ] {
            let exited = TabActivity {
                exited: true,
                ..activity(agent, true)
            };
            assert_eq!(exited.busy_reason(), None);
        }
    }

    #[test]
    fn nothing_busy_asks_nothing() {
        for target in [CloseTarget::Tab, CloseTarget::Tabs, CloseTarget::Window] {
            assert_eq!(close_warning(target, &[]), None);
        }
    }

    #[test]
    fn one_tab_is_named_in_the_title() {
        let warning = close_warning(
            CloseTarget::Tab,
            &[busy("api", BusyReason::Command(Some("npm run dev".into())))],
        )
        .unwrap();
        assert_eq!(warning.title, "Close \"api\"?");
        assert_eq!(
            warning.intro,
            "\"npm run dev\" is still running. Closing the tab ends it."
        );
        assert!(warning.lines.is_empty());
        assert_eq!(warning.ok_text, "Close tab");

        let agent = close_warning(CloseTarget::Tab, &[busy("c", BusyReason::AgentWaiting)]);
        assert_eq!(
            agent.unwrap().intro,
            "The agent is waiting for your answer. Closing the tab ends it."
        );
    }

    #[test]
    fn several_tabs_list_every_busy_one() {
        let tabs = [
            busy("api", BusyReason::Command(Some("npm run dev".into()))),
            busy("shell", BusyReason::Command(None)),
            busy("Claude Code", BusyReason::AgentWorking),
            busy("Copilot", BusyReason::AgentWaiting),
        ];
        let warning = close_warning(CloseTarget::Window, &tabs).unwrap();
        assert_eq!(warning.title, "Close the window?");
        assert_eq!(
            warning.intro,
            "4 tabs are still busy. Closing the window ends what runs in them:"
        );
        assert_eq!(
            warning.lines,
            [
                "api: running \"npm run dev\"",
                "shell: running a command",
                "Claude Code: agent working",
                "Copilot: agent waiting for your answer",
            ]
        );
        assert_eq!(warning.ok_text, "Close window");

        let group = close_warning(CloseTarget::Tabs, &tabs[2..3]).unwrap();
        assert_eq!(group.title, "Close the tabs?");
        assert_eq!(
            group.intro,
            "A tab is still busy. Closing the tabs ends what runs in it:"
        );
        assert_eq!(group.lines, ["Claude Code: agent working"]);
        assert_eq!(group.ok_text, "Close tabs");
    }

    #[test]
    fn a_long_list_is_cut_and_counted() {
        let tabs: Vec<_> = (0..MAX_LISTED + 3)
            .map(|n| busy(&format!("tab {n}"), BusyReason::AgentWorking))
            .collect();
        let warning = close_warning(CloseTarget::Window, &tabs).unwrap();
        assert_eq!(warning.lines.len(), MAX_LISTED + 1);
        assert_eq!(warning.lines.last().unwrap(), "and 3 more");
    }
}
