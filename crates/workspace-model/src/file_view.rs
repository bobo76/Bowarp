//! The file viewer's choices about a file: how it is shown, which language colors it, whether
//! its bytes can be shown at all, and JSON laid out for reading. Nothing here touches the file
//! system.

use std::fmt;

use serde::de::IgnoredAny;

use crate::file_tree::{FileKind, file_kind};

/// Files larger than this are not opened: the editor slows down well before.
pub const MAX_VIEW_BYTES: u64 = 2 * 1024 * 1024;

/// How a file is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    /// Rendered as a page, or edited as source.
    Markdown,
    /// Laid out and colored for reading, or edited as written.
    Json,
    /// SVG: drawn as an image, or edited as source.
    Svg,
    /// A raster image, drawn only.
    Image,
    /// Edited as source only.
    Source,
}

impl ViewKind {
    /// Whether the file has a view besides its source.
    pub fn has_rendered_view(self) -> bool {
        matches!(self, Self::Markdown | Self::Json | Self::Svg)
    }
}

/// How `name` is shown, from its extension.
pub fn view_kind(name: &str) -> ViewKind {
    match extension(name).as_deref() {
        Some("md" | "markdown") => ViewKind::Markdown,
        // JSONC allows comments, which the layout would refuse.
        Some("json") => ViewKind::Json,
        Some("svg") => ViewKind::Svg,
        _ if file_kind(name) == FileKind::Image => ViewKind::Image,
        _ => ViewKind::Source,
    }
}

/// The editor's name for the language `name` is written in, `"text"` when it has no colors.
pub fn editor_language(name: &str) -> &'static str {
    if name == "Cargo.lock" {
        return "toml";
    }
    match extension(name).as_deref() {
        Some("json" | "jsonc") => "json",
        Some("js" | "mjs" | "cjs" | "jsx") => "javascript",
        Some("ts" | "mts" | "cts") => "typescript",
        Some("tsx") => "tsx",
        Some("md" | "markdown") => "markdown",
        Some("toml") => "toml",
        Some("yaml" | "yml") => "yaml",
        Some("rs") => "rust",
        Some("sh" | "bash" | "zsh") => "bash",
        Some("py") => "python",
        _ => "text",
    }
}

/// The extension of `name` in lowercase. A dotfile such as `.bashrc` has none.
fn extension(name: &str) -> Option<String> {
    let (stem, extension) = name.rsplit_once('.')?;
    (!stem.is_empty()).then(|| extension.to_ascii_lowercase())
}

/// Why a file is not shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unviewable {
    TooLarge(u64),
    Binary,
    NotUtf8,
}

impl fmt::Display for Unviewable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(size) => write!(
                f,
                "The file is too large to open ({:.1} MB, the limit is {} MB).",
                *size as f64 / (1024. * 1024.),
                MAX_VIEW_BYTES / (1024 * 1024)
            ),
            Self::Binary => f.write_str("The file is binary."),
            Self::NotUtf8 => f.write_str("The file is not UTF-8 text."),
        }
    }
}

/// Whether a file of `size` bytes may be read for viewing.
pub fn check_size(size: u64) -> Result<(), Unviewable> {
    if size > MAX_VIEW_BYTES {
        return Err(Unviewable::TooLarge(size));
    }
    Ok(())
}

/// The text of a file's bytes, unless they are binary (a NUL byte) or not UTF-8.
pub fn decode_text(bytes: Vec<u8>) -> Result<String, Unviewable> {
    check_size(bytes.len() as u64)?;
    if bytes.contains(&0) {
        return Err(Unviewable::Binary);
    }
    String::from_utf8(bytes).map_err(|_| Unviewable::NotUtf8)
}

/// Where and why JSON could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// 1-based.
    pub line: usize,
    /// 1-based.
    pub column: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}

/// `text` laid out with one member per line, indented by two spaces. Keys keep their order and
/// strings and numbers stay exactly as written.
pub fn pretty_json(text: &str) -> Result<String, JsonError> {
    if let Err(err) = serde_json::from_str::<IgnoredAny>(text) {
        // The message ends with " at line L column C", already given apart.
        let message = err.to_string();
        let message = match message.rfind(" at line ") {
            Some(end) => message[..end].to_string(),
            None => message,
        };
        return Err(JsonError {
            line: err.line(),
            column: err.column(),
            message,
        });
    }
    Ok(layout_json(text))
}

/// Lay out JSON already known to be valid.
fn layout_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                let mut escaped = false;
                for c in chars.by_ref() {
                    out.push(c);
                    match c {
                        _ if escaped => escaped = false,
                        '\\' => escaped = true,
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                while chars.next_if(|c| c.is_whitespace()).is_some() {}
                // An empty object or array stays on one line.
                if let Some(close) = chars.next_if(|next| matches!(next, '}' | ']')) {
                    out.push(close);
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_kinds() {
        assert_eq!(view_kind("PLAN.md"), ViewKind::Markdown);
        assert_eq!(view_kind("notes.MARKDOWN"), ViewKind::Markdown);
        assert_eq!(view_kind("package.json"), ViewKind::Json);
        assert_eq!(view_kind("tsconfig.jsonc"), ViewKind::Source);
        assert_eq!(view_kind("main.js"), ViewKind::Source);
        assert_eq!(view_kind(".md"), ViewKind::Source);
        assert_eq!(view_kind("logo.svg"), ViewKind::Svg);
        assert_eq!(view_kind("shot.PNG"), ViewKind::Image);
        assert_eq!(view_kind("photo.avif"), ViewKind::Image);
        assert!(!ViewKind::Source.has_rendered_view());
        assert!(!ViewKind::Image.has_rendered_view());
        assert!(ViewKind::Json.has_rendered_view());
        assert!(ViewKind::Svg.has_rendered_view());
    }

    #[test]
    fn editor_languages() {
        assert_eq!(editor_language("a.json"), "json");
        assert_eq!(editor_language("a.jsonc"), "json");
        assert_eq!(editor_language("app.mjs"), "javascript");
        assert_eq!(editor_language("App.JSX"), "javascript");
        assert_eq!(editor_language("index.ts"), "typescript");
        assert_eq!(editor_language("App.tsx"), "tsx");
        assert_eq!(editor_language("Cargo.toml"), "toml");
        assert_eq!(editor_language("Cargo.lock"), "toml");
        assert_eq!(editor_language("ci.yml"), "yaml");
        assert_eq!(editor_language("main.rs"), "rust");
        assert_eq!(editor_language("run.sh"), "bash");
        assert_eq!(editor_language("README"), "text");
        assert_eq!(editor_language(".bashrc"), "text");
        assert_eq!(editor_language("notes.txt"), "text");
    }

    #[test]
    fn text_is_decoded_unless_binary_or_not_utf8() {
        assert_eq!(decode_text(b"hello\n".to_vec()), Ok("hello\n".into()));
        assert_eq!(
            decode_text(b"\x89PNG\0\0".to_vec()),
            Err(Unviewable::Binary)
        );
        assert_eq!(
            decode_text(vec![0xff, 0xfe, b'a']),
            Err(Unviewable::NotUtf8)
        );
        assert_eq!(check_size(MAX_VIEW_BYTES), Ok(()));
        assert_eq!(
            check_size(MAX_VIEW_BYTES + 1),
            Err(Unviewable::TooLarge(MAX_VIEW_BYTES + 1))
        );
    }

    #[test]
    fn json_is_laid_out_in_order_as_written() {
        let text = r#"{"success":{"message":"Hi, \"you\": [1]","status_code":200},
            "b":[1.50,  2e3,true,null], "a": {}, "empty":[ ]}"#;
        let expected = r#"{
  "success": {
    "message": "Hi, \"you\": [1]",
    "status_code": 200
  },
  "b": [
    1.50,
    2e3,
    true,
    null
  ],
  "a": {},
  "empty": []
}"#;
        assert_eq!(pretty_json(text).unwrap(), expected);
    }

    #[test]
    fn json_scalars_and_backslashes_in_strings() {
        assert_eq!(pretty_json(" 42 ").unwrap(), "42");
        assert_eq!(
            pretty_json(r#"["a\\", "b"]"#).unwrap(),
            "[\n  \"a\\\\\",\n  \"b\"\n]"
        );
    }

    #[test]
    fn invalid_json_says_where() {
        let err = pretty_json("[\n  1,\n]").unwrap_err();
        assert_eq!((err.line, err.column), (3, 1));
        assert_eq!(err.message, "expected value");
        assert_eq!(err.to_string(), "Line 3, column 1: expected value");
    }
}
