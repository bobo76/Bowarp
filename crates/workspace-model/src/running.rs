//! The command a shell tab runs, as its hover card shows it ("Running · ~/start.sh"), and
//! how its badge shows over time.

use std::time::Duration;

/// A command shows as Running only once it has run this long, so quick ones (`ls`, `cd`) never
/// flash the badge.
pub const RUNNING_AFTER: Duration = Duration::from_millis(500);
/// How long the Running badge pulses (counted from the command's start). Then it stays still,
/// so a long runner (`vim`, `ssh`) costs no frames.
pub const RUNNING_PULSE_FOR: Duration = Duration::from_secs(10);

/// How a running command's badge shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunningBadge {
    /// Not yet: the command may finish before anyone would notice it.
    Hidden,
    Pulsing,
    Still,
}

/// The badge of a command that has run for `elapsed`.
pub fn running_badge(elapsed: Duration) -> RunningBadge {
    if elapsed < RUNNING_AFTER {
        RunningBadge::Hidden
    } else if elapsed < RUNNING_PULSE_FOR {
        RunningBadge::Pulsing
    } else {
        RunningBadge::Still
    }
}

/// Characters of a running command the hover card shows; a longer one is cut with `…`.
pub const COMMAND_LABEL_CHARS: usize = 40;

/// `command` on one line of at most `max_chars` characters: runs of whitespace and control
/// characters (a multi-line command's newlines, a tab) become one space, and a longer line is
/// cut with `…`. `None` for a blank command.
pub fn command_label(command: &str, max_chars: usize) -> Option<String> {
    let words: Vec<&str> = command
        .split(|c: char| c.is_whitespace() || c.is_control() || is_invisible_format(c))
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() || max_chars == 0 {
        return None;
    }
    let line = words.join(" ");
    if line.chars().count() <= max_chars {
        return Some(line);
    }
    let kept: String = line.chars().take(max_chars - 1).collect();
    Some(format!("{}…", kept.trim_end()))
}

/// Bidi controls and invisible characters, which would reorder or hide part of the label.
/// ZWNJ and ZWJ (U+200C, U+200D) stay: Persian text and emoji sequences need them.
fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_commands_are_kept() {
        assert_eq!(
            command_label("~/start_sensor_be.sh", 40).as_deref(),
            Some("~/start_sensor_be.sh")
        );
        assert_eq!(command_label("sleep 5", 7).as_deref(), Some("sleep 5"));
    }

    #[test]
    fn whitespace_and_control_characters_become_one_space() {
        assert_eq!(
            command_label("  for f in *; do\n\techo $f\r\ndone \x1b ", 40).as_deref(),
            Some("for f in *; do echo $f done")
        );
    }

    #[test]
    fn bidi_and_invisible_characters_become_one_space() {
        assert_eq!(
            command_label("ls\u{202E}txt", 40).as_deref(),
            Some("ls txt")
        );
        assert_eq!(
            command_label("\u{FEFF}echo\u{200B}\u{2066}hi\u{2069}", 40).as_deref(),
            Some("echo hi")
        );
        // Joiners are part of the text.
        assert_eq!(
            command_label("echo \u{1F469}\u{200D}\u{1F4BB}", 40).as_deref(),
            Some("echo \u{1F469}\u{200D}\u{1F4BB}")
        );
    }

    #[test]
    fn the_badge_shows_after_a_moment_and_pulses_for_a_while() {
        assert_eq!(running_badge(Duration::ZERO), RunningBadge::Hidden);
        assert_eq!(
            running_badge(RUNNING_AFTER - Duration::from_millis(1)),
            RunningBadge::Hidden
        );
        assert_eq!(running_badge(RUNNING_AFTER), RunningBadge::Pulsing);
        assert_eq!(
            running_badge(RUNNING_PULSE_FOR - Duration::from_millis(1)),
            RunningBadge::Pulsing
        );
        assert_eq!(running_badge(RUNNING_PULSE_FOR), RunningBadge::Still);
        assert_eq!(
            running_badge(Duration::from_secs(3600)),
            RunningBadge::Still
        );
    }

    #[test]
    fn long_commands_are_cut_with_an_ellipsis() {
        assert_eq!(command_label("sleep 50000", 8).as_deref(), Some("sleep 5…"));
        // No space before the ellipsis.
        assert_eq!(command_label("sleep 50000", 7).as_deref(), Some("sleep…"));
        // Counted in characters, not bytes.
        assert_eq!(command_label("échoé été", 6).as_deref(), Some("échoé…"));
    }

    #[test]
    fn blank_commands_have_no_label() {
        assert_eq!(command_label("", 40), None);
        assert_eq!(command_label(" \n\t", 40), None);
        assert_eq!(command_label("ls", 0), None);
    }
}
