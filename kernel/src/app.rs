// ---------------------------------------------------------------------------
// Astra OS — App interface
//
// Every windowed application implements the App trait.  The desktop
// compositor owns windows and delegates title, sizing, rendering, input, and
// refresh entirely to the app object.  App state lives inside each instance;
// there are no module-level globals for per-instance data, which makes
// multi-instance support straightforward to add later.
// ---------------------------------------------------------------------------

use crate::input::Key;

/// Action an app requests from the compositor after a keyboard event.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum AppAction {
    /// No change — nothing to redraw.
    Nothing,
    /// Redraw the full client area.
    RedrawAll,
    /// Redraw a specific app-relative area inside the client region.
    RedrawArea(usize, usize, usize, usize),
    /// Redraw only the bottom input region (for apps that declare
    /// `input_region_height`).  Falls back to `RedrawAll` if the app does not
    /// declare a partial region.
    RedrawInput,
    /// Close this window.
    Close,
    /// Open a file in the Editor.  Carries the path bytes and length.
    OpenFile([u8; 128], usize),
}

/// Common interface every windowed app must implement.
pub trait App {
    /// Title bar text.
    fn title(&self) -> &str;

    /// Preferred (window_w, window_h) when first opened.
    /// The compositor clamps this to the screen minus its margin constants.
    fn preferred_size(&self) -> (usize, usize);

    /// Stable identifier used for single-instance deduplication.
    /// Two windows with the same `app_id` are considered the same app.
    fn app_id(&self) -> &'static str;

    /// Render the full client area into the back-buffer.
    /// Takes `&self` so rendering can happen inside an immutable iteration.
    fn render(&self, cx: usize, cy: usize, cw: usize, ch: usize);

    /// Height of the partial-redraw strip at the bottom of the client area.
    /// Returns `None` by default (no partial region).
    /// When `Some(h)` is returned the compositor may clear and re-render only
    /// that strip instead of the full client area (e.g. terminal input line).
    fn input_region_height(&self) -> Option<usize> {
        None
    }

    /// Render only the bottom input strip.
    /// Only called when `input_region_height()` returns `Some`.
    /// Default falls back to a full render.
    fn render_input_region(&self, cx: usize, cy: usize, cw: usize, ch: usize) {
        self.render(cx, cy, cw, ch);
    }

    /// Handle a keyboard event and return what the compositor should do.
    fn handle_key(&mut self, key: Key) -> AppAction;

    /// Handle a left mouse click at `(rel_x, rel_y)` relative to the top-left
    /// of the *client* area (below the titlebar).  Default is no-op.
    fn handle_mouse_click(&mut self, _rel_x: i32, _rel_y: i32) -> AppAction {
        AppAction::Nothing
    }

    /// Handle a right mouse click at `(rel_x, rel_y)` relative to the client area.
    /// Default is no-op.
    fn handle_mouse_right_click(&mut self, _rel_x: i32, _rel_y: i32) -> AppAction {
        AppAction::Nothing
    }

    /// Handle a mouse move at `(rel_x, rel_y)` relative to the top-left of the
    /// *client* area.  Used for hover effects.  Default is no-op.
    fn handle_mouse_move(&mut self, _rel_x: i32, _rel_y: i32) -> AppAction {
        AppAction::Nothing
    }

    /// Handle a scroll-wheel event. `delta` is lines to scroll:
    /// positive = scroll content up (towards older output), negative = down.
    fn handle_mouse_scroll(&mut self, _delta: i32) -> AppAction {
        AppAction::Nothing
    }

    /// Whether the compositor should allow multiple simultaneous windows of
    /// this app type.  Defaults to `false` (raise-existing policy).
    /// Apps like Editor that open distinct files return `true`.
    fn allow_multiple_instances(&self) -> bool {
        false
    }

    /// Called by the compositor when the user clicks the window X button.
    /// Return `AppAction::Close` to allow the close (default).
    /// Return `AppAction::RedrawAll` to intercept and handle it yourself
    /// (e.g. show an unsaved-changes prompt).
    fn request_close(&mut self) -> AppAction {
        AppAction::Close
    }

    /// Periodic refresh interval in milliseconds, or `None` for on-demand only.
    /// When `Some(ms)` is returned the compositor calls `tick()` every `ms`
    /// milliseconds and acts on the returned `AppAction`.
    fn refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    /// Called by the compositor on each refresh tick (see `refresh_interval_ms`).
    /// Return `AppAction::Nothing` to skip the redraw this cycle (useful when
    /// the app can detect that its displayed data has not changed).
    /// Default returns `RedrawAll` to preserve existing behaviour for apps that
    /// do not override this method.
    fn tick(&mut self) -> AppAction {
        AppAction::RedrawAll
    }
}
