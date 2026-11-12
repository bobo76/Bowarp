//! Links in a terminal's visible screen: an OSC 8 hyperlink on the cells, or a URL or an
//! existing file's path in the text (`workspace_model::links`), read across lines the program
//! wrapped. Reading the screen gives a [`LinkCandidate`]; whether its file exists is checked
//! on a background thread.

use std::path::{Path, PathBuf};

use term_core::Session;
use term_core::alacritty_terminal::grid::{Dimensions, Grid};
use term_core::alacritty_terminal::index::{Column, Line};
use term_core::alacritty_terminal::term::cell::{Cell as TerminalCell, Flags};
use workspace_model::links::{self, LinkText, PathRef};

use crate::file_viewer::FilePlace;
use crate::mouse::Cell;

/// The cells of a link on one visible line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkRow {
    /// Zero-based visible line.
    pub line: usize,
    pub column: usize,
    pub cells: usize,
}

/// Where a link goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// Opened in the browser (the system's handler).
    Url(String),
    /// Opened in the file viewer.
    File { path: PathBuf, place: FilePlace },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalLink {
    pub rows: Vec<LinkRow>,
    pub target: LinkTarget,
}

/// What a link's text points to, before the file system is looked at.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Unchecked {
    Url(String),
    /// A path in the text: a link only when it names a file.
    Path {
        path: PathBuf,
        place: FilePlace,
    },
    /// A `file://` URL: opened in the viewer when `path` names a file, as a URL otherwise.
    FileUrl {
        uri: String,
        path: PathBuf,
    },
}

impl Unchecked {
    /// The target, when it does not depend on the file system.
    fn known(&self) -> Option<LinkTarget> {
        match self {
            Self::Url(url) => Some(LinkTarget::Url(url.clone())),
            Self::Path { .. } | Self::FileUrl { .. } => None,
        }
    }

    /// The target. Looks at the file system, which blocks for as long as a dead network
    /// mount takes to answer.
    fn check(self) -> Option<LinkTarget> {
        match self {
            Self::Url(url) => Some(LinkTarget::Url(url)),
            Self::Path { path, place } => {
                path.is_file().then_some(LinkTarget::File { path, place })
            }
            Self::FileUrl { uri, path } => Some(if path.is_file() {
                LinkTarget::File {
                    path,
                    place: FilePlace::default(),
                }
            } else {
                LinkTarget::Url(uri)
            }),
        }
    }
}

/// The text on a cell that reads as a link. A path in it is one only when it names an existing
/// file, which [`LinkCandidate::check`] finds out, off the UI thread: the text is written by
/// any program, and a path on a network mount that does not answer blocks its reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkCandidate {
    rows: Vec<LinkRow>,
    target: Unchecked,
}

impl LinkCandidate {
    pub fn rows(&self) -> &[LinkRow] {
        &self.rows
    }

    /// The link, when it is one whatever the file system holds (a web URL).
    pub fn known(&self) -> Option<TerminalLink> {
        Some(TerminalLink {
            rows: self.rows.clone(),
            target: self.target.known()?,
        })
    }

    /// The link, if this is one. Blocking: for a background thread.
    pub fn check(self) -> Option<TerminalLink> {
        Some(TerminalLink {
            rows: self.rows,
            target: self.target.check()?,
        })
    }
}

/// What was read under the terminal lock.
enum Read {
    Hyperlink(LinkRow, String),
    /// The logical line (wrapped rows joined), its first visible line and the row width.
    Text(Vec<char>, usize, usize),
}

/// What reads as a link on visible `cell`, if anything. Paths resolve from `folder` (the
/// tab's). Does not look at the file system.
pub fn candidate_at(
    session: &Session,
    cell: Cell,
    folder: Option<&Path>,
    home: Option<&Path>,
) -> Option<LinkCandidate> {
    let (text, first_line, columns) = match read_link(session, cell)? {
        Read::Hyperlink(row, uri) => {
            return Some(LinkCandidate {
                rows: vec![row],
                target: hyperlink_target(uri, folder)?,
            });
        }
        Read::Text(text, first_line, columns) => (text, first_line, columns),
    };
    let index = (cell.line - first_line) * columns + cell.column;
    let found = links::find_link(&text, index)?;
    let target = match found.target {
        LinkText::Url(url) => hyperlink_target(url, folder)?,
        LinkText::Path(path_ref) => path_target(path_ref, folder, home)?,
    };
    let rows = links::link_rows(found.range, columns)
        .into_iter()
        .map(|span| LinkRow {
            line: first_line + span.row,
            column: span.column,
            cells: span.cells,
        })
        .collect();
    Some(LinkCandidate { rows, target })
}

fn read_link(session: &Session, cell: Cell) -> Option<Read> {
    let term = session.lock_term();
    let grid = term.grid();
    let columns = grid.columns();
    let offset = grid.display_offset() as i32;
    let bottom = grid.screen_lines() as i32 - 1 - offset;
    let line = cell.line as i32 - offset;
    if line > bottom || cell.column >= columns {
        return None;
    }
    Some(hyperlink_span(grid, cell, line).unwrap_or_else(|| logical_line(grid, line)))
}

fn hyperlink_span(grid: &Grid<TerminalCell>, cell: Cell, line: i32) -> Option<Read> {
    let columns = grid.columns();
    let row = &grid[Line(line)];
    let hyperlink = row[Column(cell.column)].hyperlink()?;
    let same = |column: usize| row[Column(column)].hyperlink().as_ref() == Some(&hyperlink);
    let start = (0..cell.column)
        .rev()
        .take_while(|&column| same(column))
        .last()
        .unwrap_or(cell.column);
    let end = (cell.column + 1..columns)
        .take_while(|&column| same(column))
        .last()
        .map_or(cell.column + 1, |column| column + 1);
    let row = LinkRow {
        line: cell.line,
        column: start,
        cells: end - start,
    };
    Some(Read::Hyperlink(row, hyperlink.uri().to_string()))
}

fn logical_line(grid: &Grid<TerminalCell>, line: i32) -> Read {
    let columns = grid.columns();
    let offset = grid.display_offset() as i32;
    let top = -offset;
    let bottom = grid.screen_lines() as i32 - 1 - offset;
    let wraps = |line: i32| {
        grid[Line(line)][Column(columns - 1)]
            .flags
            .contains(Flags::WRAPLINE)
    };
    let mut first = line;
    while first > top && wraps(first - 1) {
        first -= 1;
    }
    let mut last = line;
    while last < bottom && wraps(last) {
        last += 1;
    }
    // One char per column, so a char's index gives its cell.
    let text: Vec<char> = (first..=last)
        .flat_map(|line| {
            let row = &grid[Line(line)];
            (0..columns).map(move |column| row[Column(column)].c)
        })
        .collect();
    Read::Text(text, (first + offset) as usize, columns)
}

fn path_target(path_ref: PathRef, folder: Option<&Path>, home: Option<&Path>) -> Option<Unchecked> {
    Some(Unchecked::Path {
        path: links::resolve_path(&path_ref.path, folder, home)?,
        place: FilePlace {
            line: path_ref.line,
            column: path_ref.column,
        },
    })
}

/// A URL link: a `file://` one naming an existing file opens in the viewer, any other in the
/// browser. `None` (not a link) for schemes not to open (`links::is_openable_hyperlink`).
fn hyperlink_target(uri: String, folder: Option<&Path>) -> Option<Unchecked> {
    if !links::is_openable_hyperlink(&uri) {
        return None;
    }
    let is_file_url = uri
        .get(.."file://".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"));
    if is_file_url && let Some(path) = links::local_link(&uri, folder.unwrap_or(Path::new("/"))) {
        return Some(Unchecked::FileUrl { uri, path });
    }
    Some(Unchecked::Url(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked(target: Option<Unchecked>) -> Option<LinkTarget> {
        target.and_then(Unchecked::check)
    }

    #[test]
    fn web_links_keep_their_url_and_unsafe_schemes_are_rejected() {
        let uri = "https://example.com/a".to_string();
        let target = hyperlink_target(uri.clone(), None).unwrap();
        assert_eq!(target.known(), Some(LinkTarget::Url(uri.clone())));
        assert_eq!(target.check(), Some(LinkTarget::Url(uri)));
        assert_eq!(hyperlink_target("javascript:alert(1)".into(), None), None);
    }

    #[test]
    fn a_file_url_opens_in_the_viewer_only_when_its_file_exists() {
        let folder = std::env::current_dir().unwrap();
        let existing = folder.join("Cargo.toml");
        let uri = format!("file:///{}", existing.display()).replace('\\', "/");
        let target = hyperlink_target(uri, Some(&folder)).unwrap();
        assert_eq!(target.known(), None);
        assert_eq!(
            target.check(),
            Some(LinkTarget::File {
                path: existing,
                place: FilePlace::default(),
            })
        );

        let missing = "file:///no/such/file-here.txt".to_string();
        let target = hyperlink_target(missing.clone(), Some(&folder)).unwrap();
        assert_eq!(target.known(), None);
        assert_eq!(target.check(), Some(LinkTarget::Url(missing)));
    }

    #[test]
    fn file_targets_keep_the_source_position() {
        let folder = std::env::current_dir().unwrap();
        let path_ref = PathRef {
            path: "Cargo.toml".into(),
            line: Some(12),
            column: Some(3),
        };
        let target = path_target(path_ref, Some(&folder), None);
        assert_eq!(target.as_ref().and_then(Unchecked::known), None);
        assert_eq!(
            checked(target),
            Some(LinkTarget::File {
                path: folder.join("Cargo.toml"),
                place: FilePlace {
                    line: Some(12),
                    column: Some(3)
                },
            })
        );
        assert_eq!(
            checked(path_target(
                PathRef {
                    path: ".".into(),
                    line: None,
                    column: None
                },
                Some(&folder),
                None
            )),
            None
        );
    }
}
