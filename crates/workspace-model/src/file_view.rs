//! The file viewer's choices about a file: how it is shown, which language colors it, whether
//! its bytes can be shown at all, and JSON laid out for reading. Nothing here touches the file
//! system.

use std::fmt;

use serde::de::IgnoredAny;

use crate::file_tree::{FileKind, file_kind};

/// Files larger than this are not opened: the editor slows down well before.
const BYTES_PER_MIB: u64 = 1024 * 1024;
pub const MAX_VIEW_BYTES: u64 = 2 * BYTES_PER_MIB;

/// Still images larger than this are not shown. They are decoded and drawn by GPUI, never put
/// in the editor. This bounds the file GPUI reads, not the decoded size: the `image` crate caps
/// a still image's decode at 512 MiB, and [`MAX_IMAGE_SIDE`] keeps it within one GPU texture.
/// GIF and WebP may be animated, and every frame is decoded with no cap, so they keep
/// [`MAX_VIEW_BYTES`] (see [`image_size_limit`]).
pub const MAX_IMAGE_BYTES: u64 = 50 * BYTES_PER_MIB;

/// Images wider or taller than this many pixels are not shown: GPUI draws an image from one GPU
/// texture, and a larger one draws blank. 16384 is the texture limit of most desktop GPUs.
pub const MAX_IMAGE_SIDE: u32 = 16384;

/// How many bytes from the start of an image file [`image_size_limit`] needs.
pub const IMAGE_HEADER_LEN: usize = 12;
const WEBP_FORMAT_OFFSET: usize = 8;
const WEBP_FORMAT: &[u8] = b"WEBP";

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
    /// The file's size and the limit it is over, in bytes.
    TooLarge {
        size: u64,
        limit: u64,
    },
    /// The image's size and the limit on each side, in pixels.
    TooManyPixels {
        width: u32,
        height: u32,
        limit: u32,
    },
    Binary,
    NotUtf8,
}

impl fmt::Display for Unviewable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { size, limit } => write!(
                f,
                "The file is too large to open ({:.1} MB, the limit is {} MB).",
                *size as f64 / BYTES_PER_MIB as f64,
                limit / BYTES_PER_MIB
            ),
            Self::TooManyPixels {
                width,
                height,
                limit,
            } => write!(
                f,
                "The image is too large to show ({width} x {height} pixels, the limit is {limit} \
                 on each side)."
            ),
            Self::Binary => f.write_str("The file is binary."),
            Self::NotUtf8 => f.write_str("The file is not UTF-8 text."),
        }
    }
}

/// Whether a file of `size` bytes may be read for viewing as text.
pub fn check_size(size: u64) -> Result<(), Unviewable> {
    check_limit(size, MAX_VIEW_BYTES)
}

/// The size limit for an image file that starts with `header` (its first
/// [`IMAGE_HEADER_LEN`] bytes, or fewer if the file is shorter). GIF and WebP may be animated,
/// so they keep the text limit; other images get [`MAX_IMAGE_BYTES`]. The format is told from
/// the bytes, as GPUI does, so a misnamed file gets the limit of what it really is.
pub fn image_size_limit(header: &[u8]) -> u64 {
    let gif = header.starts_with(b"GIF8");
    let webp = header.starts_with(b"RIFF")
        && header.get(WEBP_FORMAT_OFFSET..IMAGE_HEADER_LEN) == Some(WEBP_FORMAT);
    if gif || webp {
        return MAX_VIEW_BYTES;
    }
    MAX_IMAGE_BYTES
}

/// Whether an image file of `size` bytes that starts with `header` may be shown.
pub fn check_image_size(size: u64, header: &[u8]) -> Result<(), Unviewable> {
    check_limit(size, image_size_limit(header))
}

/// Whether an image of `width` x `height` pixels may be shown.
pub fn check_image_dimensions(width: u32, height: u32) -> Result<(), Unviewable> {
    if width.max(height) > MAX_IMAGE_SIDE {
        return Err(Unviewable::TooManyPixels {
            width,
            height,
            limit: MAX_IMAGE_SIDE,
        });
    }
    Ok(())
}

fn check_limit(size: u64, limit: u64) -> Result<(), Unviewable> {
    if size > limit {
        return Err(Unviewable::TooLarge { size, limit });
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

/// Whether saving over the file would drop changes made outside the viewer: the file read now
/// (`on_disk`) differs from `saved`, the text last read or saved. A file that can't be read
/// (deleted, say) has nothing to lose.
pub fn changed_on_disk(saved: &str, on_disk: std::io::Result<Vec<u8>>) -> bool {
    on_disk.is_ok_and(|bytes| bytes != saved.as_bytes())
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
            '"' => copy_string_literal(&mut chars, &mut out),
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

fn copy_string_literal(chars: &mut impl Iterator<Item = char>, out: &mut String) {
    out.push('"');
    let mut escaped = false;
    for character in chars {
        out.push(character);
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
