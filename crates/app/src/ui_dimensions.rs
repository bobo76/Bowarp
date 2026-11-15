/// Shared height for the file explorer, file viewer and sidebar headers.
pub const HEADER_HEIGHT: f32 = 40.;
/// Shared minimum width for popup menus.
pub const MENU_WIDTH: f32 = 200.;
/// The narrowest the window gets: the sidebar, the file viewer's minimum and half the
/// terminal's minimum beside it. Narrower, the panes would run past the window's right edge.
pub const MIN_WINDOW_WIDTH: f32 = crate::sidebar::SIDEBAR_WIDTH
    + workspace_model::pane_width::MIN_VIEWER_WIDTH
    + workspace_model::pane_width::MIN_TERMINAL_WIDTH / 2.;
/// The lowest the window gets: the title bar, a few sidebar rows and terminal lines.
pub const MIN_WINDOW_HEIGHT: f32 = 400.;
