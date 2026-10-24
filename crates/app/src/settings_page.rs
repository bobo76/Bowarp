//! The settings page, shown in the terminal area while the sidebar's Settings row is
//! selected: built on gpui-component's `setting` module (a search over the rows' titles,
//! descriptions and keywords, pages on the left, groups of rows on the right), with a banner
//! for the settings file's load or save error and an "Open settings file" button.
//!
//! A row bound to a field of `Settings` ([`bound_row`], [`switch_row`], [`text_row`],
//! [`slider_row`], [`dropdown_row`]) applies a change live and saves it once per gesture, and
//! offers "Reset to default" while the value differs from the default. Rows go into the page
//! functions below ([`appearance_groups`], [`terminal_groups`]).

use std::rc::Rc;
use std::sync::LazyLock;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::link::Link;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings as SettingsView,
};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, Disableable, Icon, IconName, Side, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    Anchor, AnyElement, App, AppContext, Axis, ClickEvent, ClipboardItem, Entity, Focusable,
    IntoElement, ParentElement, SharedString, Styled, Subscription, Window, div, px,
};
use workspace_model::{FontSize, Settings, UiTheme};

use crate::about;
use crate::assets::CODE_ICON;
use crate::launcher::{self, Launcher};
use crate::settings_file;
use crate::sidebar;
use crate::terminal_element::DEFAULT_FONT_FAMILY;

/// The settings page: the error banners, the pages, and the "Open settings file" button.
pub fn render(
    launchers: Rc<[Launcher]>,
    on_open_file: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .children(file_banners(cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .child(SettingsView::new("settings-page").pages([
                    page("Appearance", appearance_groups()),
                    page("Terminal", terminal_groups(launchers)),
                    page("About", about_groups(cx)),
                ])),
        )
        .child(
            div()
                .flex_none()
                .p_2()
                .border_t_1()
                .border_color(theme.border)
                .child(
                    Button::new("open-settings-file")
                        .icon(Icon::empty().path(CODE_ICON))
                        .label("Open settings file")
                        .outline()
                        .small()
                        .w(px(234.))
                        .tooltip("Show settings.json in the file viewer")
                        .on_click(on_open_file),
                ),
        )
}

/// The Appearance page's UI theme and terminal font.
fn appearance_groups() -> Vec<SettingGroup> {
    vec![
        SettingGroup::new().title("Theme").item(
            switch_row(
                "Light theme",
                "Use the light UI instead of dark. Terminal colors stay unchanged.",
                SettingsField::new(
                    |settings| settings.ui_theme == UiTheme::Light,
                    |settings, light| {
                        settings.ui_theme = if light { UiTheme::Light } else { UiTheme::Dark };
                    },
                ),
            )
            .keywords(["appearance", "dark", "colors"]),
        ),
        SettingGroup::new().title("Font").items([
            text_row(
                "Font family",
                format!(
                    "The terminal's font, as installed. Empty, or a font that isn't installed, \
                     uses {DEFAULT_FONT_FAMILY}."
                ),
                DEFAULT_FONT_FAMILY,
                SettingsField::new(
                    |settings| settings.font_family.clone(),
                    |settings, family| settings.font_family = family.trim().to_owned(),
                ),
            )
            .keywords(["typeface", "monospace", "text"]),
            slider_row(
                "Font size",
                "The terminal's font size, in points.",
                FONT_SIZE_SLIDER,
                SettingsField::new(
                    |settings| settings.font_size.points(),
                    |settings, size| settings.font_size = FontSize::new(size),
                ),
            )
            .keywords(["text", "zoom", "points"]),
        ]),
        tab_card_group(),
    ]
}

/// The font size slider's range and step; the file accepts `FontSize::MIN..=MAX`.
const FONT_SIZE_SLIDER: SliderRange = SliderRange {
    min: 8.,
    max: 32.,
    step: 1.,
};

/// The Terminal page's groups. Later steps add their rows (launcher paths and icons) here.
fn terminal_groups(launchers: Rc<[Launcher]>) -> Vec<SettingGroup> {
    vec![
        SettingGroup::new()
            .title("Shell")
            .item(default_shell_row(launchers.clone())),
        agent_arguments_group(&launchers),
    ]
}

fn agent_arguments_group(launchers: &[Launcher]) -> SettingGroup {
    let mut group = SettingGroup::new().title("Agent launch arguments");
    for (id, label, placeholder, field) in [
        (
            "claude",
            "Claude Code arguments",
            "--model opus",
            SettingsField::new(
                |settings| settings.claude_args.clone(),
                |settings, value| settings.claude_args = value,
            ),
        ),
        (
            "copilot",
            "Copilot CLI arguments",
            "--yolo",
            SettingsField::new(
                |settings| settings.copilot_args.clone(),
                |settings, value| settings.copilot_args = value,
            ),
        ),
    ] {
        let path = launchers
            .iter()
            .find(|launcher| launcher.id == id)
            .and_then(|launcher| launcher.program.as_deref())
            .map_or_else(|| "Not installed".into(), |path| path.display().to_string());
        group = group.item(
            text_row(
                label,
                format!(
                    "{path}\nExtra arguments for new tabs, after the app's own arguments. \
                Quotes group words; no shell expansion. Put Windows paths in single quotes \
                (for example 'C:\\Users\\me\\settings.json') or double each backslash. \
                Restored tabs keep their original arguments."
                ),
                placeholder,
                field,
            )
            .keywords(["launcher", "permissions", "mode", "command line", "path"]),
        );
    }
    group
}

fn tab_card_group() -> SettingGroup {
    let mut group = SettingGroup::new().title("Tab hover card").item(switch_row(
        "Tab hover card",
        "Show details beside a hovered tab. Turning every piece off also hides the card; \
         the switch keeps its value. Group cards are unchanged.",
        SettingsField::new(
            |settings| settings.tab_card.enabled,
            |settings, value| settings.tab_card.enabled = value,
        ),
    ));
    type Piece = (
        &'static str,
        &'static str,
        fn(&Settings) -> bool,
        fn(&mut Settings, bool),
    );
    let pieces: [Piece; 5] = [
        (
            "Status",
            "Status chip, command and elapsed working or running time.",
            |s| s.tab_card.status,
            |s, v| s.tab_card.status = v,
        ),
        (
            "Location",
            "Full folder and git branch.",
            |s| s.tab_card.location,
            |s, v| s.tab_card.location = v,
        ),
        (
            "Changes",
            "Lines added in the working tree (+N from git diff).",
            |s| s.tab_card.changes,
            |s, v| s.tab_card.changes = v,
        ),
        (
            "Program title",
            "The program's title when a custom tab name hides it.",
            |s| s.tab_card.program_title,
            |s, v| s.tab_card.program_title = v,
        ),
        (
            "Session kind",
            "Kind icon, agent or shell name, and agent mode.",
            |s| s.tab_card.session_kind,
            |s, v| s.tab_card.session_kind = v,
        ),
    ];
    for (title, description, get, set) in pieces {
        group = group.item(
            bound_row(
                title,
                description,
                SettingsField::new(get, set),
                Rc::new(|checked, binding, _, cx| {
                    let binding = binding.clone();
                    Switch::new("switch")
                        .checked(checked)
                        .disabled(!settings_file::settings(cx).tab_card.enabled)
                        .on_click(move |checked: &bool, _, cx| binding.save(*checked, cx))
                        .into_any_element()
                }),
            )
            .keywords(["tab", "hover", "card", "sidebar"]),
        );
    }
    group
}

/// The `default_shell` setting: "Automatic" (empty) or one of the installed shells, shown by
/// its menu name and saved by its [`Launcher::id`]. A value typed in the file shows as the
/// shell it picks (`Zsh` as Zsh), one that isn't installed as "(not found)".
fn default_shell_row(launchers: Rc<[Launcher]>) -> SettingItem {
    let automatic = launcher::automatic_shell(&launchers).map_or_else(
        || SharedString::from("Automatic"),
        |shell| format!("Automatic ({})", shell.label).into(),
    );
    let options: Vec<_> = std::iter::once(DropdownOption::new("", automatic))
        .chain(launcher::installed_shells(&launchers).map(|shell| {
            DropdownOption::new(shell.id.clone(), shell.label.clone())
                .icon(sidebar::launcher_icon(shell.icon))
        }))
        .collect();
    let automatic_is = if cfg!(windows) {
        "Git Bash when installed"
    } else {
        "your login shell ($SHELL); when the list doesn't hold it, Ctrl+Shift+T opens the first \
         shell listed"
    };
    dropdown_row(
        "Default shell",
        format!(
            "The shell the + menu lists first and Ctrl+Shift+T opens, also for the first tab \
             when no tabs are saved. Automatic is {automatic_is}. Tabs already open keep their \
             shell."
        ),
        DropdownChoices::new(options, |name| format!("{name} (not found)").into()),
        SettingsField::new(
            move |settings| {
                settings.default_shell().map_or_else(String::new, |value| {
                    launcher::configured_shell(&launchers, value)
                        .map_or_else(|| value.to_owned(), |shell| shell.id.clone())
                })
            },
            |settings, id| settings.default_shell = id,
        ),
    )
    .keywords([
        "terminal",
        "bash",
        "zsh",
        "powershell",
        "new tab",
        "program",
    ])
}

/// A page of `groups`. The setting module hides a page without a row: each page needs one.
fn page(title: &'static str, groups: Vec<SettingGroup>) -> SettingPage {
    SettingPage::new(title).groups(groups)
}

/// Formatted once: the page is built again at each render of the workspace.
static VERSION: LazyLock<SharedString> = LazyLock::new(|| about::version().into());

fn about_groups(cx: &App) -> Vec<SettingGroup> {
    let version = VERSION.clone();
    let folder = settings_file::file(cx)
        .path()
        .and_then(|path| path.parent())
        .map(|folder| SharedString::from(folder.display().to_string()))
        .unwrap_or_else(|| "None (no home folder): settings are not saved".into());
    let executable: SharedString = std::env::current_exe()
        .map(|path| path.display().to_string().into())
        .unwrap_or_else(|_| "Unknown".into());
    vec![
        SettingGroup::new().title(about::APP_NAME).items([
            SettingItem::new(
                "Version",
                SettingField::render(move |_, _, _| {
                    let copied = version.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(version.clone())
                        .child(
                            Button::new("copy-version")
                                .icon(IconName::Copy)
                                .ghost()
                                .xsmall()
                                .tooltip("Copy version")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copied.to_string(),
                                    ));
                                }),
                        )
                }),
            )
            .description("Quote it in bug reports.")
            .keywords(["build", "commit", "release"]),
            SettingItem::new(
                "Config folder",
                SettingField::render(move |_, _, _| div().text_sm().child(folder.clone())),
            )
            .description("Holds settings.json, tabs.json and window.json.")
            // Under the title: a path is too long to share the line.
            .layout(Axis::Vertical)
            .keywords(["path", "directory", "configuration"]),
            SettingItem::new(
                "Executable",
                SettingField::render(move |_, _, _| div().text_sm().child(executable.clone())),
            )
            .description("The running copy, when several are installed.")
            .layout(Axis::Vertical)
            .keywords(["path", "binary", "exe", "install"]),
            SettingItem::new(
                "Repository",
                SettingField::render(|_, _, _| {
                    Link::new("repository")
                        .href(about::REPOSITORY)
                        .text_sm()
                        .child(about::REPOSITORY)
                }),
            )
            .keywords(["source", "github", "website"]),
        ]),
    ]
}

/// One banner per error: the file is ignored (load), or the last change wasn't saved.
fn file_banners(cx: &App) -> Vec<AnyElement> {
    let file = settings_file::file(cx);
    let path = file
        .path()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let mut banners = Vec::new();
    if let Some(error) = file.load_error() {
        let message = format!(
            "{path} can't be read, so the default settings apply: {error}. Fix it in the \
             file, or change a setting here to move it to settings.json.bak."
        );
        banners.push(
            Alert::error("settings-load-error", message)
                .banner()
                .into_any_element(),
        );
    }
    if let Some(error) = file.save_error() {
        let message = format!(
            "The settings could not be saved to {path}: {error}. Changes apply until the app \
             closes."
        );
        banners.push(
            Alert::error("settings-save-error", message)
                .banner()
                .into_any_element(),
        );
    }
    banners
}

/// Sets a row's field to a value (see [`Binding`]).
type SetValue<T> = Rc<dyn Fn(T, &mut App)>;

/// How a row's control changes its field (see [`bound_row`]): [`Binding::apply`] for each
/// step of a gesture, [`Binding::save`] once when it ends.
pub struct Binding<T> {
    /// Tells this row's keyed states apart from other rows' (see [`Binding::state_key`]).
    key: SharedString,
    apply: SetValue<T>,
    save: SetValue<T>,
}

impl<T> Clone for Binding<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            apply: self.apply.clone(),
            save: self.save.clone(),
        }
    }
}

impl<T> Binding<T> {
    /// Apply `value` live for this run without writing the file (`settings_file::preview`):
    /// for each step of a gesture (a slider moved). [`Binding::save`] must end the gesture.
    pub fn apply(&self, value: T, cx: &mut App) {
        (self.apply)(value, cx);
    }

    /// Apply `value` and save it to the file (`settings_file::update`): once per gesture (a
    /// switch or dropdown change, a slider released, a text field left or Enter pressed).
    pub fn save(&self, value: T, cx: &mut App) {
        (self.save)(value, cx);
    }

    /// A key for `Window::use_keyed_state`, unique to this row (built from its title), for a
    /// control's entities (an `InputState`, a `SliderState`); `name` tells them apart. The
    /// state lives while the page shows, so it is built again from the value after that.
    pub fn state_key(&self, name: &str) -> SharedString {
        format!("{}-{name}", self.key).into()
    }
}

/// Draws a row's control: given the current value and how to change it.
pub type Control<T> = Rc<dyn Fn(T, &Binding<T>, &mut Window, &mut App) -> AnyElement>;

type ReadSetting<T> = Rc<dyn Fn(&Settings) -> T>;
type WriteSetting<T> = Rc<dyn Fn(&mut Settings, T)>;

/// The read/write pair for one setting, shared by every row control.
pub struct SettingsField<T> {
    get: ReadSetting<T>,
    set: WriteSetting<T>,
}

impl<T> SettingsField<T> {
    pub fn new(
        get: impl Fn(&Settings) -> T + 'static,
        set: impl Fn(&mut Settings, T) + 'static,
    ) -> Self {
        Self {
            get: Rc::new(get),
            set: Rc::new(set),
        }
    }
}

/// A row bound to a field of `Settings`: `get` reads it, `set` writes it. `control` draws the
/// value and changes it through its [`Binding`] (live with `apply`, to the file with `save`).
/// "Reset to default" shows under the control, and the page's reset includes the row, while
/// the value differs from `get(&Settings::default())`. Titles must differ between rows: they
/// key the rows' control states.
pub fn bound_row<T: Clone + PartialEq + 'static>(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    field: SettingsField<T>,
    control: Control<T>,
) -> SettingItem {
    let title = title.into();
    let SettingsField { get, set } = field;
    let binding = Binding {
        key: format!("settings-row-{title}").into(),
        apply: Rc::new({
            let set = set.clone();
            move |value, cx| settings_file::preview(cx, |settings| set(settings, value))
        }),
        save: Rc::new({
            let set = set.clone();
            move |value, cx| settings_file::update(cx, |settings| set(settings, value))
        }),
    };
    let reset: Rc<dyn Fn(&mut App)> = Rc::new({
        let get = get.clone();
        move |cx| {
            let default = get(&Settings::default());
            settings_file::update(cx, |settings| set(settings, default));
        }
    });
    let is_dirty: Rc<dyn Fn(&App) -> bool> = Rc::new({
        let get = get.clone();
        move |cx| get(settings_file::settings(cx)) != get(&Settings::default())
    });
    let field = SettingField::render({
        let (reset, is_dirty) = (reset.clone(), is_dirty.clone());
        move |_, window, cx| {
            let value = get(settings_file::settings(cx));
            let reset = reset.clone();
            div()
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .child(control(value, &binding, window, cx))
                .when(is_dirty(cx), |column| {
                    column.child(
                        Button::new("reset")
                            .label("Reset to default")
                            .ghost()
                            .xsmall()
                            .on_click(move |_, _, cx| reset(cx)),
                    )
                })
        }
    });
    SettingItem::new(
        title,
        field.on_reset(move |cx| is_dirty(cx), move |_, cx| reset(cx)),
    )
    .description(description.into())
}

/// A switch row for a `bool` field of `Settings` (see [`bound_row`]); each click saves.
pub fn switch_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    field: SettingsField<bool>,
) -> SettingItem {
    bound_row(
        title,
        description,
        field,
        Rc::new(|checked, binding, _, _| {
            let binding = binding.clone();
            Switch::new("switch")
                .checked(checked)
                .on_click(move |checked: &bool, _, cx| binding.save(*checked, cx))
                .into_any_element()
        }),
    )
}

/// A text field row for a `String` field of `Settings` (see [`bound_row`]): saved when the
/// field loses the keyboard or on Enter, not per key.
pub fn text_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    field: SettingsField<String>,
) -> SettingItem {
    let placeholder = placeholder.into();
    bound_row(
        title,
        description,
        field,
        Rc::new(move |value, binding, window, cx| {
            text_field(value, binding, placeholder.clone(), window, cx)
        }),
    )
}

/// A text field's input, kept across renders (see [`Binding::state_key`]).
struct TextField {
    input: Entity<InputState>,
    _events: Subscription,
}

fn text_field(
    value: String,
    binding: &Binding<String>,
    placeholder: SharedString,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = window.use_keyed_state(binding.state_key("input"), cx, |window, cx| {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(value.clone())
        });
        let binding = binding.clone();
        let events = cx.subscribe(&input, move |_, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                binding.save(input.read(cx).value().to_string(), cx);
            }
        });
        TextField {
            input,
            _events: events,
        }
    });
    let input = state.read(cx).input.clone();
    // A value changed elsewhere (reset, the file reloaded) shows, unless the user is typing.
    let typing = input.read(cx).focus_handle(cx).is_focused(window);
    if !typing && input.read(cx).value().as_ref() != value.as_str() {
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }
    Input::new(&input).small().w_64().into_any_element()
}

/// A slider's bounds and step (see [`slider_row`]).
#[derive(Clone, Copy)]
pub struct SliderRange {
    pub min: f32,
    pub max: f32,
    pub step: f32,
}

/// A slider row for an `f32` field of `Settings` (see [`bound_row`]), with the value written
/// next to it: applied live while dragged, saved once when released. A value outside `range`
/// (set in the file) shows with the thumb at the end.
pub fn slider_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    range: SliderRange,
    field: SettingsField<f32>,
) -> SettingItem {
    bound_row(
        title,
        description,
        field,
        Rc::new(move |value, binding, window, cx| slider_field(value, range, binding, window, cx)),
    )
}

/// A slider's state, kept across renders (see [`Binding::state_key`]).
struct SliderField {
    slider: Entity<SliderState>,
    _events: Subscription,
}

fn slider_field(
    value: f32,
    range: SliderRange,
    binding: &Binding<f32>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let shown = value.clamp(range.min, range.max);
    let slider = use_slider(range, shown, binding, window, cx);
    sync_slider(&slider, shown, window, cx);
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(Slider::new(&slider).w_48())
        .child(div().w_8().text_sm().text_right().child(format!("{value}")))
        .into_any_element()
}

fn use_slider(
    range: SliderRange,
    shown: f32,
    binding: &Binding<f32>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<SliderState> {
    let state = window.use_keyed_state(binding.state_key("slider"), cx, |_, cx| {
        let slider = cx.new(|_| {
            SliderState::new()
                .min(range.min)
                .max(range.max)
                .step(range.step)
                .default_value(shown)
        });
        let binding = binding.clone();
        let events = cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(value) => binding.apply(value.start(), cx),
            SliderEvent::Release(value) => binding.save(value.start(), cx),
        });
        SliderField {
            slider,
            _events: events,
        }
    });
    state.read(cx).slider.clone()
}

fn sync_slider(slider: &Entity<SliderState>, shown: f32, window: &mut Window, cx: &mut App) {
    // A value changed elsewhere (reset, the file reloaded) moves the thumb.
    if slider.read(cx).value().start() != shown {
        slider.update(cx, |slider, cx| slider.set_value(shown, window, cx));
    }
}

/// One choice of a [`dropdown_row`]: the value saved, the label shown, and an optional icon.
#[derive(Clone)]
pub struct DropdownOption {
    pub value: SharedString,
    pub label: SharedString,
    pub icon: Option<Icon>,
}

impl DropdownOption {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
}

pub struct DropdownChoices {
    options: Rc<[DropdownOption]>,
    unlisted: Rc<dyn Fn(&str) -> SharedString>,
}

impl DropdownChoices {
    pub fn new(
        options: impl Into<Rc<[DropdownOption]>>,
        unlisted: impl Fn(&str) -> SharedString + 'static,
    ) -> Self {
        Self {
            options: options.into(),
            unlisted: Rc::new(unlisted),
        }
    }
}

/// A dropdown row for a `String` field of `Settings` (see [`bound_row`]): a button showing the
/// chosen option's label that opens a menu of `options`; picking one saves it at once. A
/// value that isn't one of them (set in the file) shows as `unlisted(value)`.
pub fn dropdown_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    choices: DropdownChoices,
    field: SettingsField<String>,
) -> SettingItem {
    let DropdownChoices { options, unlisted } = choices;
    bound_row(
        title,
        description,
        field,
        Rc::new(move |value: String, binding, _, _| {
            let chosen = options.iter().find(|option| option.value == value.as_str());
            let label = chosen.map_or_else(|| unlisted(&value), |option| option.label.clone());
            let (options, binding) = (options.clone(), binding.clone());
            // The id keys the menu's open state: unique per row (see `Binding::state_key`).
            Button::new(binding.state_key("dropdown"))
                .label(label)
                .when_some(chosen.and_then(|option| option.icon.clone()), Button::icon)
                .dropdown_caret(true)
                .outline()
                .small()
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                    let menu = options.iter().fold(menu, |menu, option| {
                        let binding = binding.clone();
                        let picked = option.value.to_string();
                        menu.item(
                            PopupMenuItem::new(option.label.clone())
                                .when_some(option.icon.clone(), PopupMenuItem::icon)
                                .checked(option.value == value.as_str())
                                .on_click(move |_, _, cx| binding.save(picked.clone(), cx)),
                        )
                    });
                    // The check on the right: on the left an option's icon would hide it.
                    menu.check_side(Side::Right).scrollable(true)
                })
                .into_any_element()
        }),
    )
}
