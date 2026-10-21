//! OHOS backend.
//!
//! OHOS fork, openspec changes `ohos-winit-p2` + `ohos-winit-p3`: p2
//! shipped the compile-contract surface (every type the
//! platform-independent layer (`src/*.rs`) forwards to exists with
//! android-parity shape), p3 filled the runtime behind it:
//!
//! - The [`OpenHarmonyApp`] storage injected through
//!   [`EventLoopBuilderExtOhos::with_openharmony_app`] and the event pump injected through
//!   [`EventLoopBuilderExtOhos::with_ohos_pump`] are the two required injections (guided panics
//!   when either is missing).
//! - The relay / distillation layer lives in `pump.rs`; the loop types (`EventLoop` /
//!   `ActiveEventLoop` / `EventLoopProxy` / attributes) live in `event_loop.rs` (re-exported below,
//!   namespace unchanged); the window / monitor queries are real implementations over the app
//!   handle (see `window.rs`).
//! - Window setters are no-ops that log their parameters via `tracing::debug!` (see `window.rs`);
//!   input translation is p4 scope.
//! - The only `unimplemented!` stubs left are the permanent rwh_04/05 paths (those
//!   raw-window-handle versions define no OHOS handle types); the reconciliation checklist lives in
//!   `OHOS_FORK_NOTES.md`.
//!
//! This file keeps the module namespace: id types, `KeyEventExtra`, and the
//! window-attributes stub.
//!
//! [`EventLoopBuilderExtOhos::with_openharmony_app`]: crate::platform::ohos::EventLoopBuilderExtOhos::with_openharmony_app
//! [`EventLoopBuilderExtOhos::with_ohos_pump`]: crate::platform::ohos::EventLoopBuilderExtOhos::with_ohos_pump

mod event_loop;
mod input;
mod keycodes;
mod pump;
mod window;

pub(crate) use self::event_loop::{
    ActiveEventLoop, OwnedDisplayHandle, PlatformSpecificEventLoopAttributes,
};
pub use self::event_loop::{EventLoop, EventLoopProxy};
pub use self::pump::{OhosInputForwarder, OhosPump};
pub(crate) use self::window::Window;
pub use self::window::{MonitorHandle, OsError, VideoModeHandle};
pub(crate) use crate::cursor::{
    NoCustomCursor as PlatformCustomCursor, NoCustomCursor as PlatformCustomCursorSource,
};
pub(crate) use crate::icon::NoIcon as PlatformIcon;

/// Additional platform-specific window attributes (none on OHOS).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlatformSpecificWindowAttributes;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeviceId;

impl DeviceId {
    pub const fn dummy() -> Self {
        DeviceId
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct WindowId;

impl WindowId {
    pub const fn dummy() -> Self {
        WindowId
    }
}

impl From<WindowId> for u64 {
    fn from(_: WindowId) -> Self {
        0
    }
}

impl From<u64> for WindowId {
    fn from(_: u64) -> Self {
        Self
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct KeyEventExtra {}
