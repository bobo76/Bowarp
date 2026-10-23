//! Links in a terminal's visible screen: an OSC 8 hyperlink on the cells, or a URL or an
//! existing file's path in the text (`workspace_model::links`), read across lines the program
//! wrapped.

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

/// What was read under the terminal lock.
enum Read {
    Hyperlink(LinkRow, String),
    /// The logical line (wrapped rows joined), its first visible line and the row width.
    Text(Vec<char>, usize, usize),
}

/// The link on visible `cell`, if any. Paths resolve from `folder` (the tab's) and count only
/// when they name an existing file.
pub fn link_at(
    session: &Session,
    cell: Cell,
    folder: Option<&Path>,
    home: Option<&Path>,
) -> Option<TerminalLink> {
    let read = read_link(session, cell)?;
    // The terminal is unlocked before looking at the file system.
    let (text, first_line, columns) = match read {
        Read::Hyperlink(row, uri) => {
            return Some(TerminalLink {
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
    Some(TerminalLink { rows, target })
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

fn path_target(
    path_ref: PathRef,
    folder: Option<&Path>,
    home: Option<&Path>,
) -> Option<LinkTarget> {
    let path = links::resolve_path(&path_ref.path, folder, home)?;
    if !path.is_file() {
        return None;
    }
    Some(LinkTarget::File {
        path,
        place: FilePlace {
            line: path_ref.line,
            column: path_ref.column,
        },
    })
}

/// A URL link: a `file://` one naming an existing file opens in the viewer, any other in the
/// browser. `None` (not a link) for schemes not to open (`links::is_openable_hyperlink`).
fn hyperlink_target(uri: String, folder: Option<&Path>) -> Option<LinkTarget> {
    if !links::is_openable_hyperlink(&uri) {
        return None;
    }
    let is_file_url = uri
        .get(.."file://".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file://"));
    if is_file_url
        && let Some(path) = links::local_link(&uri, folder.unwrap_or(Path::new("/")))
        && path.is_file()
    {
        return Some(LinkTarget::File {
            path,
            place: FilePlace::default(),
        });
    }
    Some(LinkTarget::Url(uri))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_links_keep_their_url_and_unsafe_schemes_are_rejected() {
        let uri = "https://example.com/a".to_string();
        assert_eq!(
            hyperlink_target(uri.clone(), None),
            Some(LinkTarget::Url(uri))
        );
        assert_eq!(hyperlink_target("javascript:alert(1)".into(), None), None);
    }

    #[test]
    fn file_targets_keep_the_source_position() {
        let folder = std::env::current_dir().unwrap();
        let path_ref = PathRef {
            path: "Cargo.toml".into(),
            line: Some(12),
            column: Some(3),
        };
        assert_eq!(
            path_target(path_ref, Some(&folder), None),
            Some(LinkTarget::File {
                path: folder.join("Cargo.toml"),
                place: FilePlace {
                    line: Some(12),
                    column: Some(3)
                },
            })
        );
        assert_eq!(
            path_target(
                PathRef {
                    path: ".".into(),
                    line: None,
                    column: None
                },
                Some(&folder),
                None
            ),
            None
        );
    }
}
