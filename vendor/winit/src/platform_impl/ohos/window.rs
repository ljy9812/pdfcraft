//! OHOS window.
//!
//! See `mod.rs` for the phase discipline. The setter surface here is a
//! no-op inventory: every setter consumes its parameters through a
//! `tracing::debug!` line so the no-op is observable in hilog-derived logs
//! (the fork denies warnings, so parameters must be consumed anyway).
//! The runtime queries (sizes, scale factor, monitors, rwh 0.6 handles) are
//! real implementations over the app handle since p3; the only remaining
//! `unimplemented!` stubs are the permanent rwh_04/05 paths (those
//! raw-window-handle versions define no OHOS handle types) — the
//! reconciliation checklist lives in `OHOS_FORK_NOTES.md`.

use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};

use openharmony_ability::OpenHarmonyApp;
use tracing::debug;

use super::pump::RedrawRequester;
use super::{ActiveEventLoop, WindowId};
use crate::cursor::Cursor;
use crate::dpi::{PhysicalPosition, PhysicalSize, Position, Size};
use crate::error;
use crate::platform_impl::Fullscreen;
use crate::window::{
    self, CursorGrabMode, ImePurpose, ResizeDirection, Theme, WindowButtons, WindowLevel,
};

/// The OHOS window: the app's single fullscreen XComponent surface, carried by
/// the app handle's query methods (design D6).
pub(crate) struct Window {
    /// The query-side app handle cloned from the active event loop (design
    /// D6): every runtime query goes through its read-only methods.
    app: OpenHarmonyApp,
    /// The redraw requester (design D5): `request_redraw` wakes the pump with
    /// flag dedup from any thread.
    redraw_requester: RedrawRequester,
}

/// Converts an ability pixel extent pair to a [`PhysicalSize`], clamping
/// zero/negative extents to 1.
///
/// The XComponent reports zero-sized rects before the first layout pass, so
/// every size that flows into winit's event stream or an EGL surface
/// creation goes through this guard (same clamp as the p1 spike's
/// `surface_size`, design D6).
pub(crate) fn ability_size_to_physical(width: i32, height: i32) -> PhysicalSize<u32> {
    let clamp = |value: i32| if value.is_positive() { value as u32 } else { 1 };
    PhysicalSize::new(clamp(width), clamp(height))
}

impl Window {
    pub(crate) fn new(
        el: &ActiveEventLoop,
        _window_attrs: window::WindowAttributes,
    ) -> Result<Self, error::OsError> {
        // Android parity: one fullscreen XComponent surface per ability — the
        // attributes carry nothing addressable on OHOS, so they are ignored
        // and the window is the app handle plus the loop's redraw requester.
        // The call site is inside `Resumed` by design D2's pump order;
        // constructing without a live surface still succeeds — queries
        // degrade instead of panicking (design D6).
        Ok(Self { app: el.app.clone(), redraw_requester: el.redraw_requester.clone() })
    }

    pub(crate) fn maybe_queue_on_main(&self, f: impl FnOnce(&Self) + Send + 'static) {
        f(self)
    }

    pub(crate) fn maybe_wait_on_main<R: Send>(&self, f: impl FnOnce(&Self) -> R + Send) -> R {
        f(self)
    }

    pub fn id(&self) -> WindowId {
        WindowId
    }

    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        // The single default display (design D6, android parity).
        Some(MonitorHandle::new(self.app.clone()))
    }

    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        let mut monitors = VecDeque::with_capacity(1);
        monitors.push_back(MonitorHandle::new(self.app.clone()));
        monitors
    }

    pub fn current_monitor(&self) -> Option<MonitorHandle> {
        Some(MonitorHandle::new(self.app.clone()))
    }

    pub fn scale_factor(&self) -> f64 {
        // The OHOS density (android's `densityDpi / 160` equivalent), design
        // D6.
        self.app.scale() as f64
    }

    pub fn request_redraw(&self) {
        // Design D5: the pending flag dedups, the channel-backed waker wakes
        // the pump — callable from any thread, including inside event
        // callbacks on the pump thread itself.
        self.redraw_requester.request_redraw();
    }

    pub fn pre_present_notify(&self) {}

    pub fn inner_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
        Err(error::NotSupportedError::new())
    }

    pub fn outer_position(&self) -> Result<PhysicalPosition<i32>, error::NotSupportedError> {
        Err(error::NotSupportedError::new())
    }

    pub fn set_outer_position(&self, _position: Position) {
        debug!("ohos: no-op set_outer_position({_position:?})");
    }

    pub fn inner_size(&self) -> PhysicalSize<u32> {
        // The XComponent fills the ability window, so the content rect is the
        // drawable extent; the clamp guards the pre-layout zero rect (D6).
        let rect = self.app.content_rect();
        ability_size_to_physical(rect.width, rect.height)
    }

    pub fn request_inner_size(&self, _size: Size) -> Option<PhysicalSize<u32>> {
        debug!("ohos: no-op request_inner_size({_size:?})");
        None
    }

    pub fn outer_size(&self) -> PhysicalSize<u32> {
        // No decorations on a fullscreen XComponent — outer = inner (android
        // parity, design D6).
        self.inner_size()
    }

    pub fn set_min_inner_size(&self, _size: Option<Size>) {
        debug!("ohos: no-op set_min_inner_size({_size:?})");
    }

    pub fn set_max_inner_size(&self, _size: Option<Size>) {
        debug!("ohos: no-op set_max_inner_size({_size:?})");
    }

    pub fn resize_increments(&self) -> Option<PhysicalSize<u32>> {
        None
    }

    pub fn set_resize_increments(&self, _increments: Option<Size>) {
        debug!("ohos: no-op set_resize_increments({_increments:?})");
    }

    pub fn set_title(&self, _title: &str) {
        debug!("ohos: no-op set_title({_title:?})");
    }

    pub fn set_transparent(&self, _transparent: bool) {
        debug!("ohos: no-op set_transparent({_transparent})");
    }

    pub fn set_blur(&self, _blur: bool) {
        debug!("ohos: no-op set_blur({_blur})");
    }

    pub fn set_visible(&self, _visibility: bool) {
        debug!("ohos: no-op set_visible({_visibility})");
    }

    pub fn is_visible(&self) -> Option<bool> {
        None
    }

    pub fn set_resizable(&self, _resizable: bool) {
        debug!("ohos: no-op set_resizable({_resizable})");
    }

    pub fn is_resizable(&self) -> bool {
        false
    }

    pub fn set_enabled_buttons(&self, _buttons: WindowButtons) {
        debug!("ohos: no-op set_enabled_buttons({_buttons:?})");
    }

    pub fn enabled_buttons(&self) -> WindowButtons {
        WindowButtons::all()
    }

    pub fn set_minimized(&self, _minimized: bool) {
        debug!("ohos: no-op set_minimized({_minimized})");
    }

    pub fn is_minimized(&self) -> Option<bool> {
        None
    }

    pub fn set_maximized(&self, _maximized: bool) {
        debug!("ohos: no-op set_maximized({_maximized})");
    }

    pub fn is_maximized(&self) -> bool {
        false
    }

    pub fn set_fullscreen(&self, _monitor: Option<Fullscreen>) {
        debug!("ohos: no-op set_fullscreen({_monitor:?})");
    }

    pub fn fullscreen(&self) -> Option<Fullscreen> {
        None
    }

    pub fn set_decorations(&self, _decorations: bool) {
        debug!("ohos: no-op set_decorations({_decorations})");
    }

    pub fn is_decorated(&self) -> bool {
        true
    }

    pub fn set_window_level(&self, _level: WindowLevel) {
        debug!("ohos: no-op set_window_level({_level:?})");
    }

    pub fn set_window_icon(&self, _window_icon: Option<crate::icon::Icon>) {
        debug!("ohos: no-op set_window_icon (icon present: {})", _window_icon.is_some());
    }

    pub fn set_ime_cursor_area(&self, _position: Position, _size: Size) {
        debug!("ohos: no-op set_ime_cursor_area({_position:?}, {_size:?})");
    }

    pub fn set_ime_allowed(&self, _allowed: bool) {
        debug!("ohos: no-op set_ime_allowed({_allowed})");
    }

    pub fn set_ime_purpose(&self, _purpose: ImePurpose) {
        debug!("ohos: no-op set_ime_purpose({_purpose:?})");
    }

    pub fn focus_window(&self) {}

    pub fn request_user_attention(&self, _request_type: Option<window::UserAttentionType>) {
        debug!("ohos: no-op request_user_attention({_request_type:?})");
    }

    pub fn set_cursor(&self, _cursor: Cursor) {
        debug!("ohos: no-op set_cursor({_cursor:?})");
    }

    pub fn set_cursor_position(&self, _position: Position) -> Result<(), error::ExternalError> {
        debug!("ohos: no-op set_cursor_position({_position:?})");
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    pub fn set_cursor_grab(&self, _mode: CursorGrabMode) -> Result<(), error::ExternalError> {
        debug!("ohos: no-op set_cursor_grab({_mode:?})");
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    pub fn set_cursor_visible(&self, _visible: bool) {
        debug!("ohos: no-op set_cursor_visible({_visible})");
    }

    pub fn drag_window(&self) -> Result<(), error::ExternalError> {
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    pub fn drag_resize_window(
        &self,
        _direction: ResizeDirection,
    ) -> Result<(), error::ExternalError> {
        debug!("ohos: no-op drag_resize_window({_direction:?})");
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[inline]
    pub fn show_window_menu(&self, _position: Position) {
        debug!("ohos: no-op show_window_menu({_position:?})");
    }

    pub fn set_cursor_hittest(&self, _hittest: bool) -> Result<(), error::ExternalError> {
        debug!("ohos: no-op set_cursor_hittest({_hittest})");
        Err(error::ExternalError::NotSupported(error::NotSupportedError::new()))
    }

    #[cfg(feature = "rwh_04")]
    pub fn raw_window_handle_rwh_04(&self) -> rwh_04::RawWindowHandle {
        // rwh_04 has no OHOS handle types; rwh_06 (OhosNdkWindowHandle) is the
        // supported path on OHOS.
        unimplemented!(
            "ohos backend: rwh_04 has no OHOS window handle type, enable the rwh_06 feature"
        )
    }

    #[cfg(feature = "rwh_05")]
    pub fn raw_window_handle_rwh_05(&self) -> rwh_05::RawWindowHandle {
        // rwh_05 has no OHOS handle types; rwh_06 (OhosNdkWindowHandle) is the
        // supported path on OHOS.
        unimplemented!(
            "ohos backend: rwh_05 has no OHOS window handle type, enable the rwh_06 feature"
        )
    }

    #[cfg(feature = "rwh_05")]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        // rwh_05 has no OHOS handle types; rwh_06 (OhosDisplayHandle) is the
        // supported path on OHOS.
        unimplemented!(
            "ohos backend: rwh_05 has no OHOS display handle type, enable the rwh_06 feature"
        )
    }

    #[cfg(feature = "rwh_06")]
    pub fn raw_window_handle_rwh_06(&self) -> Result<rwh_06::RawWindowHandle, rwh_06::HandleError> {
        // The binding reports the `OhosNdk` handle while the surface is
        // live; between `Suspended` and the next `Resumed` it reports `None`,
        // which maps to winit 0.30's `Unavailable` (design D6 — never a
        // panic; Option shape anchored at the p1 spike's surface-create
        // path).
        self.app
            .native_window()
            .and_then(|window| window.raw_window_handle())
            .ok_or(rwh_06::HandleError::Unavailable)
    }

    #[cfg(feature = "rwh_06")]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::OhosDisplayHandle::new().into())
    }

    pub fn set_theme(&self, _theme: Option<Theme>) {
        debug!("ohos: no-op set_theme({_theme:?})");
    }

    pub fn theme(&self) -> Option<Theme> {
        None
    }

    pub fn set_content_protected(&self, _protected: bool) {
        debug!("ohos: no-op set_content_protected({_protected})");
    }

    pub fn has_focus(&self) -> bool {
        // Assume focused until the p3 lifecycle tracks real focus state
        // (mirrors the android backend's static default).
        true
    }

    pub fn title(&self) -> String {
        String::new()
    }

    pub fn reset_dead_keys(&self) {}
}

#[derive(Default, Clone, Debug)]
pub struct OsError;

impl Display for OsError {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), fmt::Error> {
        write!(fmt, "OHOS OS Error")
    }
}

/// The OHOS monitor: the default display, carried by the app handle's
/// display queries (design D6) — a phone-form device has exactly one.
#[derive(Clone, Debug)]
pub struct MonitorHandle {
    /// The app handle backing the display queries (design D6).
    app: OpenHarmonyApp,
}

// Single-display semantics (design D6): two handles over the same — the
// only — display are equal, hash identically, and never order apart.
// `OpenHarmonyApp` carries no comparison derives, so the identities are
// handwritten (the constant that android gets from its derived handles).
impl PartialEq for MonitorHandle {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for MonitorHandle {}

impl std::hash::Hash for MonitorHandle {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // A constant hash keeps the single-display invariant: the hash of
        // any two handles agrees, as their equality does.
        state.write_u8(0);
    }
}

impl PartialOrd for MonitorHandle {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MonitorHandle {
    fn cmp(&self, _other: &Self) -> std::cmp::Ordering {
        std::cmp::Ordering::Equal
    }
}

impl MonitorHandle {
    pub(crate) fn new(app: OpenHarmonyApp) -> Self {
        Self { app }
    }

    pub fn name(&self) -> Option<String> {
        // A constant name for the one display (android's "Android Device"
        // parity).
        Some("OpenHarmony Display".to_owned())
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        // The default display's physical extents — the real screen size, as
        // opposed to the window's own content rect.
        let (width, height) = self.app.display_size();
        PhysicalSize::new(width, height)
    }

    pub fn position(&self) -> PhysicalPosition<i32> {
        // A single mobile display is at the origin by definition (android
        // parity).
        (0, 0).into()
    }

    pub fn scale_factor(&self) -> f64 {
        self.app.scale() as f64
    }

    pub fn refresh_rate_millihertz(&self) -> Option<u32> {
        // The DisplayManager reports Hz; winit wants millihertz. The unit
        // mapping is hilog-verified against the device during G4 (design
        // D6's apply-time adjudication).
        Some(self.app.refresh_rate() * 1000)
    }

    pub fn video_modes(&self) -> impl Iterator<Item = VideoModeHandle> {
        // One synthetic mode: the display's current configuration (design
        // D6; android's single-entry parity).
        let size = self.size().into();
        let refresh_rate_millihertz = self.refresh_rate_millihertz().unwrap_or(60_000);
        std::iter::once(VideoModeHandle {
            size,
            bit_depth: 32,
            refresh_rate_millihertz,
            monitor: self.clone(),
        })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VideoModeHandle {
    size: (u32, u32),
    bit_depth: u16,
    refresh_rate_millihertz: u32,
    monitor: MonitorHandle,
}

impl VideoModeHandle {
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size.into()
    }

    pub fn bit_depth(&self) -> u16 {
        self.bit_depth
    }

    pub fn refresh_rate_millihertz(&self) -> u32 {
        self.refresh_rate_millihertz
    }

    pub fn monitor(&self) -> MonitorHandle {
        self.monitor.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openharmony_ability::{Rect, Size};

    use super::super::{EventLoop, OhosPump, PlatformSpecificEventLoopAttributes};

    /// UT for `ability_size_to_physical` — ability pixel extents convert to
    /// `PhysicalSize<u32>` with the zero-clamp guard (design D6).
    /// # Brief
    /// 1. Positive extents convert 1:1 — an ability `Size` (event payloads) and a content `Rect`
    ///    (query results) both feed the i32 pair.
    /// 2. Zero/negative extents clamp to 1 — the XComponent reports zero-sized rects before the
    ///    first layout pass (the p1 spike `surface_size` discipline), so a zero never reaches
    ///    winit's event stream or an EGL surface creation.
    #[test]
    fn ability_size_converts_to_physical() {
        let size = Size { width: 1080, height: 2340 };
        assert_eq!(
            ability_size_to_physical(size.width, size.height),
            PhysicalSize::new(1080, 2340),
        );

        let rect = Rect { top: 0, left: 0, width: 1080, height: 2340 };
        assert_eq!(
            ability_size_to_physical(rect.width, rect.height),
            PhysicalSize::new(1080, 2340),
        );

        assert_eq!(ability_size_to_physical(0, 0), PhysicalSize::new(1, 1));
        assert_eq!(ability_size_to_physical(-5, 720), PhysicalSize::new(1, 720));
    }

    /// UT for `Window::raw_window_handle_rwh_06` — a window constructed over
    /// a headless app (no XComponent surface) reports `Unavailable` instead
    /// of panicking.
    /// # Brief
    /// 1. Build a dual-injected `EventLoop` (group-3 infrastructure) over a pure-memory app and
    ///    construct a `Window` from its active event loop — construction succeeds regardless of
    ///    surface state (design D6).
    /// 2. `raw_window_handle_rwh_06` returns `Err(HandleError::Unavailable)` — winit 0.30's
    ///    semantics for a missing surface (android parity), never a panic.
    #[test]
    fn handle_unavailable_without_surface() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<u8>::install(&app);
        let mut attributes =
            PlatformSpecificEventLoopAttributes { app: Some(app), pump: Some(Box::new(pump)) };
        let event_loop = EventLoop::<u8>::new(&mut attributes).unwrap();
        let window =
            Window::new(&event_loop.window_target().p, window::WindowAttributes::default())
                .unwrap();
        assert!(matches!(window.raw_window_handle_rwh_06(), Err(rwh_06::HandleError::Unavailable)));
    }
}
