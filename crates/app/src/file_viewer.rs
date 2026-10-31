//! The file viewer panel: a file clicked in the explorer (or a path Ctrl+clicked in the
//! terminal), shown beside the terminal. Markdown, JSON and SVG have a rendered view (the page,
//! the JSON laid out and colored, the drawing) and a raw one; other text files only the raw
//! one, and images only the drawing. The raw view edits the file, and Ctrl+S saves it. Each tab
//! has its own viewer.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants};
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme, IconName, Selectable, Sizable, WindowExt};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, DevicePixels, ElementId, Entity,
    EventEmitter, ImgResourceLoader, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ObjectFit, ParentElement, Render, RenderImage, Resource, SMOOTH_SVG_SCALE_FACTOR, SharedString,
    Size, StatefulInteractiveElement, Styled, StyledImage, Subscription, WeakEntity, Window, div,
    img, px,
};
use image::{ImageError, ImageReader};
use notify::{Event, EventKind};
use workspace_model::display_path;
use workspace_model::file_tree::entry_name;
use workspace_model::file_view::{self, JsonError, ViewKind};
use workspace_model::links;

use crate::folder_watcher::{FolderWatcher, WatchesFolders};

/// Shown for an image (or an SVG drawing) that does not decode.
const UNREADABLE_IMAGE: &str = "The image could not be read.";

/// What the viewer tells the workspace.
pub enum ViewerEvent {
    /// A file is now shown.
    Opened,
    /// The panel was closed.
    Closed,
    /// A Markdown link to a folder was clicked: show it in the explorer.
    RevealFolder(PathBuf),
    /// The file was saved (Ctrl+S).
    Saved(PathBuf),
}

/// A place in a file to show: 1-based, as compilers print it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilePlace {
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Rendered,
    Raw,
}

/// The rendered view of the text, as it was when the view was last shown.
enum Rendered {
    Markdown(SharedString),
    Json(Entity<EditorState>),
    InvalidJson(JsonError),
    /// An SVG drawn from the text at its own size, or `Err` when it does not parse.
    Svg(Result<Arc<RenderImage>, ()>),
}

struct OpenFile {
    kind: ViewKind,
    mode: Mode,
    /// The text as last read or saved.
    saved: String,
    editor: Entity<EditorState>,
    rendered: Option<Rendered>,
    /// Edited since last read or saved.
    dirty: bool,
    _edits: Subscription,
}

enum Content {
    Open(OpenFile),
    /// A raster image, drawn from the file (GPUI caches it by path until the file changes).
    Image,
    /// Why the file is not shown.
    Unviewable(String),
}

pub struct FileViewer {
    path: Option<PathBuf>,
    content: Option<Content>,
    /// Without a platform watcher, the file is not reloaded.
    watcher: FolderWatcher,
    watched_folder: Option<PathBuf>,
    /// For shortening the folder to `~` in the header.
    home: Option<PathBuf>,
}

impl EventEmitter<ViewerEvent> for FileViewer {}

impl WatchesFolders for FileViewer {
    const OWNER: &'static str = "file viewer";

    fn ignores(event: &Event) -> bool {
        matches!(event.kind, EventKind::Access(_))
    }

    /// Reload the file when it changed on disk.
    fn on_folder_changes(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path.as_ref().is_some_and(|open| paths.contains(open)) {
            self.reload(window, cx);
        }
    }
}

impl FileViewer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            path: None,
            content: None,
            watcher: FolderWatcher::new(window, cx),
            watched_folder: None,
            home: std::env::home_dir(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.path.is_some()
    }

    /// Show `path`, asking first when the file shown has unsaved edits.
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.open_at(path, FilePlace::default(), window, cx);
    }

    /// Show `path` with the cursor at `place` (in the raw view, where lines show), asking
    /// first when another file shown has unsaved edits.
    pub fn open_at(
        &mut self,
        path: PathBuf,
        place: FilePlace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.path.as_ref() == Some(&path) {
            self.go_to(place, window, cx);
            return;
        }
        self.discard_then(window, cx, move |viewer, window, cx| {
            viewer.load(path.clone(), window, cx);
            viewer.go_to(place, window, cx);
        });
    }

    /// Put the cursor at `place`, scrolled into view, switching to the raw view.
    fn go_to(&mut self, place: FilePlace, window: &mut Window, cx: &mut Context<Self>) {
        let Some(line) = place.line else {
            return;
        };
        self.set_mode(Mode::Raw, window, cx);
        let Some(file) = self.open_file() else {
            return;
        };
        let position = Position {
            line: line.saturating_sub(1),
            character: place.column.unwrap_or(1).saturating_sub(1),
        };
        file.editor.update(cx, |editor, cx| {
            editor.set_cursor_position(position, window, cx);
        });
    }

    /// Close the panel, asking first when the file has unsaved edits.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.discard_then(window, cx, |viewer, window, cx| {
            viewer.forget_image(window, cx);
            viewer.path = None;
            viewer.content = None;
            viewer.watch(None);
            cx.emit(ViewerEvent::Closed);
            cx.notify();
        });
    }

    #[cfg(test)]
    pub(crate) fn mark_dirty_for_test(&mut self, dirty: bool) {
        if let Some(file) = self.open_file_mut() {
            file.dirty = dirty;
        }
    }

    #[cfg(test)]
    pub(crate) fn edit_for_test(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = self.open_file_mut() {
            let editor = file.editor.clone();
            file.dirty = true;
            editor.update(cx, |editor, cx| {
                editor.set_value(text.to_owned(), window, cx)
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn save_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
    }

    /// Whether the file shown has edits not saved.
    pub(crate) fn is_dirty(&self) -> bool {
        self.open_file().is_some_and(|file| file.dirty)
    }

    /// Run `then` now, or once the user agrees to drop the unsaved edits.
    pub(crate) fn discard_then(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        self.discard_then_or(window, cx, then, |_| {});
    }

    /// [`Self::discard_then`], also telling the caller when the user cancels the prompt.
    pub(crate) fn discard_then_or(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        on_cancel: impl Fn(&mut App) + 'static,
    ) {
        if !self.is_dirty() {
            then(self, window, cx);
            return;
        }
        let name = self.file_name();
        let viewer = cx.entity().downgrade();
        let then = Rc::new(then);
        let on_cancel = Rc::new(on_cancel);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let viewer = viewer.clone();
            let then = then.clone();
            let on_cancel = on_cancel.clone();
            alert
                .title(format!("Discard changes to \"{name}\"?"))
                .description("The edits not saved with Ctrl+S are lost.")
                .ok_text("Discard")
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = viewer.update(cx, |viewer, cx| then(viewer, window, cx));
                    true
                })
                .on_cancel(move |_, _, cx| {
                    on_cancel(cx);
                    true
                })
        });
    }

    fn load(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = entry_name(&path);
        self.forget_image(window, cx);
        let kind = file_view::view_kind(&name);
        self.content = Some(if kind == ViewKind::Image {
            image_content(&path)
        } else {
            match read_text(&path) {
                Ok(text) => Content::Open(self.open_text(&name, text, window, cx)),
                Err(reason) => Content::Unviewable(reason),
            }
        });
        self.watch(path.parent());
        self.path = Some(path);
        cx.emit(ViewerEvent::Opened);
        cx.notify();
    }

    fn open_text(
        &self,
        name: &str,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> OpenFile {
        let kind = file_view::view_kind(name);
        let language = file_view::editor_language(name);
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .line_number(true)
                .default_value(text.clone())
        });
        let edits = cx.subscribe(&editor, |viewer, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event
                && let Some(file) = viewer.open_file_mut()
                && !file.dirty
            {
                file.dirty = true;
                cx.notify();
            }
        });
        let mode = if kind.has_rendered_view() {
            Mode::Rendered
        } else {
            Mode::Raw
        };
        let mut file = OpenFile {
            kind,
            mode,
            saved: text,
            editor,
            rendered: None,
            dirty: false,
            _edits: edits,
        };
        if mode == Mode::Rendered {
            file.rendered = render_text(kind, &file.saved, None, window, cx);
        }
        file
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = self.open_file_mut() else {
            return;
        };
        if file.mode == mode {
            return;
        }
        file.mode = mode;
        if mode == Mode::Rendered {
            // Show the edits made in the raw view.
            let text = file.editor.read(cx).value();
            refresh_rendered(file, &text, window, cx);
        }
        cx.notify();
    }

    /// Save the edits, asking first when the file changed on disk since it was read (an agent
    /// may have written it while the edits waited).
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(file) = self.open_file() else {
            return;
        };
        if !file_view::changed_on_disk(&file.saved, fs::read(&path)) {
            self.write(window, cx);
            return;
        }
        let name = self.file_name();
        let viewer = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let viewer = viewer.clone();
            alert
                .title(format!("\"{name}\" changed on disk"))
                .description("Saving replaces the changes made outside the viewer.")
                .ok_text("Overwrite")
                .ok_variant(ButtonVariant::Danger)
                .show_cancel(true)
                .on_ok(move |_, window, cx| {
                    let _ = viewer.update(cx, |viewer, cx| viewer.write(window, cx));
                    true
                })
        });
    }

    /// Write the editor's text to the file. In place, not through a renamed temporary file:
    /// that would replace a symlink with a file and drop the file's permissions and owner.
    fn write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(file) = self.open_file_mut() else {
            return;
        };
        let text = file.editor.read(cx).value().to_string();
        if let Err(err) = fs::write(&path, &text) {
            window.push_notification(Notification::error(format!("Couldn't save: {err}")), cx);
            return;
        }
        file.saved = text;
        file.dirty = false;
        cx.emit(ViewerEvent::Saved(path));
        cx.notify();
    }

    /// Watch the folder holding the file shown: a file's own watch would end when an editor
    /// replaces the file by renaming another over it.
    fn watch(&mut self, folder: Option<&Path>) {
        if self.watched_folder.as_deref() == folder || !self.watcher.is_available() {
            return;
        }
        if let Some(old) = self.watched_folder.take() {
            self.watcher.unwatch(&old);
        }
        if let Some(folder) = folder {
            match self.watcher.watch(folder) {
                Ok(()) => self.watched_folder = Some(folder.to_path_buf()),
                Err(err) => log::warn!("file viewer: can't watch {}: {err}", folder.display()),
            }
        }
    }

    /// Show the file as it now is on disk, unless it has unsaved edits, which are kept.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        // By the file's kind: an image that could not be read a moment ago may be back.
        let is_image = file_view::view_kind(&entry_name(&path)) == ViewKind::Image;
        if is_image {
            // Decoded again on the next paint.
            self.forget_image(window, cx);
            self.content = Some(image_content(&path));
            cx.notify();
            return;
        }
        // Gone (perhaps for a moment, while being replaced): keep showing what was read.
        let Ok(text) = read_text(&path) else {
            return;
        };
        let Some(file) = self.open_file_mut() else {
            // It may have become readable.
            self.load(path, window, cx);
            return;
        };
        if file.dirty || file.saved == text {
            return;
        }
        set_value_keeping_scroll(&file.editor, text.clone(), window, cx);
        if file.mode == Mode::Rendered {
            refresh_rendered(file, &text, window, cx);
        }
        file.saved = text;
        cx.notify();
    }

    /// Drop the decoded image of the file shown from GPUI's cache, so the memory goes and a
    /// changed file is decoded afresh.
    pub(crate) fn forget_image(&mut self, window: &mut Window, cx: &mut App) {
        match &mut self.content {
            Some(Content::Image) => {
                let Some(path) = &self.path else {
                    return;
                };
                let resource = image_resource(path);
                // The decoded image's textures, then the image.
                if let Some(Ok(image)) = window.get_asset::<ImgResourceLoader>(&resource, cx) {
                    cx.drop_image(image, Some(window));
                }
                cx.remove_asset::<ImgResourceLoader>(&resource);
            }
            Some(Content::Open(file)) => {
                if let Some(Rendered::Svg(Ok(image))) = &file.rendered {
                    cx.drop_image(image.clone(), Some(window));
                }
            }
            _ => {}
        }
    }

    /// The pixel size of the image shown, once decoded; `Some(Err)` when it does not decode.
    /// Asks for a redraw when the decoding ends.
    fn image_size(
        &self,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Size<DevicePixels>, ()>> {
        match &self.content {
            Some(Content::Image) => {
                let resource = image_resource(self.path.as_ref()?);
                let decoded = window.use_asset::<ImgResourceLoader>(&resource, cx)?;
                Some(decoded.map(|image| image.size(0)).map_err(|_| ()))
            }
            Some(Content::Open(file)) if file.mode == Mode::Rendered => match &file.rendered {
                Some(Rendered::Svg(image)) => {
                    Some(image.as_ref().map(|image| image.size(0)).map_err(|_| ()))
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// Open a link clicked in the Markdown page: a file next to it in this viewer, a folder in
    /// the explorer, a web page with the system's handler.
    fn open_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.path.as_ref().and_then(|path| path.parent());
        match folder.and_then(|folder| links::local_link(url, folder)) {
            Some(path) if path.is_file() => self.open(path, window, cx),
            Some(path) if path.is_dir() => cx.emit(ViewerEvent::RevealFolder(path)),
            // Nothing local goes to the system's handler: a device or a pipe is not opened.
            Some(path) => {
                let problem = if path.exists() {
                    "Can't open"
                } else {
                    "Can't find"
                };
                window.push_notification(
                    Notification::error(format!("{problem} {}", path.display())),
                    cx,
                );
            }
            None if url.starts_with('#') => {}
            None => cx.open_url(url),
        }
    }

    fn open_file(&self) -> Option<&OpenFile> {
        match &self.content {
            Some(Content::Open(file)) => Some(file),
            _ => None,
        }
    }

    fn open_file_mut(&mut self) -> Option<&mut OpenFile> {
        match &mut self.content {
            Some(Content::Open(file)) => Some(file),
            _ => None,
        }
    }

    fn file_name(&self) -> String {
        self.path.as_deref().map(entry_name).unwrap_or_default()
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = &keystroke.modifiers;
        if keystroke.key == "s" && modifiers.secondary() && !modifiers.shift && !modifiers.alt {
            self.save(window, cx);
            cx.stop_propagation();
        }
    }

    fn render_header(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pixel_size = match self.image_size(window, cx) {
            Some(Ok(size)) => Some(format!(
                "{} × {}",
                u32::from(size.width),
                u32::from(size.height)
            )),
            _ => None,
        };
        let muted = cx.theme().muted_foreground;
        let file = self.open_file();
        let dirty = file.is_some_and(|file| file.dirty);
        let toggle = file
            .filter(|file| file.kind.has_rendered_view())
            .map(|file| {
                ButtonGroup::new("viewer-mode")
                    .outline()
                    .xsmall()
                    .child(
                        Button::new("viewer-rendered")
                            .label("Rendered")
                            .selected(file.mode == Mode::Rendered),
                    )
                    .child(
                        Button::new("viewer-raw")
                            .label("Raw")
                            .selected(file.mode == Mode::Raw),
                    )
                    .on_click(cx.listener(|viewer, clicked: &Vec<usize>, window, cx| {
                        let mode = if clicked.contains(&0) {
                            Mode::Rendered
                        } else {
                            Mode::Raw
                        };
                        viewer.set_mode(mode, window, cx);
                    }))
            });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(crate::ui_dimensions::HEADER_HEIGHT))
            .pl_3()
            .pr_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_sm()
                    .child(if dirty {
                        format!("{} ●", self.file_name())
                    } else {
                        self.file_name()
                    }),
            )
            .children(
                pixel_size.map(|size| div().flex_none().text_xs().text_color(muted).child(size)),
            )
            .children(toggle)
            .child(
                Button::new("viewer-menu")
                    .icon(IconName::EllipsisVertical)
                    .ghost()
                    .xsmall()
                    .tooltip("File actions")
                    .dropdown_menu({
                        let viewer = cx.entity().downgrade();
                        move |menu, _, cx| viewer_menu(&viewer, menu, cx)
                    }),
            )
            .child(
                Button::new("close-viewer")
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip("Close file")
                    .on_click(cx.listener(|viewer, _, window, cx| viewer.close(window, cx))),
            )
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let file = match &self.content {
            None => return div().into_any_element(),
            Some(Content::Image) => {
                let Some(path) = &self.path else {
                    return div().into_any_element();
                };
                return image_view(img(path.clone()), muted);
            }
            Some(Content::Unviewable(reason)) => {
                return div()
                    .p_4()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("Can't show the file. {reason}"))
                    .into_any_element();
            }
            Some(Content::Open(file)) => file,
        };
        let raw_editor = || editable_code_editor(&file.editor).into_any_element();
        match (file.mode, &file.rendered) {
            (Mode::Raw, _) | (Mode::Rendered, None) => raw_editor(),
            (Mode::Rendered, Some(Rendered::Json(pretty))) => {
                readonly_code_editor(pretty).into_any_element()
            }
            (Mode::Rendered, Some(Rendered::InvalidJson(err))) => div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div()
                        .flex_none()
                        .px_3()
                        .py_2()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(format!("Not valid JSON. {err}")),
                )
                .child(div().flex_1().min_h_0().child(raw_editor()))
                .into_any_element(),
            (Mode::Rendered, Some(Rendered::Svg(Ok(image)))) => {
                image_view(img(image.clone()), muted)
            }
            (Mode::Rendered, Some(Rendered::Svg(Err(())))) => div()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child(UNREADABLE_IMAGE)
                .into_any_element(),
            (Mode::Rendered, Some(Rendered::Markdown(text))) => self.render_markdown(text, cx),
        }
    }

    fn render_markdown(&self, text: &SharedString, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let path = self
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        let folder = self
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|folder| display_path(folder, self.home.as_deref()));
        let viewer = cx.entity().downgrade();
        div()
            .id("viewer-markdown")
            .size_full()
            .overflow_y_scroll()
            .px_5()
            .py_4()
            .children(folder.map(|folder| div().mb_3().text_xs().text_color(muted).child(folder)))
            .child(
                TextView::markdown(
                    ElementId::Name(path.unwrap_or_default().into()),
                    text.clone(),
                )
                .selectable(true)
                .on_link_click(move |url, event, window, cx| {
                    on_link_click(&viewer, url, event, window, cx);
                }),
            )
            .into_any_element()
    }
}

fn viewer_menu(
    viewer: &WeakEntity<FileViewer>,
    menu: PopupMenu,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(entity) = viewer.upgrade() else {
        return menu;
    };
    let file = entity.read(cx).open_file();
    let menu = menu.item(save_item(viewer, file.is_some_and(|file| file.dirty)));
    let Some(file) = file.filter(|file| file.kind.has_rendered_view()) else {
        return menu;
    };
    [("Rendered", Mode::Rendered), ("Raw", Mode::Raw)]
        .into_iter()
        .fold(menu.separator(), |menu, (label, mode)| {
            menu.item(mode_item(viewer, label, mode, file.mode))
        })
}

fn save_item(viewer: &WeakEntity<FileViewer>, dirty: bool) -> PopupMenuItem {
    let viewer = viewer.clone();
    PopupMenuItem::new("Save")
        .disabled(!dirty)
        .on_click(move |_, window, cx| {
            let _ = viewer.update(cx, |viewer, cx| viewer.save(window, cx));
        })
}

fn mode_item(
    viewer: &WeakEntity<FileViewer>,
    label: &'static str,
    mode: Mode,
    current: Mode,
) -> PopupMenuItem {
    let viewer = viewer.clone();
    PopupMenuItem::new(label)
        .checked(mode == current)
        .on_click(move |_, window, cx| {
            let _ = viewer.update(cx, |viewer, cx| viewer.set_mode(mode, window, cx));
        })
}

/// A link clicked in the Markdown page, with the left or middle button (as without a handler)
/// or the keyboard.
fn on_link_click(
    viewer: &WeakEntity<FileViewer>,
    url: &SharedString,
    event: &ClickEvent,
    window: &mut Window,
    cx: &mut App,
) {
    // As the default handler: a touch long-press is a secondary click.
    let opens = match event {
        ClickEvent::Mouse(click) => {
            matches!(click.up.button, MouseButton::Left | MouseButton::Middle)
        }
        ClickEvent::Keyboard(_) => true,
        ClickEvent::Touch(click) => !click.long_press,
    };
    if !opens {
        return;
    }
    let _ = viewer.update(cx, |viewer, cx| viewer.open_link(url, window, cx));
}

/// An image fit to the pane and centred, never drawn larger than its own size.
fn image_view(image: gpui_kit::Img, muted: gpui_kit::Hsla) -> AnyElement {
    div()
        .size_full()
        .p_4()
        .child(
            image
                .size_full()
                .object_fit(ObjectFit::ScaleDown)
                .with_fallback(move || {
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(UNREADABLE_IMAGE)
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// GPUI's cache key for an image file, as `img(path)` uses it.
fn image_resource(path: &Path) -> Resource {
    Resource::Path(Arc::from(path))
}

/// The editor for `state`, filling its space without a border of its own.
fn editable_code_editor(state: &Entity<EditorState>) -> Editor {
    Editor::new(state).bordered(false).readonly(false).h_full()
}

fn readonly_code_editor(state: &Entity<EditorState>) -> Editor {
    editable_code_editor(state).readonly(true)
}

fn image_content(path: &Path) -> Content {
    match check_image(path) {
        Ok(()) => Content::Image,
        Err(reason) => Content::Unviewable(reason),
    }
}

fn refresh_rendered(
    file: &mut OpenFile,
    text: &str,
    window: &mut Window,
    cx: &mut Context<FileViewer>,
) {
    let previous = match file.rendered.take() {
        Some(Rendered::Svg(Ok(image))) => {
            cx.drop_image(image, Some(window));
            None
        }
        previous => previous,
    };
    file.rendered = render_text(file.kind, text, previous, window, cx);
}

/// Show `text` in `editor` where its view was: `set_value` scrolls back to the top. The
/// offset is applied after the next layout, clamped to the new text's length.
fn set_value_keeping_scroll(
    editor: &Entity<EditorState>,
    text: String,
    window: &mut Window,
    cx: &mut Context<FileViewer>,
) {
    editor.update(cx, |editor, cx| {
        let offset = editor.scroll_offset();
        editor.set_value(text, window, cx);
        editor.set_scroll_offset(offset, cx);
    });
}

/// The rendered view of `text`, reusing the editor of `previous` for JSON.
fn render_text(
    kind: ViewKind,
    text: &str,
    previous: Option<Rendered>,
    window: &mut Window,
    cx: &mut Context<FileViewer>,
) -> Option<Rendered> {
    match kind {
        ViewKind::Markdown => Some(Rendered::Markdown(text.to_string().into())),
        ViewKind::Svg => {
            // GPUI draws SVGs at twice their size for smoothness (its `img` paints them so); this
            // draws one at its own size, the size the header shows.
            let image = cx
                .svg_renderer()
                .render_single_frame(text.as_bytes(), 1. / SMOOTH_SVG_SCALE_FACTOR)
                .map_err(|err| log::debug!("file viewer: SVG not drawn: {err}"));
            Some(Rendered::Svg(image))
        }
        ViewKind::Json => Some(render_json(text, previous, window, cx)),
        ViewKind::Source | ViewKind::Image => None,
    }
}

fn render_json(
    text: &str,
    previous: Option<Rendered>,
    window: &mut Window,
    cx: &mut Context<FileViewer>,
) -> Rendered {
    let pretty = match file_view::pretty_json(text) {
        Ok(pretty) => pretty,
        Err(err) => return Rendered::InvalidJson(err),
    };
    if let Some(Rendered::Json(editor)) = previous {
        set_value_keeping_scroll(&editor, pretty, window, cx);
        return Rendered::Json(editor);
    }
    Rendered::Json(cx.new(|cx| {
        EditorState::new(window, cx)
            .language("json")
            .line_number(true)
            .default_value(pretty)
    }))
}

/// Whether an image file may be shown: it exists, is within its format's size limit, and is not
/// wider or taller than a GPU texture. Only the file's header is read. Whether it decodes is
/// known once it is drawn.
fn check_image(path: &Path) -> Result<(), String> {
    let size = fs::metadata(path).map_err(|err| err.to_string())?.len();
    let mut header = Vec::with_capacity(file_view::IMAGE_HEADER_LEN);
    File::open(path)
        .and_then(|file| {
            file.take(file_view::IMAGE_HEADER_LEN as u64)
                .read_to_end(&mut header)
        })
        .map_err(|err| err.to_string())?;
    file_view::check_image_size(size, &header).map_err(|reason| reason.to_string())?;
    // A header `image` can't read is left to GPUI, which shows its own error when drawing.
    let Ok((width, height)) = ImageReader::open(path)
        .and_then(ImageReader::with_guessed_format)
        .map_err(ImageError::IoError)
        .and_then(ImageReader::into_dimensions)
    else {
        return Ok(());
    };
    file_view::check_image_dimensions(width, height).map_err(|reason| reason.to_string())
}

/// The file's text, or why it is not shown.
fn read_text(path: &Path) -> Result<String, String> {
    let size = fs::metadata(path).map_err(|err| err.to_string())?.len();
    file_view::check_size(size).map_err(|reason| reason.to_string())?;
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    file_view::decode_text(bytes).map_err(|reason| reason.to_string())
}

impl Render for FileViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (background, border) = (theme.background, theme.border);
        div()
            .id("file-viewer")
            .debug_selector(|| "file-viewer".into())
            .flex()
            .flex_col()
            .size_full()
            .bg(background)
            .border_l_1()
            .border_color(border)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_header(window, cx))
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_image_reads_the_header_for_format_and_dimensions() {
        let dir = std::env::temp_dir().join(format!("agentws-check-image-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let photo = dir.join("photo.png");
        image::RgbaImage::new(64, 32).save(&photo).unwrap();
        assert_eq!(check_image(&photo), Ok(()));

        let panorama = dir.join("panorama.png");
        image::GrayImage::new(file_view::MAX_IMAGE_SIDE + 1, 1)
            .save(&panorama)
            .unwrap();
        assert_eq!(
            check_image(&panorama),
            Err(
                "The image is too large to show (16385 x 1 pixels, the limit is 16384 on each \
                 side)."
                    .to_owned()
            )
        );

        // Named as a PNG, but GIF bytes: the GIF limit applies.
        let gif = dir.join("animated.png");
        let mut bytes = b"GIF89a".to_vec();
        bytes.resize(file_view::MAX_VIEW_BYTES as usize + 1, 0);
        fs::write(&gif, bytes).unwrap();
        assert!(check_image(&gif).unwrap_err().contains("the limit is 2 MB"));

        // A header that can't be read is left to GPUI.
        let garbage = dir.join("garbage.png");
        fs::write(&garbage, b"not an image").unwrap();
        assert_eq!(check_image(&garbage), Ok(()));

        assert!(check_image(&dir.join("missing.png")).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
