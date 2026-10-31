//! OHOS event loop: `EventLoop`, `ActiveEventLoop`, `EventLoopProxy` and the
//! platform-specific builder attributes.
//!
//! Split out of `mod.rs` in the p3 phase (openspec change `ohos-winit-p3`):
//! the event pump machinery grew past a single-file module, so the loop types
//! live here while `mod.rs` keeps the module namespace (id types, re-exports)
//! and `pump.rs` owns the relay/distillation layer. Pure move at the split
//! point — no behavior change (see `OHOS_FORK_NOTES.md`).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ohos_hilog_binding::{Hilog, LogOptions};
use openharmony_ability::OpenHarmonyApp;
use tracing::debug;

use crate::error::EventLoopError;
use crate::event::{Event, StartCause, WindowEvent};
use crate::event_loop::{
    self, ActiveEventLoop as RootAEL, ControlFlow, DeviceEvents, EventLoopClosed,
};
use crate::platform::pump_events::PumpStatus;
use crate::window::{CustomCursor, CustomCursorSource, Theme};

use super::input::InputTranslator;
use super::pump::{ChannelWaker, Distilled, PumpAck, PumpMessage, RedrawRequester, SharedFlag};
use super::window::{ability_size_to_physical, MonitorHandle};
use super::{OhosPump, PlatformCustomCursor, WindowId};

/// Platform-specific event loop attributes.
///
/// Two injected halves (design D1): the app handle via
/// [`EventLoopBuilderExtOhos::with_openharmony_app`] and the event pump via
/// [`EventLoopBuilderExtOhos::with_ohos_pump`]; leaving either `None` makes
/// [`EventLoop::new`] panic with guidance.
///
/// This struct stays non-generic — it is the shared field type of the root
/// `EventLoopBuilder`, and genericizing it would change every platform's
/// builder signature — so the pump's `T` is erased at the builder boundary
/// and recovered by `EventLoop::<T>::new`'s downcast.
///
/// [`EventLoopBuilderExtOhos::with_openharmony_app`]: crate::platform::ohos::EventLoopBuilderExtOhos::with_openharmony_app
/// [`EventLoopBuilderExtOhos::with_ohos_pump`]: crate::platform::ohos::EventLoopBuilderExtOhos::with_ohos_pump
/// [`EventLoop::new`]: crate::event_loop::EventLoop::new
#[derive(Default)]
pub(crate) struct PlatformSpecificEventLoopAttributes {
    pub(crate) app: Option<OpenHarmonyApp>,
    /// The injected [`OhosPump`], type-erased here (see struct docs) as
    /// `dyn Any + Send` (design D1). The `Send` half keeps the attributes
    /// type honest about what crosses into it: a pump can only be produced
    /// behind the injection signature's `U: Send + 'static` bound (it has
    /// already crossed the entrypoint's `thread::spawn` boundary). The
    /// downcast in `EventLoop::<T>::new` needs only `'static` (`Any`'s
    /// blanket impl), so any user-event type of the root
    /// `EventLoop<T: 'static>` round-trips the slot.
    ///
    /// [`OhosPump`]: super::OhosPump
    pub(crate) pump: Option<Box<dyn Any + Send>>,
}

/// The platform event loop for OHOS.
///
/// Owns the pump's channel ends for its whole lifetime (design D10): the
/// pump loop consumes `rx` and answers surface-destroy acks on `ack_tx`,
/// while `tx` is the clone source for the user-event proxy and the redraw
/// waker (design D5 — the channel doubles as the waker). Holding them on
/// the loop rather than inside a handler closure is what lets `eframe`'s
/// `with_event_loop` reuse one loop across `run_native` calls on the same
/// thread.
pub struct EventLoop<T: 'static> {
    window_target: event_loop::ActiveEventLoop,
    /// The pump's consuming end — the pump loop's `recv_timeout` target.
    rx: mpsc::Receiver<PumpMessage<T>>,
    /// Sender kept as the clone source for `EventLoopProxy` and the redraw
    /// requester (design D5).
    tx: mpsc::Sender<PumpMessage<T>>,
    /// The ack sender used to release the relay's bounded `SurfaceDestroy`
    /// wait (design D4).
    ack_tx: mpsc::Sender<PumpAck>,
    /// Latches the first `pump_events` call of a run to the `Init`
    /// iteration (android's `loop_running`).
    loop_running: Cell<bool>,
    /// The Pause/Resume gate (android's `running`): redraws are only
    /// dispatched while running.
    running: Cell<bool>,
    /// The redraw-pending flag shared with the requester (design D5): set by
    /// `request_redraw` from any thread, consumed by the pending-redraw gate.
    pending_redraw: SharedFlag,
    /// Whether the current surface's `SurfaceCreated` has been dispatched as
    /// `Resumed` — the release condition of the resumed-first ordering gate
    /// (see [`Self::dispatch_window_or_hold`]).
    resumed_once: Cell<bool>,
    /// Window events held back while no surface exists (arrival order),
    /// flushed right after the `Resumed` of the next `SurfaceCreated`
    /// (see [`Self::dispatch_window_or_hold`]).
    held_back: RefCell<VecDeque<WindowEvent>>,
    /// Loop-side input translation state (design D1): routes each distilled
    /// `Input` to its p4 translation function and threads modifier state
    /// across key events. A `RefCell` because `dispatch_main_event` takes
    /// `&self` — same discipline as `held_back`.
    translator: RefCell<InputTranslator>,
    /// Rate-limited pump-loop counters (p2 design D8, task 2.3,
    /// ohos-craft-p2) — see [`PumpMetrics`].
    metrics: PumpMetrics,
    _marker: PhantomData<T>,
}

impl<T: 'static> EventLoop<T> {
    pub(crate) fn new(
        attributes: &mut PlatformSpecificEventLoopAttributes,
    ) -> Result<Self, EventLoopError> {
        // The app handle is not global state on OHOS - it must be injected via
        // `EventLoopBuilderExtOhos::with_openharmony_app` before `build()`.
        // Mirrors the android backend's guided panic on a missing `AndroidApp`.
        let app = attributes
            .app
            .as_ref()
            .expect(
                "An `OpenHarmonyApp` passed to `with_openharmony_app()` is required to create an \
                 `EventLoop` on OpenHarmony",
            )
            .clone();

        // The pump follows the same injection discipline (design D1). The
        // check lives here rather than at the type level because the builder
        // stores the pump type-erased; `take` hands over ownership so the
        // channel ends move straight into the loop.
        let pump = attributes.pump.take().expect(
            "An `OhosPump` built by `OhosPump::install()` and passed to `with_ohos_pump()` is \
             required to create an `EventLoop` on OpenHarmony",
        );
        let Ok(pump) = pump.downcast::<OhosPump<T>>() else {
            panic!(
                "the `OhosPump` injected via `with_ohos_pump` was installed with a different \
                 user-event type: `OhosPump::<T>::install` and `with_ohos_pump` must use the same \
                 `T`"
            );
        };
        let OhosPump { tx, rx, ack_tx } = *pump;

        // The redraw requester (design D5): the flag dedups, the
        // channel-backed waker sends `Wake` on the pump's own channel. Held
        // by the active event loop so windows clone it at creation time
        // (design D6, group 6); the loop keeps a handle to the same flag for
        // its pending-redraw gate.
        let redraw_requester = RedrawRequester::new(Arc::new(ChannelWaker::new(tx.clone())));
        let pending_redraw = redraw_requester.pending_flag();

        Ok(Self {
            window_target: event_loop::ActiveEventLoop {
                p: ActiveEventLoop {
                    app,
                    redraw_requester,
                    control_flow: Cell::new(ControlFlow::default()),
                    exit: Cell::new(false),
                },
                _marker: PhantomData,
            },
            tx,
            rx,
            ack_tx,
            loop_running: Cell::new(false),
            running: Cell::new(false),
            pending_redraw,
            resumed_once: Cell::new(false),
            held_back: RefCell::default(),
            translator: RefCell::new(InputTranslator::new()),
            metrics: PumpMetrics::new(),
            _marker: PhantomData,
        })
    }

    pub fn run<F>(mut self, event_handler: F) -> Result<(), EventLoopError>
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        self.run_on_demand(event_handler)
    }

    pub fn run_on_demand<F>(&mut self, mut event_handler: F) -> Result<(), EventLoopError>
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        // State reset for a fresh run (design D10): the channel ends persist
        // on the loop (so one loop can run again on the same thread — eframe's
        // `with_event_loop`), while the per-run iteration state starts over.
        // The exit flag was already cleared by the root `run_on_demand`
        // wrapper. `running` starts `true` — the loop is live from the first
        // iteration; the ability's `Pause` event flips it off (design D2).
        self.loop_running.set(false);
        self.running.set(true);
        self.pending_redraw.clear();
        // The ordering gate tracks the surface lifetime, not the run: a
        // re-run sees no new `SurfaceCreated` (the ability fires it once per
        // surface), so `resumed_once` must persist — resetting it here would
        // hold every post-re-run window event forever. Anything still held
        // from a previous run is stale — its app is gone.
        self.held_back.borrow_mut().clear();

        loop {
            match self.pump_events(None, &mut event_handler) {
                PumpStatus::Exit(0) => return Ok(()),
                PumpStatus::Exit(code) => return Err(EventLoopError::ExitFailure(code)),
                PumpStatus::Continue => continue,
            }
        }
    }

    pub fn pump_events<F>(&mut self, timeout: Option<Duration>, mut event_handler: F) -> PumpStatus
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        if !self.loop_running.get() {
            self.loop_running.set(true);

            // The `Init` iteration (android parity): no main event, no wait —
            // the handler sees `NewEvents(Init)` → … → `AboutToWait` before
            // any ability event, so state set up there (eframe's viewport) is
            // ready before `Resumed`.
            self.single_iteration(None, IterationTrigger::Init, &mut event_handler);
        }

        // The `Init` iteration may already have requested an exit (android
        // parity).
        if !self.window_target.exiting() {
            self.wait_iteration(timeout, &mut event_handler);
        }

        if self.window_target.exiting() {
            self.loop_running.set(false);

            (event_handler)(Event::LoopExiting, &self.window_target);

            PumpStatus::Exit(0)
        } else {
            PumpStatus::Continue
        }
    }

    /// One wait-then-iterate round (design D10): compute the wait bound from
    /// the control flow, capped by the caller's external timeout, block on
    /// the channel, and run one iteration for whatever came off it.
    fn wait_iteration<F>(&self, external_timeout: Option<Duration>, event_handler: &mut F)
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        let start = Instant::now();
        let control_flow = self.window_target.control_flow();
        let deadline = match control_flow {
            ControlFlow::WaitUntil(deadline) => Some(deadline),
            _ => None,
        };
        let timeout = min_timeout(control_flow_timeout(control_flow, start), external_timeout);

        let (trigger, message) = if timeout.is_none()
            && should_skip_blocking_wait(self.running.get(), self.pending_redraw.is_set())
        {
            // The flag-starvation watchdog (p2 design D8, ohos-craft-p2): an
            // unbounded wait with the pending-redraw flag set is the G7
            // terminal state — a blocking `recv` would strand the flag (the
            // queue is empty, so nothing would ever wake the loop again;
            // input revives it, the renderer is irrelevant). Skip the block
            // and run a no-message iteration; the pending-redraw gate at its
            // end consumes the flag via `take`, so the next wait blocks
            // normally — at most one skip per flag set, no busy loop. Only
            // the `timeout == None` arm is guarded: a bounded wait always
            // self-recovers, and `None` arises only under a plain
            // `ControlFlow::Wait` with no external cap (hence no resume to
            // report).
            self.metrics.count(&self.metrics.watchdog_skip);
            (IterationTrigger::Message { start, requested_resume: None }, None)
        } else {
            let received = match timeout {
                None => {
                    self.metrics.count(&self.metrics.wait_block);
                    self.rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
                },
                Some(duration) => self.rx.recv_timeout(duration),
            };

            match received {
                Ok(message) => {
                    let trigger = match control_flow {
                        // Under `Poll` every iteration reports `Poll`, message or
                        // not (android parity).
                        ControlFlow::Poll => IterationTrigger::Poll,
                        _ => IterationTrigger::Message { start, requested_resume: deadline },
                    };
                    (trigger, Some(message))
                },
                Err(RecvTimeoutError::Timeout) => {
                    let trigger = match control_flow {
                        ControlFlow::Poll => IterationTrigger::Poll,
                        // The external cap fired under plain `Wait`: the wait was
                        // cancelled, not resumed (android maps this arm to
                        // `WaitCancelled` with no resume).
                        ControlFlow::Wait => {
                            IterationTrigger::Message { start, requested_resume: None }
                        },
                        // The external cap may fire before the deadline — report
                        // the wake as cancelled, not resumed (android's
                        // now-before-deadline race check).
                        ControlFlow::WaitUntil(resume) if Instant::now() < resume => {
                            IterationTrigger::Message { start, requested_resume: Some(resume) }
                        },
                        ControlFlow::WaitUntil(resume) => {
                            IterationTrigger::Deadline { start, requested_resume: resume }
                        },
                    };
                    (trigger, None)
                },
                Err(RecvTimeoutError::Disconnected) => {
                    // Unreachable while this loop runs — it holds `tx` alive on
                    // `self`, so the channel cannot disconnect mid-run. Handle it
                    // defensively anyway: flag the exit and run a no-message
                    // iteration, so the handler sees one consistent cycle instead
                    // of a busy loop on the sticky disconnect.
                    debug!("ohos pump: channel disconnected — exiting the loop");
                    self.window_target.exit();
                    (IterationTrigger::Message { start, requested_resume: deadline }, None)
                },
            }
        };

        self.single_iteration(message, trigger, event_handler);
        self.log_pump_counters();
    }

    /// One pump iteration (design D10): `NewEvents(cause)` → the main event
    /// (D2 table) → the pending redraw → `AboutToWait` — android's iteration
    /// shape; the 0.29-era `MainEventsCleared`/`RedrawEventsCleared` marks in
    /// the D10 sketch do not exist on winit 0.30's `Event` enum (removed in
    /// the 0.30 event redesign), so the corrected sequence omits them.
    fn single_iteration<F>(
        &self,
        message: Option<PumpMessage<T>>,
        trigger: IterationTrigger,
        event_handler: &mut F,
    ) where
        F: FnMut(Event<T>, &RootAEL),
    {
        let el = &self.window_target;
        (event_handler)(Event::NewEvents(iteration_cause(trigger)), el);

        if let Some(message) = message {
            self.dispatch_main_event(message, event_handler);
        }

        // The pending-redraw gate (design D5/D10): dispatch at most one
        // redraw per iteration, only while running. The take both tests and
        // re-arms the requester's flag, so a request made during this
        // redraw's own dispatch re-arms the next iteration, not this one.
        if self.running.get() && self.pending_redraw.take() {
            self.metrics.count(&self.metrics.redraw_dispatch);
            self.dispatch_window_event(WindowEvent::RedrawRequested, event_handler);
        }

        (event_handler)(Event::AboutToWait, el);
    }

    /// Dispatches one `WindowEvent` addressed to the single implicit OHOS
    /// window (design D10).
    fn dispatch_window_event<F>(&self, event: WindowEvent, event_handler: &mut F)
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        (event_handler)(
            Event::WindowEvent { window_id: crate::window::WindowId(WindowId::dummy()), event },
            &self.window_target,
        );
    }

    /// Dispatches one `WindowEvent` to the single implicit OHOS window, or
    /// holds it back until a surface exists — the resumed-first ordering
    /// gate.
    ///
    /// The ability fires `GainedFocus` / `WindowResize` before
    /// `SurfaceCreate` (p1 R2 evidence), but the mobile contract eframe is
    /// built against — android's — guarantees `Resumed` (window creation)
    /// precedes any window event: eframe's `window_event` returns
    /// `EventResult::Exit` while its run state does not exist yet, so
    /// dispatching as-arrived made `run_native` exit cleanly before the
    /// first frame (field report 2026-10-16). The gate holds the early
    /// arrivals; the `SurfaceCreated` arm flushes them, in arrival order,
    /// right after the `Resumed` they were waiting for.
    fn dispatch_window_or_hold<F>(&self, event: WindowEvent, event_handler: &mut F)
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        if self.resumed_once.get() {
            self.dispatch_window_event(event, event_handler);
        } else {
            self.held_back.borrow_mut().push_back(event);
        }
    }

    /// Dispatches one pump message per the D2 mapping table (design D10's
    /// main-event step).
    fn dispatch_main_event<F>(&self, message: PumpMessage<T>, event_handler: &mut F)
    where
        F: FnMut(Event<T>, &RootAEL),
    {
        let el = &self.window_target;

        match message {
            PumpMessage::Event(Distilled::SurfaceCreated) => {
                // eframe creates its window inside `Resumed` (`native_window()`
                // is `Some` by then), then learns its extents — only for a
                // non-zero rect (the XComponent reports zero-sized rects
                // before the first layout pass).
                (event_handler)(Event::Resumed, el);
                // Release the ordering gate and flush what it held (in
                // arrival order): the window these events address now
                // exists. Collected first so the handler runs without the
                // queue borrow held.
                self.resumed_once.set(true);
                let held_back: Vec<WindowEvent> = self.held_back.borrow_mut().drain(..).collect();
                for event in held_back {
                    self.dispatch_window_event(event, event_handler);
                }
                let rect = el.p.app.content_rect();
                if rect.width > 0 && rect.height > 0 {
                    self.dispatch_window_event(
                        WindowEvent::Resized(ability_size_to_physical(rect.width, rect.height)),
                        event_handler,
                    );
                }
            },
            PumpMessage::Event(Distilled::SurfaceDestroyed) => {
                // The handler drops the EGL surface synchronously inside
                // `Suspended` (eframe), so once it returns the NDK is free to
                // invalidate the native window — release the relay's bounded
                // wait (design D4).
                (event_handler)(Event::Suspended, el);
                if self.ack_tx.send(PumpAck::SurfaceDropped).is_err() {
                    debug!("ohos pump: relay gone before the surface-dropped ack");
                }
                // Re-arm the ordering gate: the window is gone with the
                // surface, so window events belong to the next surface's
                // `Resumed` again.
                self.resumed_once.set(false);
                // Modifier state does not survive the surface either — the
                // same reset as focus loss (design D6).
                self.translator.borrow_mut().reset();
            },
            PumpMessage::Event(Distilled::Resized(size)) => {
                self.dispatch_window_or_hold(WindowEvent::Resized(size), event_handler);
            },
            PumpMessage::Event(Distilled::Focused(focused)) => {
                self.dispatch_window_or_hold(WindowEvent::Focused(focused), event_handler);
                // The OS resets modifiers on focus loss — drop any stale
                // SHIFT/etc. so it cannot poison the next session (design D6).
                if !focused {
                    self.translator.borrow_mut().reset();
                }
            },
            PumpMessage::Event(Distilled::Input(input)) => {
                // p4 (design D1/D7): translate loop-side, then dispatch through
                // the same resumed-first gate as any other window event — no
                // separate gating path for input. Collected first so the
                // handler runs without the translator borrow held.
                let events = self.translator.borrow_mut().translate(&input);
                for event in events {
                    self.dispatch_window_or_hold(event, event_handler);
                }
            },
            PumpMessage::Event(Distilled::MemoryWarning) => {
                (event_handler)(Event::MemoryWarning, el);
            },
            PumpMessage::Event(Distilled::SetRunning(running)) => {
                // The Pause/Resume gate — pump-internal state, not a winit
                // event (android parity).
                self.running.set(running);
            },
            PumpMessage::Event(Distilled::ExitRequested) => {
                el.p.exit();
            },
            PumpMessage::UserEvent(event) => {
                self.metrics.count(&self.metrics.user_event);
                (event_handler)(Event::UserEvent(event), el);
            },
            PumpMessage::Wake => {
                // A redraw wake with nothing to dispatch: the pending-redraw
                // gate at the end of the iteration consumes the flag, and a
                // stale wake (the flag already cleared) falls through as a
                // no-op.
            },
        }
    }

    pub fn window_target(&self) -> &RootAEL {
        &self.window_target
    }

    pub fn create_proxy(&self) -> EventLoopProxy<T> {
        EventLoopProxy { tx: self.tx.clone() }
    }

    /// Flushes the pump-loop counters to hilog at most once per second (p2
    /// design D8, task 2.3, ohos-craft-p2) — G7 terminal-state evidence
    /// (`watchdog_skip_count` above zero marks a starvation hit) and SC4
    /// sustained-interaction evidence (`redraw_dispatch_count` keeps flowing,
    /// with no long zero-dispatch stretch where `watchdog_skip_count` is also
    /// stuck at zero). The counters are lifetime-cumulative; consumers derive
    /// rates from consecutive lines. Called at the end of every wait
    /// iteration — the 1Hz gate keeps the steady-state cost at two `Cell`
    /// loads.
    ///
    /// Tests never reach the flush: each constructs a fresh loop and
    /// finishes well inside the first one-second window, and `Hilog`'s
    /// instance API stores options without FFI — only this flush calls
    /// `OH_LOG_Print`.
    fn log_pump_counters(&self) {
        let now = Instant::now();
        if now.duration_since(self.metrics.last_flush.get()) < Duration::from_secs(1) {
            return;
        }
        self.metrics.last_flush.set(now);
        self.metrics.hilog.info(format!(
            "pump counters: wait_block_count={} watchdog_skip_count={} redraw_dispatch_count={} \
             user_event_count={}",
            self.metrics.wait_block.get(),
            self.metrics.watchdog_skip.get(),
            self.metrics.redraw_dispatch.get(),
            self.metrics.user_event.get(),
        ));
    }
}

/// Rate-limited hilog counters for the pump loop (p2 design D8, task 2.3,
/// ohos-craft-p2).
///
/// The four counters cover the G7 starvation story end to end: `wait_block`
/// counts unbounded blocking waits (the arm the watchdog guards),
/// `watchdog_skip` counts watchdog hits (zero in a healthy loop),
/// `redraw_dispatch` counts redraws dispatched from the pending-redraw gate,
/// and `user_event` counts `UserEvent` messages — eframe's repaint request
/// rides that arm, so the user_event/redraw_dispatch pairing is the
/// starvation signature the watchdog breaks. Counters are
/// lifetime-cumulative across `run_on_demand` re-runs on the same loop. The
/// `Cell` fields match the loop's single-thread discipline (`running` already
/// is one), and the `Hilog` instance carries the fork's own tag
/// (`OHOS_SPIKE`) so the app's global hilog configuration is untouched.
struct PumpMetrics {
    wait_block: Cell<u64>,
    watchdog_skip: Cell<u64>,
    redraw_dispatch: Cell<u64>,
    user_event: Cell<u64>,
    /// The last 1Hz aggregate flush; seeded at construction so the first
    /// flush lands one second into the run, not at t=0 with all-zero
    /// counters.
    last_flush: Cell<Instant>,
    hilog: Hilog,
}

impl PumpMetrics {
    fn new() -> Self {
        let hilog = Hilog::new();
        hilog.with_options(LogOptions { domain: 0, tag: "OHOS_SPIKE" });
        Self {
            wait_block: Cell::new(0),
            watchdog_skip: Cell::new(0),
            redraw_dispatch: Cell::new(0),
            user_event: Cell::new(0),
            last_flush: Cell::new(Instant::now()),
            hilog,
        }
    }

    /// Bumps one counter — `Cell` keeps this `&self`, matching the loop's
    /// single-thread discipline.
    fn count(&self, counter: &Cell<u64>) {
        counter.set(counter.get() + 1);
    }
}

/// How one pump iteration came to run (design D10's cause table — the OHOS
/// counterpart of android's `PollEvent` → `StartCause` mapping).
///
/// Each variant carries exactly the fields its [`StartCause`] image needs,
/// so the mapping cannot construct an impossible combination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IterationTrigger {
    /// A pump message arrived before the wait bound — `WaitCancelled`,
    /// carrying the wait's start instant and the `WaitUntil` deadline that
    /// was in force, if any.
    Message { start: Instant, requested_resume: Option<Instant> },
    /// The `WaitUntil` deadline elapsed — `ResumeTimeReached`.
    Deadline { start: Instant, requested_resume: Instant },
    /// Zero-timeout iteration under `ControlFlow::Poll`.
    Poll,
    /// The first iteration of a run (android's initial `single_iteration`
    /// with no main event).
    Init,
}

/// Maps one iteration's trigger onto the winit [`StartCause`] (design D10).
pub(crate) fn iteration_cause(trigger: IterationTrigger) -> StartCause {
    match trigger {
        IterationTrigger::Message { start, requested_resume } => {
            StartCause::WaitCancelled { start, requested_resume }
        },
        IterationTrigger::Deadline { start, requested_resume } => {
            StartCause::ResumeTimeReached { start, requested_resume }
        },
        IterationTrigger::Poll => StartCause::Poll,
        IterationTrigger::Init => StartCause::Init,
    }
}

/// Converts the loop's [`ControlFlow`] into the pump's wait bound (design
/// D10): `Wait` blocks indefinitely, `WaitUntil` waits out the remaining
/// time (a passed deadline saturates to zero — android's
/// `saturating_duration_since`), `Poll` returns immediately.
pub(crate) fn control_flow_timeout(control_flow: ControlFlow, now: Instant) -> Option<Duration> {
    match control_flow {
        ControlFlow::Wait => None,
        ControlFlow::Poll => Some(Duration::ZERO),
        ControlFlow::WaitUntil(deadline) => Some(deadline.saturating_duration_since(now)),
    }
}

/// Picks the smaller of the control-flow wait bound and the caller's external
/// pump timeout (`None` = no bound, i.e. block indefinitely — android's
/// `min_timeout`).
fn min_timeout(
    control_flow_timeout: Option<Duration>,
    external_timeout: Option<Duration>,
) -> Option<Duration> {
    match (control_flow_timeout, external_timeout) {
        (Some(control_flow), Some(external)) => Some(control_flow.min(external)),
        (control_flow, None) => control_flow,
        (None, external) => external,
    }
}

/// The flag-starvation watchdog's decision (p2 design D8, ohos-craft-p2):
/// whether `wait_iteration` should skip its blocking `recv` and run a
/// no-message iteration instead.
///
/// The G7 pathology this guards against: eframe's repaint request produces
/// two messages (a `UserEvent::RequestRepaint` through the proxy and a
/// `Wake` from the requester), the pump consumes one per iteration, and the
/// freshness guard in eframe's `run` drops the stale `UserEvent` — leaving
/// the queue empty while the pending-redraw flag is set. A blocking `recv`
/// in that state strands the flag forever (nothing ever wakes the loop;
/// input revives it, the renderer is irrelevant). Skipping exactly when the
/// pending-redraw gate at the iteration's end would consume the flag (`take`)
/// resolves the stall with at most one skip per flag set — no busy loop.
///
/// Only the unbounded branch consults this: a bounded wait (`Some(timeout)`)
/// always self-recovers when its timeout fires.
pub(crate) fn should_skip_blocking_wait(running: bool, pending_redraw: bool) -> bool {
    running && pending_redraw
}

/// The proxy returned by [`EventLoop::create_proxy`].
///
/// [`EventLoop::create_proxy`]: crate::event_loop::EventLoop::create_proxy
// No `PhantomData<T>` marker (unlike the loop itself): the sender field
// already carries `T`, and a marker would make the auto traits demand
// `T: Sync` — upstream's `sync_object` regression asserts
// `EventLoopProxy<T: Send>: Sync` (windows derives it the same way, from
// the sender field alone).
pub struct EventLoopProxy<T: 'static> {
    /// Sender clone from the pump's own channel (design D5): a user event
    /// send is also the wake that unblocks the pump loop's `recv_timeout`.
    tx: mpsc::Sender<PumpMessage<T>>,
}

impl<T: 'static> Clone for EventLoopProxy<T> {
    fn clone(&self) -> Self {
        Self { tx: self.tx.clone() }
    }
}

impl<T> EventLoopProxy<T> {
    pub fn send_event(&self, event: T) -> Result<(), EventLoopClosed<T>> {
        match self.tx.send(PumpMessage::UserEvent(event)) {
            Ok(()) => Ok(()),
            // The pump loop is gone (the loop that owns the receiver has
            // exited) — hand the untouched event back per the root
            // `EventLoopClosed` contract.
            Err(send_error) => match send_error.0 {
                PumpMessage::UserEvent(event) => Err(EventLoopClosed(event)),
                // Unreachable by construction: this proxy is the only sender
                // of `UserEvent` on the channel (upstream-parity idiom for
                // structurally impossible arms, see the ios backend).
                _ => unreachable!("only `UserEvent` is sent by the proxy"),
            },
        }
    }
}

/// The platform active event loop for OHOS.
///
/// Control-flow and exit state are real; the app handle (回改项 2 — the p2
/// skeleton stored nothing) backs the window and monitor queries (design
/// D6), cloned out of the loop's builder-time injection, and the redraw
/// requester (design D5) is cloned into every window at creation time.
pub struct ActiveEventLoop {
    /// The query-side handle: cloned into windows at creation time and into
    /// monitor handles at enumeration time (design D6).
    pub(crate) app: OpenHarmonyApp,
    /// The redraw requester (design D5): windows clone it so
    /// `request_redraw` works from any thread with flag dedup.
    pub(crate) redraw_requester: RedrawRequester,
    control_flow: Cell<ControlFlow>,
    exit: Cell<bool>,
}

impl ActiveEventLoop {
    pub fn primary_monitor(&self) -> Option<MonitorHandle> {
        // The single default display (design D6, android parity).
        Some(MonitorHandle::new(self.app.clone()))
    }

    pub fn available_monitors(&self) -> VecDeque<MonitorHandle> {
        let mut monitors = VecDeque::with_capacity(1);
        monitors.push_back(MonitorHandle::new(self.app.clone()));
        monitors
    }

    pub fn create_custom_cursor(&self, source: CustomCursorSource) -> CustomCursor {
        let _ = source.inner;
        CustomCursor { inner: PlatformCustomCursor }
    }

    pub fn listen_device_events(&self, _allowed: DeviceEvents) {}

    pub fn system_theme(&self) -> Option<Theme> {
        None
    }

    pub(crate) fn set_control_flow(&self, control_flow: ControlFlow) {
        self.control_flow.set(control_flow)
    }

    pub(crate) fn control_flow(&self) -> ControlFlow {
        self.control_flow.get()
    }

    pub(crate) fn exit(&self) {
        self.exit.set(true)
    }

    pub(crate) fn clear_exit(&self) {
        self.exit.set(false)
    }

    pub(crate) fn exiting(&self) -> bool {
        self.exit.get()
    }

    pub(crate) fn owned_display_handle(&self) -> OwnedDisplayHandle {
        OwnedDisplayHandle
    }

    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        // rwh_05 has no OHOS handle types; rwh_06 (OhosDisplayHandle /
        // OhosNdkWindowHandle) is the supported path on OHOS.
        unimplemented!(
            "ohos backend: rwh_05 has no OHOS display handle type, enable the rwh_06 feature"
        )
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::OhosDisplayHandle::new().into())
    }
}

#[derive(Clone)]
pub(crate) struct OwnedDisplayHandle;

impl OwnedDisplayHandle {
    #[cfg(feature = "rwh_05")]
    #[inline]
    pub fn raw_display_handle_rwh_05(&self) -> rwh_05::RawDisplayHandle {
        // rwh_05 has no OHOS handle types; rwh_06 (OhosDisplayHandle /
        // OhosNdkWindowHandle) is the supported path on OHOS.
        unimplemented!(
            "ohos backend: rwh_05 has no OHOS display handle type, enable the rwh_06 feature"
        )
    }

    #[cfg(feature = "rwh_06")]
    #[inline]
    pub fn raw_display_handle_rwh_06(
        &self,
    ) -> Result<rwh_06::RawDisplayHandle, rwh_06::HandleError> {
        Ok(rwh_06::OhosDisplayHandle::new().into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::OhosPump;
    use super::*;
    use crate::platform::ohos::EventLoopBuilderExtOhos;

    /// UT for `EventLoop::new` — building without a pump panics with
    /// guidance pointing at the installer.
    /// # Brief
    /// 1. Attributes carry only the app handle (real `OpenHarmonyApp::new()`, pure memory — no NDK
    ///    dependency).
    /// 2. `EventLoop::<u8>::new` panics; the message names `OhosPump::install` so the fix is
    ///    discoverable (mirrors the missing-app guided panic shape).
    #[test]
    #[should_panic(expected = "OhosPump::install")]
    fn event_loop_new_panics_without_pump() {
        let mut attributes =
            PlatformSpecificEventLoopAttributes { app: Some(OpenHarmonyApp::new()), pump: None };
        let _ = EventLoop::<u8>::new(&mut attributes);
    }

    /// UT for `EventLoop::new` — a pump installed with a different
    /// user-event type than the loop's panics on the downcast.
    /// # Brief
    /// 1. Install the pump as `OhosPump::<u8>` but build `EventLoop::<String>` — the erased slot
    ///    holds the wrong concrete type.
    /// 2. The downcast fails and the panic states that `install` and `with_ohos_pump` must use the
    ///    same `T`.
    #[test]
    #[should_panic(expected = "must use the same `T`")]
    fn event_loop_new_panics_on_type_mismatch() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<u8>::install(&app);
        let mut attributes =
            PlatformSpecificEventLoopAttributes { app: Some(app), pump: Some(Box::new(pump)) };
        let _ = EventLoop::<String>::new(&mut attributes);
    }

    /// UT for `EventLoop::new` — both injections with matching `T`
    /// construct successfully.
    /// # Brief
    /// 1. Install `OhosPump::<u8>` and build `EventLoop::<u8>` — the downcast recovers the pump.
    /// 2. Construction returns `Ok` (guards against the validation panics over-firing).
    #[test]
    fn event_loop_builds_with_both_injections() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<u8>::install(&app);
        let mut attributes =
            PlatformSpecificEventLoopAttributes { app: Some(app), pump: Some(Box::new(pump)) };
        let _event_loop = EventLoop::<u8>::new(&mut attributes).unwrap();
    }

    /// UT for `EventLoopBuilderExtOhos` — the app injection lands in the
    /// builder's attributes (the query-side half of the contract).
    /// # Brief
    /// 1. A fresh root builder stores neither the app nor the pump.
    /// 2. `with_openharmony_app` fills the app slot; `with_ohos_pump` fills the pump slot — two
    ///    independent injections, two independent slots.
    #[test]
    fn with_openharmony_app_stores_query_handle() {
        let mut builder = event_loop::EventLoop::builder();
        assert!(builder.platform_specific.app.is_none());
        assert!(builder.platform_specific.pump.is_none());

        builder.with_openharmony_app(OpenHarmonyApp::new());
        assert!(builder.platform_specific.app.is_some());
        assert!(builder.platform_specific.pump.is_none());

        builder.with_ohos_pump(OhosPump::<()>::install(&OpenHarmonyApp::new()));
        assert!(builder.platform_specific.pump.is_some());
        assert!(builder.platform_specific.app.is_some());
    }

    /// UT for `control_flow_timeout` — the ControlFlow→wait-bound conversion
    /// (design D10).
    /// # Brief
    /// 1. `Wait` → `None` (block indefinitely); `Poll` → `Some(ZERO)` (return immediately).
    /// 2. `WaitUntil` in the future → `Some(remaining)` relative to the passed now.
    /// 3. `WaitUntil` in the past → `Some(ZERO)` — the saturation guard, never a negative duration
    ///    (android's `saturating_duration_since` parity).
    #[test]
    fn control_flow_timeout_mapping() {
        let now = Instant::now();
        assert_eq!(control_flow_timeout(ControlFlow::Wait, now), None);
        assert_eq!(control_flow_timeout(ControlFlow::Poll, now), Some(Duration::ZERO));

        let future = now + Duration::from_secs(10);
        assert_eq!(
            control_flow_timeout(ControlFlow::WaitUntil(future), now),
            Some(Duration::from_secs(10)),
        );

        let past = now - Duration::from_secs(10);
        assert_eq!(control_flow_timeout(ControlFlow::WaitUntil(past), now), Some(Duration::ZERO));
    }

    /// UT for `iteration_cause` — the trigger→StartCause table (design D10,
    /// android's three-cause parity).
    /// # Brief
    /// 1. A message trigger maps to `WaitCancelled`, carrying the start instant and the deadline in
    ///    force (`None` under plain `Wait`).
    /// 2. A deadline trigger maps to `ResumeTimeReached` carrying start + deadline.
    /// 3. `Poll` maps to `Poll`; `Init` maps to `Init`.
    #[test]
    fn iteration_cause_matches_trigger() {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(5);

        assert_eq!(
            iteration_cause(IterationTrigger::Message { start, requested_resume: None }),
            StartCause::WaitCancelled { start, requested_resume: None },
        );
        assert_eq!(
            iteration_cause(IterationTrigger::Message { start, requested_resume: Some(deadline) }),
            StartCause::WaitCancelled { start, requested_resume: Some(deadline) },
        );
        assert_eq!(
            iteration_cause(IterationTrigger::Deadline { start, requested_resume: deadline }),
            StartCause::ResumeTimeReached { start, requested_resume: deadline },
        );
        assert_eq!(iteration_cause(IterationTrigger::Poll), StartCause::Poll);
        assert_eq!(iteration_cause(IterationTrigger::Init), StartCause::Init);
    }

    /// UT for the resumed-first ordering gate — window events the ability
    /// fires before `SurfaceCreate` must be held back and flushed, in arrival
    /// order, right after the `Resumed` they were waiting for.
    /// # Brief
    /// 1. Wire a real `relay` into a hand-built `OhosPump` (the channel ends the production
    ///    `install` would create) and build an `EventLoop` from it.
    /// 2. Feed the p1-R2-evidenced ability startup order: `GainedFocus` and `WindowResize` arrive
    ///    BEFORE `SurfaceCreate`. Dispatching them as they arrive would kill the loop before it
    ///    ever sees `Resumed`: eframe's `window_event` returns `EventResult::Exit` while its run
    ///    state (the window) does not exist, so `run_native` exits cleanly with zero frames painted
    ///    (field report 2026-10-16). Android, the reference order eframe is built against,
    ///    guarantees Resumed-first — the pump must synthesize that guarantee from the OHOS order.
    /// 3. A post-surface `WindowResize` must dispatch immediately, not queue behind anything;
    ///    `Destroy` ends the run.
    /// 4. The handler's observation order must be exactly: `resumed` → the two held-back window
    ///    events → the post-surface resize — no window event may precede the first `resumed`.
    #[test]
    fn window_events_held_back_until_first_resumed() {
        use super::super::pump::relay;
        use openharmony_ability::{Event as AbilityEvent, Size};

        let (tx, rx) = mpsc::channel::<PumpMessage<u8>>();
        let (ack_tx, ack_rx) = mpsc::channel::<PumpAck>();
        let mut attributes = PlatformSpecificEventLoopAttributes {
            app: Some(OpenHarmonyApp::new()),
            pump: Some(Box::new(OhosPump { tx: tx.clone(), rx, ack_tx })),
        };
        let mut event_loop = EventLoop::<u8>::new(&mut attributes).unwrap();

        let sender = std::thread::spawn(move || {
            // The ability startup order (p1 R2 evidence): focus and resize
            // precede the surface; the surface precedes the next resize.
            relay(&AbilityEvent::GainedFocus { window_id: 0 }, &tx, &ack_rx);
            relay(
                &AbilityEvent::WindowResize {
                    window_id: 0,
                    size: Size { width: 1080, height: 2340 },
                },
                &tx,
                &ack_rx,
            );
            relay(&AbilityEvent::SurfaceCreate, &tx, &ack_rx);
            relay(
                &AbilityEvent::WindowResize {
                    window_id: 0,
                    size: Size { width: 720, height: 1600 },
                },
                &tx,
                &ack_rx,
            );
            relay(&AbilityEvent::Destroy, &tx, &ack_rx);
        });

        let mut observed: Vec<String> = Vec::new();
        event_loop
            .run_on_demand(|event, _el| match event {
                Event::Resumed => observed.push("resumed".to_owned()),
                Event::WindowEvent { event: WindowEvent::Focused(true), .. } => {
                    observed.push("focused".to_owned());
                },
                Event::WindowEvent { event: WindowEvent::Resized(size), .. } => {
                    observed.push(format!("resized {}x{}", size.width, size.height));
                },
                _ => (),
            })
            .unwrap();
        sender.join().unwrap();

        assert_eq!(
            observed,
            vec![
                "resumed".to_owned(),
                "focused".to_owned(),
                "resized 1080x2340".to_owned(),
                "resized 720x1600".to_owned(),
            ],
            "no window event may precede the first Resumed, and held-back events must flush in \
             arrival order"
        );
    }

    /// UT for the input dispatch gate — translated input follows the same
    /// resumed-first ordering gate as regular window events (design D7: no
    /// separate gating path for input).
    /// # Brief
    /// 1. Wire a real `relay` into a hand-built `OhosPump` (the channel ends the production
    ///    `install` would create) and build an `EventLoop` from it.
    /// 2. Feed a touch Down BEFORE `SurfaceCreate` — it must translate but be held back;
    ///    `SurfaceCreate` flushes it after the `Resumed`; a post-surface touch Move dispatches
    ///    immediately; `SurfaceDestroy` (`Suspended`) re-arms the gate, so a post-suspend touch Up
    ///    is held again and never dispatched; `Destroy` ends the run.
    /// 3. The observed order must be exactly: resumed → touch Started → touch Moved → suspended —
    ///    the post-suspend touch appears nowhere.
    #[test]
    fn input_held_back_until_resumed() {
        use super::super::pump::relay;
        use ohos_xcomponent_binding::{TouchEvent, TouchEventData, TouchPointData};
        use openharmony_ability::{Event as AbilityEvent, InputEvent};

        let (tx, rx) = mpsc::channel::<PumpMessage<u8>>();
        let (ack_tx, ack_rx) = mpsc::channel::<PumpAck>();
        let mut attributes = PlatformSpecificEventLoopAttributes {
            app: Some(OpenHarmonyApp::new()),
            pump: Some(Box::new(OhosPump { tx: tx.clone(), rx, ack_tx })),
        };
        let mut event_loop = EventLoop::<u8>::new(&mut attributes).unwrap();

        let sender = std::thread::spawn(move || {
            let touch =
                |event_type: TouchEvent, touch_points: Vec<TouchPointData>| AbilityEvent::Input {
                    window_id: 0,
                    input: InputEvent::TouchEvent(TouchEventData {
                        event_type,
                        touch_points,
                        ..TouchEventData::default()
                    }),
                };
            // Pre-surface input: translated, but held back by the gate.
            relay(&touch(TouchEvent::Down, Vec::new()), &tx, &ack_rx);
            relay(&AbilityEvent::SurfaceCreate, &tx, &ack_rx);
            // Post-surface input: dispatches immediately.
            relay(
                &touch(TouchEvent::Move, vec![TouchPointData {
                    event_type: TouchEvent::Move,
                    x: 5.0,
                    y: 6.0,
                    ..TouchPointData::default()
                }]),
                &tx,
                &ack_rx,
            );
            // Suspend re-arms the gate...
            relay(&AbilityEvent::SurfaceDestroy, &tx, &ack_rx);
            // ...so this input is held again and never dispatched.
            relay(&touch(TouchEvent::Up, Vec::new()), &tx, &ack_rx);
            relay(&AbilityEvent::Destroy, &tx, &ack_rx);
        });

        let mut observed: Vec<String> = Vec::new();
        event_loop
            .run_on_demand(|event, _el| match event {
                Event::Resumed => observed.push("resumed".to_owned()),
                Event::Suspended => observed.push("suspended".to_owned()),
                Event::WindowEvent { event: WindowEvent::Touch(touch), .. } => {
                    observed.push(format!("touch {:?}", touch.phase));
                },
                _ => (),
            })
            .unwrap();
        sender.join().unwrap();

        assert_eq!(
            observed,
            vec![
                "resumed".to_owned(),
                "touch Started".to_owned(),
                "touch Moved".to_owned(),
                "suspended".to_owned(),
            ],
            "input follows the resumed-first gate: held before Resumed, flushed in arrival order \
             after it, re-armed after Suspended"
        );
    }

    /// UT for `run_on_demand` + `EventLoopProxy` — a user event wakes a
    /// blocked loop, round-trips to the handler, and the closed loop rejects
    /// sends.
    /// # Brief
    /// 1. Build a dual-injected `EventLoop` (group-3 infrastructure) and a proxy.
    /// 2. From another thread, `send_event(42)` while the loop blocks in `Wait` — the send itself
    ///    must wake the pump (design D5); the handler observes `Event::UserEvent(42)` and exits the
    ///    loop; `run_on_demand` returns `Ok`.
    /// 3. Dropping the loop drops the channel receiver — a further proxy send returns
    ///    `EventLoopClosed` with the event handed back.
    #[test]
    fn proxy_user_event_roundtrip_and_closure() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<u8>::install(&app);
        let mut attributes =
            PlatformSpecificEventLoopAttributes { app: Some(app), pump: Some(Box::new(pump)) };
        let mut event_loop = EventLoop::<u8>::new(&mut attributes).unwrap();
        let proxy = event_loop.create_proxy();

        let waking_proxy = proxy.clone();
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            waking_proxy.send_event(42)
        });

        let mut saw_user_event = false;
        event_loop
            .run_on_demand(|event, el| {
                if let Event::UserEvent(42) = event {
                    saw_user_event = true;
                    el.exit();
                }
            })
            .unwrap();
        assert!(saw_user_event);
        assert!(sender.join().unwrap().is_ok());

        drop(event_loop);
        assert!(matches!(proxy.send_event(7), Err(EventLoopClosed(7))));
    }

    /// UT for the flag-starvation watchdog's decision (p2 design D8,
    /// ohos-craft-p2) — `should_skip_blocking_wait`, all four input
    /// combinations.
    /// # Brief
    /// 1. `(running, pending_redraw)` = (F, F), (F, T), (T, F) → do not skip: a paused loop must
    ///    not spin on a set flag (the gate drops it after the next `SetRunning(true)`), and a live
    ///    loop with nothing pending has no reason to leave the wait.
    /// 2. `(T, T)` → skip: the G7 terminal state (queue empty + flag set + unbounded `Wait`) is
    ///    exactly the state where a blocking recv would strand the flag forever.
    #[test]
    fn skip_blocking_wait_when_pending_redraw_set() {
        assert!(!should_skip_blocking_wait(false, false));
        assert!(!should_skip_blocking_wait(false, true));
        assert!(!should_skip_blocking_wait(true, false));
        assert!(should_skip_blocking_wait(true, true));
    }

    /// UT for the redraw wake path (event-pump spec scenario "request_redraw
    /// 唤醒并派发 RedrawRequested" — previously covered by no
    /// assertion-bearing test) — `request_redraw` from another thread wakes a
    /// blocked pump and dispatches exactly one `RedrawRequested`; residual
    /// `Wake` messages with no pending flag are swallowed without a dispatch.
    /// # Brief
    /// 1. Hand-build the pump channels (the test owns a `tx` clone for raw `Wake` injection) and
    ///    run the loop — it blocks in the default `ControlFlow::Wait`.
    /// 2. Sender thread, spaced by sleeps: (a) a raw stale `Wake`, flag unset — must be a no-op;
    ///    (b) `request_redraw` twice — the flag dedups to one requester wake, one `RedrawRequested`
    ///    dispatch; (c) another raw stale `Wake` after the gate consumed the flag — again no
    ///    dispatch.
    /// 3. The handler exits at the `AboutToWait` of the first full iteration after the redraw —
    ///    that iteration's pending-redraw gate has already run, so a spurious second dispatch
    ///    cannot hide behind the exit.
    /// 4. Assert exactly one `RedrawRequested` across the whole run (flag dedup + stale-wake
    ///    swallow).
    #[test]
    fn request_redraw_wakes_and_dispatches_redraw() {
        let (tx, rx) = mpsc::channel::<PumpMessage<u8>>();
        // The ack receiver stays alive but unconsumed: this test injects raw
        // messages, so no relay ever waits on a surface-dropped ack.
        let (ack_tx, _ack_rx) = mpsc::channel::<PumpAck>();
        let mut attributes = PlatformSpecificEventLoopAttributes {
            app: Some(OpenHarmonyApp::new()),
            pump: Some(Box::new(OhosPump { tx: tx.clone(), rx, ack_tx })),
        };
        let mut event_loop = EventLoop::<u8>::new(&mut attributes).unwrap();
        // The production requester sharing its flag with the loop's
        // pending-redraw gate; the clone crosses to the sender thread.
        let requester = event_loop.window_target.p.redraw_requester.clone();

        let sender = std::thread::spawn(move || {
            // (a) stale wake, no flag set: swallowed, no dispatch.
            std::thread::sleep(Duration::from_millis(100));
            tx.send(PumpMessage::Wake).unwrap();
            // (b) two requests: one flag transition, one requester wake, one dispatch.
            std::thread::sleep(Duration::from_millis(100));
            requester.request_redraw();
            requester.request_redraw();
            // (c) stale wake after the flag was consumed: swallowed, no dispatch.
            std::thread::sleep(Duration::from_millis(100));
            tx.send(PumpMessage::Wake).unwrap();
        });

        let mut redraw_count = 0usize;
        let mut wait_cancelled_after_redraw = 0usize;
        event_loop
            .run_on_demand(|event, el| match event {
                Event::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                    redraw_count += 1;
                },
                Event::NewEvents(StartCause::WaitCancelled { .. }) if redraw_count > 0 => {
                    wait_cancelled_after_redraw += 1;
                },
                Event::AboutToWait if wait_cancelled_after_redraw > 0 => {
                    el.exit();
                },
                _ => (),
            })
            .unwrap();
        sender.join().unwrap();

        assert_eq!(
            redraw_count, 1,
            "two deduped requests must dispatch exactly one RedrawRequested, and stale Wakes with \
             no pending flag must not dispatch"
        );
    }
}
