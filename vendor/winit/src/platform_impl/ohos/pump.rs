//! OHOS event pump: ability-callback relay, message distillation, and the
//! public installer.
//!
//! Populated across the p3 tasks (openspec change `ohos-winit-p3`, design
//! D1-D5): `PumpMessage` distillation of the borrowed
//! `openharmony_ability::Event` variants, the relay closure registered on the
//! app (callback thread discipline: distil + send only), the bounded
//! `SurfaceDestroy` ack handshake, `OhosPump::install`, and the
//! `RedrawRequester` wake/dedup machinery.

use crate::dpi::PhysicalSize;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openharmony_ability::{Event, InputEvent, OpenHarmonyApp};
use tracing::{debug, error, trace};

use super::window::ability_size_to_physical;

/// A message on the pump's main channel (design D1/D5).
///
/// The channel doubles as the waker: a send from any thread unblocks the
/// pump's `recv_timeout`, so `Wake` and `UserEvent` need no separate
/// notification machinery.
pub(crate) enum PumpMessage<T> {
    /// A distilled ability event from the relay.
    Event(Distilled),
    /// A user event injected from any thread via `EventLoopProxy::send_event`.
    UserEvent(T),
    /// A redraw wake from `RedrawRequester` (deduped by its flag before the
    /// send, design D5).
    Wake,
}

/// The ack the pump sends back to the relay once `SurfaceDestroyed` has been
/// fully processed (design D4) — the EGL surface is dropped by then, so the
/// NDK is free to invalidate the native window.
pub(crate) enum PumpAck {
    SurfaceDropped,
}

/// Outcome of the relay's bounded wait for the pump's surface-dropped ack
/// (design D4). Every branch returns — the NDK invalidates the native window
/// once the relay callback returns, so the outcome only decides what gets
/// logged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AckWait {
    /// The pump finished processing `Suspended` — the EGL surface is dropped,
    /// safe to hand the window back to the NDK.
    Dropped,
    /// The pump did not ack within the bound — logged as an error (deadlock/
    /// hang signature for the G4 front/back pressure test) and returned.
    TimedOut,
    /// The pump side is gone — nothing left to protect, fast path.
    Disconnected,
}

/// Upper bound of the relay's surface-dropped ack wait (design D4). The
/// pump's `Suspended` processing is a tight synchronous path (no rendering,
/// no waiting), so a healthy pump acks in milliseconds — the bound only trips
/// on deadlock/hang, which the G4 front/back pressure test must surface as a
/// loud error, never as a wedged NDK callback.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(1);

/// Waits a bounded time for the pump's surface-dropped ack (design D4).
///
/// Runs inside the NDK `on_surface_destroyed` callback: this is the ONE
/// deliberate blocking exception to the relay's distil-and-send discipline —
/// returning early here would let the NDK invalidate the native window under
/// a live EGL surface (use-after-free).
pub(crate) fn wait_surface_ack(rx: &mpsc::Receiver<PumpAck>, timeout: Duration) -> AckWait {
    match rx.recv_timeout(timeout) {
        Ok(PumpAck::SurfaceDropped) => AckWait::Dropped,
        Err(RecvTimeoutError::Timeout) => {
            error!(
                ?timeout,
                "ohos pump: surface-dropped ack timed out — the pump may be hung; the NDK will \
                 invalidate the native window anyway"
            );
            AckWait::TimedOut
        },
        Err(RecvTimeoutError::Disconnected) => AckWait::Disconnected,
    }
}

/// Relays one ability event to the pump channel (design D1/D4).
///
/// Registered by [`OhosPump::install`] as the app's `run_loop` handler. The
/// body obeys the callback-thread discipline: distil + send only — zero
/// native calls, zero panicking operations (the relay runs inside the NDK
/// callback boundary, where an unwinding panic aborts the process). The one
/// deliberate blocking exception is the `SurfaceDestroy` ack handshake: send
/// the message, then hold the callback until the pump has dropped the EGL
/// surface (or the bound expires — see [`ACK_TIMEOUT`]). The installer side
/// is [`OhosPump::install`].
pub(crate) fn relay<T>(
    event: &Event<'_>,
    tx: &mpsc::Sender<PumpMessage<T>>,
    ack_rx: &mpsc::Receiver<PumpAck>,
) {
    // p4 G8 diagnosis probe (DECISIONS.md §G5): an input line here proves the
    // NDK XComponent input callback fired AND passed the ability crate's
    // surface-active check — the event entered the pump pipeline via the
    // relay. A live touch on screen with NO such line localizes the block to
    // the platform's dispatch into the callback itself (the on-device finding
    // that motivated the ArkTS-side forward). Note the forward path
    // (`OhosInputForwarder`, added after this probe proved its case) delivers
    // straight onto the channel and never passes through here. Trace level,
    // riding the demo's D13 forwarding subscriber; volume is one line per
    // input event (bursty only under continuous drag — exactly when the log
    // is read).
    if matches!(event, Event::Input { .. }) {
        trace!("ohos pump: input reached relay");
    }
    match distill(event) {
        Some(Distilled::SurfaceDestroyed) => {
            // D4 sequence: send first, then wait — the pump cannot ack a
            // message it never received. If the send failed the pump is
            // already gone, so there is no surface left to protect and the
            // wait would only burn the bound.
            if tx.send(PumpMessage::Event(Distilled::SurfaceDestroyed)).is_ok() {
                let outcome = wait_surface_ack(ack_rx, ACK_TIMEOUT);
                debug!(?outcome, "ohos pump: surface destroy handshake complete");
            }
        },
        Some(distilled) => {
            if tx.send(PumpMessage::Event(distilled)).is_err() {
                debug!("ohos pump: receiver gone — dropping distilled event at relay");
            }
        },
        None => {
            // The vsync tick stays fully silent: `WindowRedraw` fires once per
            // frame (~120/s on the target device) from the XComponent frame
            // callback, so even a trace line per tick would flood the log the
            // moment any subscriber exists (p4 D13 installs one) — the same
            // energy argument as D3's "never crosses the channel". The other
            // unmapped variants are once-per-lifecycle and safe to trace.
            if !matches!(event, Event::WindowRedraw { .. }) {
                trace!(
                    variant = event.as_str(),
                    "ohos pump: dropping ability event at relay (no consumer)"
                );
            }
        },
    }
}

/// The consumer half of the OHOS event pump (design D1).
///
/// Built in the ability entrypoint by [`OhosPump::install`], then handed
/// across one `thread::spawn` boundary to the worker thread that constructs
/// the event loop (`OhosPump<T>` is `Send` when `T` is `Send` — the channels
/// and the ack sender carry no other state). The pump never holds the
/// `OpenHarmonyApp` itself: the app handle is the query side
/// (`with_openharmony_app`), the pump is the event side — two injections, two
/// responsibilities.
pub struct OhosPump<T: 'static> {
    /// Sender kept for the wake/user-event wiring: `EventLoopProxy` sends and
    /// the redraw requester clone from this end (design D5 — the channel
    /// doubles as the waker).
    pub(crate) tx: mpsc::Sender<PumpMessage<T>>,
    /// The pump's consuming end — the worker thread's `recv_timeout` target in
    /// the pump loop (design D10).
    pub(crate) rx: mpsc::Receiver<PumpMessage<T>>,
    /// The ack sender the pump uses to release the relay's bounded
    /// `SurfaceDestroy` wait (design D4).
    pub(crate) ack_tx: mpsc::Sender<PumpAck>,
}

impl<T: 'static> OhosPump<T> {
    /// Installs the relay as the app's `run_loop` handler and returns the
    /// pump (design D1).
    ///
    /// Call this synchronously inside the `#[ability]` entrypoint, before
    /// spawning the worker thread: `openharmony-ability` does not queue
    /// events, so the relay must be registered before the first lifecycle
    /// event can arrive (a missing handler silently drops it — the p1 R2
    /// spike proved this topology race-free on device).
    ///
    /// **Process-single-shot**: the ability `run_loop` latch is
    /// first-install-wins — a second `install` has its handler rejected with
    /// an error log while events keep flowing to the first relay, so the
    /// second pump's channels would never see a message. `install` is a
    /// once-per-process call; treat a second call as a programming error.
    ///
    /// `T` is the event loop's user-event type and must be spelled at the
    /// call site (`OhosPump::<UserEvent>::install(&app)`); it must match the
    /// builder the pump is later injected into, or event-loop construction
    /// panics on the downcast with a hint.
    pub fn install(app: &OpenHarmonyApp) -> Self {
        let (tx, rx) = mpsc::channel::<PumpMessage<T>>();
        let (ack_tx, ack_rx) = mpsc::channel::<PumpAck>();
        let relay_tx = tx.clone();
        app.run_loop(move |event| relay(&event, &relay_tx, &ack_rx));
        Self { tx, rx, ack_tx }
    }
}

/// A clonable handle for injecting input events into a pump's channel from
/// any thread (p4 G8 — the R2 remediation path).
///
/// The NDK XComponent input callbacks are structurally unreachable in the
/// node-mode layout `openharmony-ability` renders: surface callbacks fire,
/// `DispatchTouchEvent`/`DispatchMouseEvent` never do (on-device evidence
/// chain in `ohos-spike/DECISIONS.md` §G5 — the relay's input probe stayed
/// silent while ArkUI's own `onTouch` on the same page fired). The ArkTS
/// shell therefore owns the forward: its `onTouch` hands each touch to a
/// napi export, which calls this handle — the event enters the exact same
/// channel slot the relay would have used, so the loop-side translation
/// path is unchanged. The maintainer's own key-event forward
/// (`MainPage.onKeyPreIme` → `notify_key_event` →
/// `OpenHarmonyApp::dispatch_key_event`) is the same pattern at the ability
/// layer.
///
/// Non-generic on purpose: the napi export stores it in a process-global
/// slot and must not spell the loop's user-event type. The handle only ever
/// sends the `Event(Distilled::Input(..))` variant, so no user-event value
/// crosses a thread boundary through it, and the sender sits behind a
/// `Mutex` — concurrent `forward_input` calls are sound with zero
/// `unsafe impl`s (contrast [`ChannelWaker`]'s bare-sender pair, BM002).
#[derive(Clone)]
pub struct OhosInputForwarder {
    /// The type-erased send closure; `Arc` keeps handle clones cheap and the
    /// pump's `T` hidden behind it.
    forward: Arc<dyn Fn(InputEvent) + Send + Sync>,
}

impl OhosInputForwarder {
    /// Forwards one input event onto the pump's channel.
    ///
    /// Callable from any thread, including the napi boundary the ArkTS shell
    /// calls from: non-blocking (one mpsc send) and non-panicking (a dead
    /// receiver is a debug log; a poisoned lock is recovered — the closure
    /// body cannot panic, matching the relay's callback discipline).
    pub fn forward_input(&self, input: InputEvent) {
        (self.forward)(input);
    }
}

impl<T: Send + 'static> OhosPump<T> {
    /// Takes the [`OhosInputForwarder`] for this pump (p4 G8).
    ///
    /// Call before the pump crosses into the event loop
    /// (`EventLoopBuilderExtOhos::with_ohos_pump` consumes it): the forwarder
    /// keeps its own channel sender alive, so events forwarded after loop
    /// teardown are dropped with a debug log instead of panicking. The
    /// `T: Send` bound mirrors the builder-side pump bound — the closure
    /// captures the channel sender, whose auto traits need it.
    pub fn input_forwarder(&self) -> OhosInputForwarder {
        let tx = Mutex::new(self.tx.clone());
        OhosInputForwarder {
            forward: Arc::new(move |input| {
                // A poisoned lock cannot arise through this closure (the
                // critical section cannot panic), but recover it anyway: this
                // runs on the napi boundary, where a panic is process-fatal.
                let tx = tx.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if tx.send(PumpMessage::Event(Distilled::Input(input))).is_err() {
                    debug!("ohos pump: receiver gone — dropping forwarded input event");
                }
            }),
        }
    }
}

/// The wake hook behind [`RedrawRequester`] (design D5).
///
/// A trait object rather than a generic sender: the requester is held by the
/// non-generic `ActiveEventLoop`/`Window` (the root `ActiveEventLoop`'s
/// platform field is monomorphic), so the channel sender's `T` must be
/// erased here — android's `Box<dyn Wake>` is the same shape.
pub(crate) trait PumpWaker: Send + Sync {
    /// Unblocks the pump. Implementations must be non-blocking and
    /// non-panicking: callable from any thread, including from inside event
    /// callbacks on the pump thread itself.
    fn wake(&self);
}

/// The channel-backed [`PumpWaker`]: a wake is a `Wake` send on the pump's
/// own channel (design D5 — the channel doubles as the waker, so there is no
/// separate notification machinery to keep alive).
pub(crate) struct ChannelWaker<T> {
    tx: mpsc::Sender<PumpMessage<T>>,
}

// The `Send`/`Sync` auto traits on `Sender<PumpMessage<T>>` require `T: Send`
// (the value is expected to cross to the receiver's thread), but the root
// `EventLoop<T: 'static>` carries no `Send` bound — the same wall the 3.1
// type-erasure hit. This handle is still sound to share across threads:
//
// SAFETY: `ChannelWaker` only ever sends the `Wake` variant, which carries no
// `T`, so no `T` value crosses a thread boundary through this handle; the
// sender is private and never escapes, so no `T`-carrying message can be sent
// from here either. `T`-carrying messages (`UserEvent`) are only sent through
// the proxy, whose own auto traits keep it thread-bound for non-`Send` `T`.
unsafe impl<T> Send for ChannelWaker<T> {}
unsafe impl<T> Sync for ChannelWaker<T> {}

impl<T> ChannelWaker<T> {
    pub(crate) fn new(tx: mpsc::Sender<PumpMessage<T>>) -> Self {
        Self { tx }
    }
}

impl<T> PumpWaker for ChannelWaker<T> {
    fn wake(&self) {
        // A failed send means the pump side is gone — nothing left to wake.
        let _ = self.tx.send(PumpMessage::Wake);
    }
}

/// The shared "redraw pending" flag (design D5).
///
/// Held by both sides of the wake handshake: the requester sets it (dedup),
/// the pump loop clears it when dispatching `RedrawRequested`. `Relaxed`
/// ordering suffices — the pump channel provides the real synchronization
/// edge, the flag only collapses duplicate wakes (a stale `true` costs one
/// swallowed `Wake`, never a missed redraw).
#[derive(Clone, Default)]
pub(crate) struct SharedFlag(Arc<AtomicBool>);

impl SharedFlag {
    /// Sets the flag, returning whether it was already set.
    pub(crate) fn set(&self) -> bool {
        self.0.swap(true, Ordering::Relaxed)
    }

    /// Clears the flag.
    pub(crate) fn clear(&self) {
        self.0.store(false, Ordering::Relaxed)
    }

    /// Takes the flag: returns whether it was set, and clears it.
    ///
    /// The pump loop's pending-redraw gate (design D10): the take both reads
    /// the pending state and re-arms the requester in one atomic step.
    pub(crate) fn take(&self) -> bool {
        self.0.swap(false, Ordering::Relaxed)
    }

    /// Reads the flag without clearing it — the flag-starvation watchdog's
    /// non-consuming peek (p2 design D8, ohos-craft-p2); the pending-redraw
    /// gate stays the sole consumer (`take`).
    pub(crate) fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Requests redraws from any thread (design D5) — the OHOS counterpart of
/// android's `RedrawRequester`.
///
/// The flag dedups: while a request is pending, further `request_redraw`
/// calls send nothing. The pump loop dispatches one `RedrawRequested` and
/// clears the flag, which re-arms the requester.
#[derive(Clone)]
pub(crate) struct RedrawRequester {
    flag: SharedFlag,
    waker: Arc<dyn PumpWaker>,
}

impl RedrawRequester {
    pub(crate) fn new(waker: Arc<dyn PumpWaker>) -> Self {
        Self { flag: SharedFlag::default(), waker }
    }

    /// The shared pending flag — the pump loop dispatches `RedrawRequested`
    /// when it is set and clears it afterwards (design D5).
    pub(crate) fn pending_flag(&self) -> SharedFlag {
        self.flag.clone()
    }

    /// Requests a redraw: wakes the pump only on the pending flag's
    /// false→true transition (design D5 — the flag dedups, the channel
    /// wakes).
    ///
    /// The design sketch spelled the condition as `if self.flag.set()`;
    /// `set()` returns the *old* value, so the dedup predicate is the
    /// negation — wake when the flag was NOT already pending. The 4.1 test
    /// pins this behavior.
    pub(crate) fn request_redraw(&self) {
        if !self.flag.set() {
            self.waker.wake();
        }
    }
}

/// The ability `window_id` of the single fullscreen XComponent window this
/// backend serves (design D1: `0` = primary). Input carrying any other id is
/// dropped at distill with a trace — never a panic inside the callback
/// boundary.
pub(crate) const PRIMARY_WINDOW_ID: i64 = 0;

/// The owned outcome of distilling one borrowed ability [`Event`].
///
/// The ability `Event<'a>` cannot cross a channel (borrowed variants), so the
/// relay reduces it to this owned enum first (design D2 mapping table — the
/// table is the spec for [`distill`], exhaustive over all 24 ability variants
/// with no wildcard arm). The pump loop maps each variant onto the winit
/// event stream (design D10).
///
/// p4 adds the `Input` arm, whose payload (`InputEvent`) implements neither
/// `Copy` nor `PartialEq`, so the derives give way to a manual `PartialEq`
/// that compares `Input` by kind only (the loop-side translator owns the
/// payload's semantics; tests never need deep input equality).
#[derive(Clone, Debug)]
pub(crate) enum Distilled {
    /// `SurfaceCreate` — the XComponent render surface exists; the pump
    /// dispatches `Event::Resumed` (eframe creates its window inside Resumed,
    /// so `native_window()` is `Some` by then) followed by `Resized` when the
    /// content rect is non-zero.
    SurfaceCreated,
    /// `SurfaceDestroy` — the pump dispatches `Event::Suspended` (eframe drops
    /// the EGL surface + window synchronously) and then acks the relay.
    SurfaceDestroyed,
    /// `WindowResize` — size in physical pixels, clamped to non-zero.
    Resized(PhysicalSize<u32>),
    /// `GainedFocus` / `LostFocus`.
    Focused(bool),
    /// `LowMemory` — android `onLowMemory` parity.
    MemoryWarning,
    /// `Pause` / `Resume` — sets the pump's internal running flag (android
    /// `running` parity); not dispatched as a winit event.
    SetRunning(bool),
    /// `Destroy` / `PrepareToTerminate` — the pump requests the standard exit
    /// path. p3 never prevents termination (the `TerminateAnswer` slot is
    /// left untouched).
    ExitRequested,
    /// `Input` — an owned input event for the primary window (design D1:
    /// distillation is a pure borrow→owned wrap; translation to winit
    /// `WindowEvent`s happens loop-side in `InputTranslator`).
    Input(InputEvent),
}

impl PartialEq for Distilled {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Distilled::Input(_), Distilled::Input(_)) => true,
            (Distilled::Resized(a), Distilled::Resized(b)) => a == b,
            (Distilled::Focused(a), Distilled::Focused(b)) => a == b,
            (Distilled::SetRunning(a), Distilled::SetRunning(b)) => a == b,
            (Distilled::SurfaceCreated, Distilled::SurfaceCreated)
            | (Distilled::SurfaceDestroyed, Distilled::SurfaceDestroyed)
            | (Distilled::MemoryWarning, Distilled::MemoryWarning)
            | (Distilled::ExitRequested, Distilled::ExitRequested) => true,
            _ => false,
        }
    }
}

/// Distills a borrowed ability [`Event`] into an owned [`Distilled`].
///
/// Pure function: no native calls, no panicking operations (the relay runs it
/// inside the NDK callback boundary). Variants without a consumer
/// (`WindowRedraw` and the unmapped lifecycle set — design D2 table rows)
/// distill to `None` and are dropped at the relay.
///
/// Exhaustive by design: no wildcard arm, so a new ability variant fails this
/// match until the mapping table gains a row.
pub(crate) fn distill(event: &Event<'_>) -> Option<Distilled> {
    match event {
        // Mapped rows (design D2 table).
        Event::SurfaceCreate => Some(Distilled::SurfaceCreated),
        Event::SurfaceDestroy => Some(Distilled::SurfaceDestroyed),
        Event::WindowResize { size, .. } => {
            Some(Distilled::Resized(ability_size_to_physical(size.width, size.height)))
        },
        Event::GainedFocus { .. } => Some(Distilled::Focused(true)),
        Event::LostFocus { .. } => Some(Distilled::Focused(false)),
        Event::LowMemory => Some(Distilled::MemoryWarning),
        Event::Pause => Some(Distilled::SetRunning(false)),
        Event::Resume(_) => Some(Distilled::SetRunning(true)),
        Event::Destroy => Some(Distilled::ExitRequested),
        Event::PrepareToTerminate { .. } => Some(Distilled::ExitRequested),

        // Dropped at the relay (design D2 negative rows).
        // D3: the vsync frame tick never crosses the channel — frame pacing
        // belongs to eglSwapBuffers, an unconditional relay would wake the
        // pump ~60x/s at idle.
        Event::WindowRedraw { .. } => None,
        // p4 (design D1): input for the primary window crosses the channel as
        // an owned `Distilled::Input`; any other window id is dropped with a
        // trace (zero dispatch, zero panic). `KeyboardEvent(i32)` is the
        // ability-side raw keycode probe with no consumer — it stays dropped.
        Event::Input { window_id, input } => {
            if *window_id == PRIMARY_WINDOW_ID {
                Some(Distilled::Input(input.clone()))
            } else {
                trace!(?window_id, "ohos pump: input for an unknown window; dropped");
                None
            }
        },
        Event::KeyboardEvent(_) => None,
        // No p3 consumer; `UserEvent` is the ability waker TSFN channel —
        // listed explicitly so a wildcard arm cannot silently swallow it
        // (p3 wakeups go through the pump's own channel, design D5).
        Event::Create
        | Event::Start
        | Event::Stop
        | Event::WindowCreate
        | Event::WindowDestroy { .. }
        | Event::NewWant { .. }
        | Event::ConfigChanged(_)
        | Event::AvoidAreaChange(_)
        | Event::ContentRectChange(_)
        | Event::SaveState(_)
        | Event::UserEvent => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ohos_xcomponent_binding::{
        Action, EventSource, KeyCode as OhosKeyCode, KeyEventData, TouchEventData,
    };
    use openharmony_ability::{
        AvoidArea, AvoidAreaInfo, AvoidAreaType, AxisEventData, Configuration, ContentRect,
        ImeEvent, InputEvent, IntervalInfo, MouseEventData, OpenHarmonyApp, Rect, RectChangeReason,
        SaveLoader, SaveSaver, Size, TerminateAnswer,
    };

    /// UT for `distill` — SurfaceCreate distills to the Resumed trigger.
    /// # Brief
    /// 1. Construct `Event::SurfaceCreate` (fieldless unit variant).
    /// 2. `distill` yields `Some(Distilled::SurfaceCreated)`.
    #[test]
    fn surface_create_distills_to_resumed_trigger() {
        assert_eq!(distill(&Event::SurfaceCreate), Some(Distilled::SurfaceCreated));
    }

    /// UT for `distill` — SurfaceDestroy distills to the Suspended trigger.
    /// # Brief
    /// 1. Construct `Event::SurfaceDestroy` (fieldless unit variant).
    /// 2. `distill` yields `Some(Distilled::SurfaceDestroyed)`.
    #[test]
    fn surface_destroy_distills_to_suspended_trigger() {
        assert_eq!(distill(&Event::SurfaceDestroy), Some(Distilled::SurfaceDestroyed));
    }

    /// UT for `distill` — WindowResize distills to a physical size with the
    /// zero-clamp applied.
    /// # Brief
    /// 1. Distill `WindowResize` with a positive `Size` — yields the exact `PhysicalSize`.
    /// 2. Distill `WindowResize` with zero/negative extents — clamps to 1 (same guard as the p1
    ///    spike's `surface_size`: the XComponent reports zero-sized rects before the first layout
    ///    pass).
    #[test]
    fn window_resize_distills_to_resized() {
        let normal = distill(&Event::WindowResize {
            window_id: 0,
            size: Size { width: 1080, height: 2340 },
        });
        assert_eq!(normal, Some(Distilled::Resized(PhysicalSize::new(1080, 2340))));

        let clamped =
            distill(&Event::WindowResize { window_id: 0, size: Size { width: 0, height: -5 } });
        assert_eq!(clamped, Some(Distilled::Resized(PhysicalSize::new(1, 1))));
    }

    /// UT for `distill` — the two focus variants distill to true/false.
    /// # Brief
    /// 1. `GainedFocus` yields `Focused(true)`.
    /// 2. `LostFocus` yields `Focused(false)`.
    #[test]
    fn focus_variants_distill_true_and_false() {
        assert_eq!(distill(&Event::GainedFocus { window_id: 0 }), Some(Distilled::Focused(true)));
        assert_eq!(distill(&Event::LostFocus { window_id: 0 }), Some(Distilled::Focused(false)));
    }

    /// UT for `distill` — LowMemory distills to MemoryWarning.
    /// # Brief
    /// 1. `LowMemory` yields `Some(Distilled::MemoryWarning)`.
    #[test]
    fn low_memory_distills() {
        assert_eq!(distill(&Event::LowMemory), Some(Distilled::MemoryWarning));
    }

    /// UT for `distill` — Pause/Resume distill to the running-flag message.
    /// # Brief
    /// 1. `Pause` yields `SetRunning(false)`.
    /// 2. `Resume(SaveLoader)` yields `SetRunning(true)` — the loader is not consumed (state saving
    ///    goes through eframe's own Suspend path).
    #[test]
    fn pause_resume_distill_running_flag() {
        let app = OpenHarmonyApp::new();
        assert_eq!(distill(&Event::Pause), Some(Distilled::SetRunning(false)));
        assert_eq!(
            distill(&Event::Resume(SaveLoader::new(&app))),
            Some(Distilled::SetRunning(true))
        );
    }

    /// UT for `distill` — both terminate variants distill to ExitRequested.
    /// # Brief
    /// 1. `Destroy` yields `Some(Distilled::ExitRequested)`.
    /// 2. `PrepareToTerminate` yields the same, without touching the answer slot (p3 does not
    ///    prevent termination).
    #[test]
    fn terminate_variants_request_exit() {
        assert_eq!(distill(&Event::Destroy), Some(Distilled::ExitRequested));

        let answer = TerminateAnswer::new();
        assert_eq!(
            distill(&Event::PrepareToTerminate { answer: &answer }),
            Some(Distilled::ExitRequested)
        );
        assert!(!answer.is_prevented());
    }

    /// UT for `distill` — WindowRedraw is dropped at the relay (negative).
    /// # Brief
    /// 1. Distill `WindowRedraw` with a real `IntervalInfo`.
    /// 2. Yields `None` — the vsync frame tick never crosses the channel (design D3: frame pacing
    ///    belongs to `eglSwapBuffers`).
    #[test]
    fn window_redraw_is_dropped_at_relay() {
        let event = Event::WindowRedraw {
            window_id: 0,
            info: IntervalInfo { time_stamp: 0, target_time_stamp: 0 },
        };
        assert_eq!(distill(&event), None);
    }

    /// UT for `distill` — input events cross the channel as owned
    /// `Distilled::Input` (design D1: distillation is a pure
    /// borrow-to-owned wrap; translation happens loop-side).
    /// # Brief
    /// 1. Distill `Input` carrying each of touch / mouse / key / axis (and the IME variant — it
    ///    crosses too, as a pure wrap, and is dropped at the loop-side translation boundary).
    /// 2. Each yields `Some(Distilled::Input(..))` — the p3 drop arm is inverted.
    /// 3. `KeyboardEvent` (the IME keyboard-height alias) stays `None`.
    #[test]
    fn input_distilled_not_dropped() {
        let inputs = [
            InputEvent::TouchEvent(TouchEventData::default()),
            InputEvent::MouseEvent(MouseEventData::default()),
            InputEvent::KeyEvent(KeyEventData {
                code: OhosKeyCode::A,
                action: Action::Down,
                device_id: 0,
                source: EventSource::Keyboard,
                timestamp: 0,
            }),
            InputEvent::AxisEvent(AxisEventData::default()),
            InputEvent::ImeEvent(ImeEvent::BackspaceEvent(0)),
        ];
        for input in &inputs {
            let event = Event::Input { window_id: 0, input: input.clone() };
            assert!(
                matches!(distill(&event), Some(Distilled::Input(_))),
                "input must cross the channel as Distilled::Input"
            );
        }

        assert_eq!(distill(&Event::KeyboardEvent(0)), None);
    }

    /// UT for `distill` — input for a window the pump never registered is
    /// dropped without panicking (design D1: late input during
    /// window-teardown races is a legal ordering).
    /// # Brief
    /// 1. Distill `Input` with `window_id: 999` (no such window in the single-window model).
    /// 2. Yields `None` — nothing enters the channel, so nothing can dispatch.
    #[test]
    fn input_window_id_mismatch_dropped() {
        let event = Event::Input {
            window_id: 999,
            input: InputEvent::TouchEvent(TouchEventData::default()),
        };
        assert_eq!(distill(&event), None);
    }

    /// UT for `OhosPump::input_forwarder` — the ArkTS-side forward handle
    /// delivers a distilled input event onto the pump's channel (p4 G8 R2
    /// remediation: the NDK XComponent input callbacks are structurally
    /// unreachable, so the shell forwards the touches ArkUI dispatches to the
    /// page into the pump directly).
    /// # Brief
    /// 1. Install a pump on a fresh app (no relay traffic is involved).
    /// 2. Take the forwarder and forward one touch event.
    /// 3. The pump's channel yields `Event(Distilled::Input(..))` — the exact shape the relay
    ///    produces, so the loop-side translation path is unchanged.
    #[test]
    fn input_forwarder_delivers_to_pump_channel() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<()>::install(&app);
        let forwarder = pump.input_forwarder();
        forwarder.forward_input(InputEvent::TouchEvent(TouchEventData::default()));
        assert!(
            matches!(pump.rx.try_recv(), Ok(PumpMessage::Event(Distilled::Input(_)))),
            "forwarded input must land on the pump channel as Distilled::Input"
        );
    }

    /// UT for `OhosInputForwarder` — a dead receiver is survivable.
    /// # Brief
    /// 1. Drop the pump (its channel receiver included), keeping only the forwarder handle.
    /// 2. Forward one touch event — must not panic (the napi boundary the shell calls this from has
    ///    no panic net; the relay's callback discipline applies).
    #[test]
    fn input_forwarder_survives_dead_receiver() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<()>::install(&app);
        let forwarder = pump.input_forwarder();
        drop(pump);
        forwarder.forward_input(InputEvent::TouchEvent(TouchEventData::default()));
    }

    /// UT for `OhosInputForwarder` — the handle is shareable across threads.
    /// # Brief
    /// 1. Assert `Send + Sync` on the handle type — the compile-time contract the shell's
    ///    process-global storage slot relies on.
    #[test]
    fn input_forwarder_is_send_sync() {
        fn assert_shareable<T: Send + Sync>() {}
        assert_shareable::<OhosInputForwarder>();
    }

    /// UT for `distill` — every remaining lifecycle variant is dropped at
    /// the relay (negative, value by value).
    /// # Brief
    /// 1. Construct the eleven variants without a p3 consumer, including `UserEvent` (the ability
    ///    waker TSFN channel — explicitly listed so a wildcard arm cannot silently swallow it).
    /// 2. Each distills to `None`.
    #[test]
    fn unmapped_lifecycle_variants_are_traced() {
        let app = OpenHarmonyApp::new();
        let rect = Rect { top: 0, left: 0, width: 10, height: 10 };
        let dropped = [
            Event::Create,
            Event::Start,
            Event::Stop,
            Event::WindowCreate,
            Event::WindowDestroy { window_id: 0 },
            Event::NewWant { uri: String::new() },
            Event::ConfigChanged(Configuration::default()),
            Event::AvoidAreaChange(AvoidAreaInfo {
                area_type: AvoidAreaType::System,
                area: AvoidArea {
                    visible: false,
                    left_rect: rect,
                    top_rect: rect,
                    right_rect: rect,
                    bottom_rect: rect,
                },
            }),
            Event::ContentRectChange(ContentRect {
                reason: RectChangeReason::Undefined,
                rect,
                window_id: 0,
            }),
            Event::SaveState(SaveSaver::new(&app)),
            Event::UserEvent,
        ];
        for event in &dropped {
            assert_eq!(
                distill(event),
                None,
                "variant {} must be dropped at the relay",
                event.as_str()
            );
        }
    }

    /// UT for `wait_surface_ack` — a queued ack resolves to Dropped.
    /// # Brief
    /// 1. Pair a real mpsc ack channel and queue `SurfaceDropped`.
    /// 2. With the short 10ms bound, the wait resolves to `AckWait::Dropped` (the queued ack must
    ///    win over the timeout).
    #[test]
    fn ack_wait_returns_dropped() {
        let (tx, rx) = mpsc::channel::<PumpAck>();
        tx.send(PumpAck::SurfaceDropped).unwrap();
        assert_eq!(wait_surface_ack(&rx, Duration::from_millis(10)), AckWait::Dropped);
    }

    /// UT for `wait_surface_ack` — a silent pump resolves to TimedOut.
    /// # Brief
    /// 1. Pair a channel and keep the sender alive but silent.
    /// 2. With a 10ms bound the wait resolves to `AckWait::TimedOut` (the bounded-ack contract: a
    ///    hung pump must not wedge the NDK callback forever).
    #[test]
    fn ack_wait_times_out() {
        let (_tx, rx) = mpsc::channel::<PumpAck>();
        assert_eq!(wait_surface_ack(&rx, Duration::from_millis(10)), AckWait::TimedOut);
    }

    /// UT for `wait_surface_ack` — a dropped sender resolves to Disconnected.
    /// # Brief
    /// 1. Pair a channel and drop the sender (the pump side is gone).
    /// 2. The wait resolves to `AckWait::Disconnected` immediately (fast path — a dead pump has no
    ///    surface left to protect).
    #[test]
    fn ack_wait_proceeds_on_disconnect() {
        let (tx, rx) = mpsc::channel::<PumpAck>();
        drop(tx);
        assert_eq!(wait_surface_ack(&rx, Duration::from_secs(1)), AckWait::Disconnected);
    }

    /// UT for `RedrawRequester::request_redraw` — the pending flag dedups
    /// wakes, and clearing it re-arms the request.
    /// # Brief
    /// 1. Pair a real pump channel; the channel waker sends `Wake` on it (the pump's own channel is
    ///    the waker, design D5).
    /// 2. Two consecutive requests deliver exactly one `Wake` — the second collapses on the pending
    ///    flag (no wake while a redraw is already pending).
    /// 3. Clearing the flag (what the pump loop does after dispatching `RedrawRequested`) re-arms
    ///    the requester: one more request delivers exactly one further `Wake`.
    #[test]
    fn redraw_requester_dedups_wakes() {
        let (tx, rx) = mpsc::channel::<PumpMessage<()>>();
        let requester = RedrawRequester::new(Arc::new(ChannelWaker::new(tx)));

        requester.request_redraw();
        requester.request_redraw();
        assert!(matches!(rx.try_recv(), Ok(PumpMessage::Wake)));
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));

        requester.pending_flag().clear();
        requester.request_redraw();
        assert!(matches!(rx.try_recv(), Ok(PumpMessage::Wake)));
        assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    }
}
