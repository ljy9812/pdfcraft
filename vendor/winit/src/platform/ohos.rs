//! # OpenHarmony
//!
//! The OpenHarmony backend builds on (and exposes types from) the
//! [`openharmony-ability`] crate, which provides the Rust side of the ArkUI
//! ability entrypoint and the `XComponent` surface lifecycle.
//!
//! OpenHarmony applications need an ability entrypoint that receives an
//! [`OpenHarmonyApp`] handle from the system. Winit does not define that
//! entrypoint - it belongs to the application crate - but the event loop
//! requires two injections from it before being built: the app handle via
//! [`EventLoopBuilderExtOhos::with_openharmony_app`] (the query side) and the
//! event pump via [`EventLoopBuilderExtOhos::with_ohos_pump`] (the event
//! side).
//!
//! Input needs a third, shell-side piece: the NDK XComponent input callbacks
//! never fire in the node-mode layout `openharmony-ability` renders, so the
//! ArkTS shell forwards the touches its page receives through
//! `OhosPump::input_forwarder`'s handle (`OhosInputForwarder`, ohos-only) —
//! a napi export hands each touch to `forward_input` and the event enters
//! the pump's channel exactly as the relay would have delivered it.
//!
//! [`openharmony-ability`]: https://docs.rs/openharmony-ability/

use crate::event_loop::EventLoopBuilder;

#[cfg(ohos_platform)]
pub use openharmony_ability::OpenHarmonyApp;

// Docs fallback only, mirroring the `AndroidApp` docs stub in
// `src/platform/android.rs`: the `openharmony-ability` dependency is
// target-gated, so non-ohos doc builds (docsrs) see a placeholder instead.
#[cfg(not(ohos_platform))]
#[doc(hidden)]
pub struct OpenHarmonyApp;

#[cfg(ohos_platform)]
pub use crate::platform_impl::{OhosInputForwarder, OhosPump};

// Docs fallback for the pump export above, same pattern as `OpenHarmonyApp`:
// the real type lives in the ohos backend module, which non-ohos doc builds
// (docsrs) do not compile, so they see this placeholder instead.
#[cfg(not(ohos_platform))]
#[doc(hidden)]
pub struct OhosPump<T: 'static>(core::marker::PhantomData<T>);

/// Additional methods on [`EventLoopBuilder`] that are specific to OpenHarmony.
pub trait EventLoopBuilderExtOhos {
    /// Associates the [`OpenHarmonyApp`] received by the ability entrypoint
    /// with the event loop.
    ///
    /// This must be called on OpenHarmony since the [`OpenHarmonyApp`] is not
    /// global state. Constructing an event loop without it will panic with a
    /// hint pointing to this method.
    fn with_openharmony_app(&mut self, app: OpenHarmonyApp) -> &mut Self;

    /// Associates the event pump built by `OhosPump::install` with the event
    /// loop.
    ///
    /// The pump must be created by `OhosPump::install` called synchronously
    /// inside the `#[ability]` entrypoint, before spawning the worker thread
    /// that builds the event loop: `openharmony-ability` does not queue
    /// events, so the relay has to be registered before the first lifecycle
    /// event can arrive. The pump then crosses the `thread::spawn` boundary
    /// and is injected here.
    ///
    /// The pump's `T` must match this builder's user-event type: the pump is
    /// stored type-erased and downcast back at event-loop construction, and a
    /// mismatch panics with a hint. Spell `T` at both ends -
    /// `OhosPump::<T>::install(&app)` in the entrypoint, `with_ohos_pump` on
    /// this `EventLoopBuilder<T>` - and keep them identical.
    ///
    /// Constructing an event loop without a pump will panic with a hint
    /// pointing to `OhosPump::install`.
    fn with_ohos_pump<T: Send + 'static>(&mut self, pump: OhosPump<T>) -> &mut Self;
}

impl<T> EventLoopBuilderExtOhos for EventLoopBuilder<T> {
    fn with_openharmony_app(&mut self, app: OpenHarmonyApp) -> &mut Self {
        self.platform_specific.app = Some(app);
        self
    }

    fn with_ohos_pump<U: Send + 'static>(&mut self, pump: OhosPump<U>) -> &mut Self {
        self.platform_specific.pump = Some(Box::new(pump));
        self
    }
}
