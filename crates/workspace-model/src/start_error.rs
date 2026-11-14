//! What the window says when start-up could not open what was asked for: a requested folder
//! that is not a folder, bad command-line arguments, or a first tab whose program would not
//! start. The window opens anyway (on Windows there is no console to read an error in).

use std::path::{Path, PathBuf};

/// The folder a tab opens in instead of a requested `folder` that is not a folder, and the
/// warning naming it. `home` is the user's home folder; without one the tab opens in the
/// app's working folder (`None`).
pub fn folder_fallback(folder: &Path, home: Option<PathBuf>) -> (Option<PathBuf>, String) {
    let instead = if home.is_some() {
        "the home folder"
    } else {
        "the app's working folder"
    };
    let warning = format!(
        "Not a folder: {}. Opened the tab in {instead} instead.",
        folder.display()
    );
    (home, warning)
}

/// Why start-up left the window without a tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartError {
    /// The command line could not be read: nothing to retry.
    InvalidArguments { message: String, usage: String },
    /// The first tab's program (after its fallback) would not start; `attempts` counts the
    /// start and every Retry.
    Launch {
        program: String,
        error: String,
        attempts: u32,
    },
}

impl StartError {
    pub fn launch(program: impl Into<String>, error: impl Into<String>) -> Self {
        Self::Launch {
            program: program.into(),
            error: error.into(),
            attempts: 1,
        }
    }

    pub fn heading(&self) -> String {
        match self {
            Self::InvalidArguments { .. } => "Invalid command line".into(),
            Self::Launch { program, .. } => format!("Could not start {program}"),
        }
    }

    /// The text under the heading, a paragraph per line.
    pub fn details(&self) -> Vec<String> {
        match self {
            Self::InvalidArguments { message, usage } => vec![message.clone(), usage.clone()],
            Self::Launch {
                error, attempts, ..
            } => {
                let tried = (*attempts > 1).then(|| format!("Tried {attempts} times."));
                std::iter::once(error.clone()).chain(tried).collect()
            }
        }
    }

    pub fn can_retry(&self) -> bool {
        matches!(self, Self::Launch { .. })
    }

    /// A Retry failed too: show its error (`latest`) instead, and count it.
    pub fn retry_failed(&mut self, latest: StartError) {
        if let (Self::Launch { attempts, .. }, Self::Launch { program, error, .. }) =
            (&*self, latest)
        {
            *self = Self::Launch {
                program,
                error,
                attempts: attempts + 1,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_folder_falls_back_to_home_and_names_the_path() {
        let home = PathBuf::from("/home/me");
        let (folder, warning) = folder_fallback(Path::new("/no/such"), Some(home.clone()));
        assert_eq!(folder, Some(home));
        assert_eq!(
            warning,
            "Not a folder: /no/such. Opened the tab in the home folder instead."
        );
        let (folder, warning) = folder_fallback(Path::new("/no/such"), None);
        assert_eq!(folder, None);
        assert!(
            warning.ends_with("in the app's working folder instead."),
            "{warning}"
        );
    }

    #[test]
    fn invalid_arguments_show_the_message_and_usage_without_retry() {
        let error = StartError::InvalidArguments {
            message: "unknown session kind \"bogus\"".into(),
            usage: "usage: agent-workspace [shell] [project-dir]".into(),
        };
        assert_eq!(error.heading(), "Invalid command line");
        assert_eq!(
            error.details(),
            [
                "unknown session kind \"bogus\"",
                "usage: agent-workspace [shell] [project-dir]"
            ]
        );
        assert!(!error.can_retry());
    }

    #[test]
    fn a_failed_retry_replaces_the_error_and_counts_the_attempts() {
        let mut error = StartError::launch("Bash", "No such file or directory");
        assert_eq!(error.heading(), "Could not start Bash");
        assert_eq!(error.details(), ["No such file or directory"]);
        assert!(error.can_retry());
        error.retry_failed(StartError::launch("sh", "Permission denied"));
        assert_eq!(error.heading(), "Could not start sh");
        assert_eq!(error.details(), ["Permission denied", "Tried 2 times."]);
        error.retry_failed(StartError::launch("sh", "Permission denied"));
        assert_eq!(error.details()[1], "Tried 3 times.");
    }
}
