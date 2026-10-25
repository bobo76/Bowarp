use gpui_kit::App;

/// The value a pane divider's drag has reached, kept in memory while the pointer moves and
/// saved once when the drag ends ([`Self::finish`]).
pub struct DividerDrag<T> {
    dragged: Option<T>,
    save: fn(T, &mut App),
}

impl<T: Copy + PartialEq> DividerDrag<T> {
    pub fn new(save: fn(T, &mut App)) -> Self {
        Self {
            dragged: None,
            save,
        }
    }

    /// The value being dragged, else the saved one from `saved`.
    pub fn current(&self, saved: impl FnOnce() -> T) -> T {
        self.dragged.unwrap_or_else(saved)
    }

    /// Follow the pointer to `value`; whether it differs from what `current` returns (`shown`),
    /// i.e. whether a redraw is due.
    pub fn follow(&mut self, value: T, shown: T) -> bool {
        if value == shown {
            return false;
        }
        self.dragged = Some(value);
        true
    }

    /// On any mouse release: save what a drag left, once.
    pub fn finish(&mut self, cx: &mut App) {
        if let Some(value) = self.dragged.take() {
            (self.save)(value, cx);
        }
    }

    /// Double-click: drop any drag and save `default`.
    pub fn reset(&mut self, default: T, cx: &mut App) {
        self.dragged = None;
        (self.save)(default, cx);
    }
}
