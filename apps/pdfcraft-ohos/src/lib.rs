//! OpenHarmony entry crate for PdfCraft (ohos-craft-p2).
//!
//! Host face: empty — the crate is compiled out entirely off the ohos target
//! (crate-level cfg below), so host builds and `cargo test -p pdfcraft-ohos`
//! see an empty crate. The ohos face ports the frame-spike blueprint
//! (ohos-winit p1): `#[ability]` napi entry, hilog/tracing forwarding, the
//! input pump and touch forwarding, and `eframe::run_native` driving
//! `PdfCraftApp` with an embedded sample PDF.

#![cfg(target_env = "ohos")]
// napi-derive-backend-ohos 1.2.0 (the codegen layer napi-derive-ohos
// delegates to) compiles its export-registration ctors out of test builds
// (`#[cfg(all(not(test), ...))]` in its `codegen/fn.rs`) — so the `#[ability]`
// entry chain, live and exercised in the shipping cdylib, reads as dead code
// to the unit-test binary. Suppress dead_code there only; the lib build
// keeps the lint fully enforced (frame-spike precedent).
#![cfg_attr(test, allow(dead_code))]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::OnceLock;
use std::thread;

use ohos_hilog_binding::log::{init_once, Config as HilogLogConfig};
use ohos_hilog_binding::{hilog_error, hilog_info, set_global_options, LogOptions};
use openharmony_ability::xcomponent::{TouchEvent, TouchEventData, TouchPointData, TouchPointTool};
use openharmony_ability::{InputEvent, OpenHarmonyApp};
use openharmony_ability_derive::ability;
use pdfcraft_ui_egui::PdfCraftApp;
use winit::platform::ohos::{EventLoopBuilderExtOhos, OhosInputForwarder, OhosPump};

/// hilog tag for every entry-crate line (evidence protocol:
/// `hdc shell hilog | grep OHOS_SPIKE` — the same tag the fork's pump
/// instrumentation (`PumpMetrics`, task 2.3) writes under, so one grep
/// collects both channels).
const HILOG_TAG: &str = "OHOS_SPIKE";

/// The title handed to `eframe::run_native` (mirrors main.rs, so the window
/// identity is unchanged between the desktop and ohos faces).
const APP_TITLE: &str = "PdfCraft";

/// Process-global input forwarder slot: the napi export below runs on the
/// ArkTS main thread with no other route to the pump, which lives inside
/// eframe's event loop on the worker thread — this slot is the bridge.
/// Filled once from the ability entry before the pump crosses into the
/// worker; a relaunch's second set is logged and dropped (first-install-wins,
/// the same topology the ability crate's `run_loop` latch uses).
static INPUT_FORWARDER: OnceLock<OhosInputForwarder> = OnceLock::new();

/// The vp→px multiplier captured at entry from `OpenHarmonyApp::scale()` —
/// the same display-manager read (`default_display_scaled_density`) the
/// fork's `scale_factor()` (and thus eframe's `pixels_per_point`) uses. See
/// [`effective_touch_scale`] for the conversion contract and the degrade
/// semantics.
static INPUT_SCALE: OnceLock<f32> = OnceLock::new();

/// The embedded sample PDF (design D6): a content-rich two-page vector
/// fixture, opened at construction so the first screen proves the real
/// hayro→vello_cpu→texture pipeline with visible ink (SC3 first-screen
/// evidence), not just an empty shell. (The earlier choice — the ui-egui
/// form-field test PDF — renders a legitimately blank page: its only page
/// has no content stream and its annotation appearance streams are empty,
/// so it cannot carry visual evidence even though it opens fine.)
const SAMPLE_PDF: &[u8] = include_bytes!("../tests/data/sample-content.pdf");

/// Ability entry (pump producer side): install the event pump synchronously
/// — openharmony-ability does not queue events, so the relay must be
/// registered before the first lifecycle event can arrive — then hand the
/// app handle and the pump to the worker thread that owns the winit event
/// loop and EGL (the worker is this process's `run_native` thread for its
/// whole lifetime, satisfying eframe's thread-local event-loop reuse).
#[ability]
fn on_ability_event(app: OpenHarmonyApp) {
    set_global_options(LogOptions { domain: 0, tag: HILOG_TAG });
    // Bridge the `log` crate into hilog: eframe, egui-winit and the rfd-ohos
    // stub diagnose through `log`, which without a logger is silent on
    // device. Max level Trace so the fork's trace-level lines (forwarded by
    // `LogForwarder` below) surface too.
    init_once(HilogLogConfig::default().with_tag(HILOG_TAG).with_max_level(log::LevelFilter::Trace));
    // Surface the fork's `tracing` diagnostics (pump instrumentation, task
    // 2.3): the fork depends on `tracing` without the `log` feature and has
    // no subscriber on device, so its pump lines vanish without this
    // forwarder. Process-global; the entry runs once per process, and a
    // second install (ability relaunch in one process) just logs and moves
    // on.
    if tracing::subscriber::set_global_default(LogForwarder).is_err() {
        hilog_error!("[entry] tracing global subscriber already set");
    }
    let pump = OhosPump::<eframe::UserEvent>::install(&app);
    // Take the forwarder before the pump crosses into the worker thread
    // (`with_ohos_pump` consumes it): the ArkTS touch forward below is the
    // only input path this layout has (the NDK callbacks are structurally
    // unreachable).
    if INPUT_FORWARDER.set(pump.input_forwarder()).is_err() {
        hilog_error!("[entry] input forwarder slot already set — keeping the first");
    }
    // Capture the vp→px multiplier alongside the forwarder (see `INPUT_SCALE`
    // for why this exact source): the entry log line doubles as the on-device
    // confirmation of the scale the fork's `scale_factor` reports.
    let scale = app.scale();
    if INPUT_SCALE.set(scale).is_err() {
        hilog_error!("[entry] input scale slot already set — keeping the first");
    }
    hilog_info!("[entry] pdfcraft-ohos init; pump installed; input scale {scale}");
    let worker = thread::Builder::new().name("pdfcraft-worker".to_owned()).spawn({
        let app = app.clone();
        move || {
            // A plain worker thread, not an FFI boundary, but a panic
            // escaping `run_native` would otherwise die with the thread
            // and leave only the default hook's line; catch it so hilog
            // carries the diagnosis.
            let outcome = catch_unwind(AssertUnwindSafe(|| run_eframe(app, pump)));
            if let Err(payload) = outcome {
                let detail = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("<non-string panic payload>");
                hilog_error!(format!("[worker] eframe PANIC {detail}").as_str());
            }
        }
    });
    if worker.is_err() {
        hilog_error!("[entry] worker spawn failed; eframe will not run");
    }
}

/// ArkTS touch forward: the NDK XComponent touch callback is structurally
/// unreachable in the node-mode layout, so Index.ets forwards every touch
/// ArkUI dispatches to the page through this export into the fork's pump
/// channel — the same `Distilled::Input` slot the relay would have used.
///
/// Coordinates arrive as `displayX/Y` in vp in parallel arrays — one entry
/// per touch point — and are converted to px here with the entry-captured
/// scale ([`effective_touch_scale`]/[`vp_to_px`]): px = vp × scale is the
/// pump channel's coordinate contract. Down/Up events carry the changed
/// point, Move/Cancel all active points (the fork translator maps Down/Up
/// from the primary fields, Move/Cancel per touch point). `event_type` is
/// the ArkTS `TouchType` value (0 Down, 1 Up, 2 Move, 3 Cancel — SDK
/// enums.d.ts).
///
/// napi-boundary discipline, relay-style: no panics — empty or mismatched
/// arrays and unknown types are logged early returns (a panic crossing the
/// napi boundary is process-fatal).
#[napi_derive_ohos::napi]
pub fn notify_touch_event(event_type: i32, ids: Vec<i32>, xs: Vec<f64>, ys: Vec<f64>, forces: Vec<f64>, device_id: i64) {
    let Some(forwarder) = INPUT_FORWARDER.get() else {
        hilog_error!("[forward] no input forwarder installed — dropping touch");
        return;
    };
    if xs.is_empty() || ids.len() != xs.len() || xs.len() != ys.len() || xs.len() != forces.len() {
        hilog_error!(format!(
            "[forward] malformed touch arrays (ids={} xs={} ys={} forces={}) — dropping",
            ids.len(),
            xs.len(),
            ys.len(),
            forces.len()
        )
        .as_str());
        return;
    }
    let event_type = match event_type {
        0 => TouchEvent::Down,
        1 => TouchEvent::Up,
        2 => TouchEvent::Move,
        3 => TouchEvent::Cancel,
        other => {
            hilog_error!(format!("[forward] unknown TouchType {other} — dropping").as_str());
            return;
        }
    };
    let scale64 = effective_touch_scale(INPUT_SCALE.get().copied());
    // Down/Up trace only (vp → px side by side): a per-Move line would flood
    // hilog at ArkUI's ~60 Hz dispatch rate.
    if matches!(event_type, TouchEvent::Down | TouchEvent::Up) {
        hilog_info!(format!(
            "[forward] touch type={event_type:?} n={} first=({:.1}, {:.1}) vp -> ({:.1}, {:.1}) px",
            ids.len(),
            xs[0],
            ys[0],
            xs[0] * scale64,
            ys[0] * scale64
        )
        .as_str());
    }
    // Per-point fields the fork translator consumes: id, x, y, force — plus
    // the NDK-shaped bookkeeping. `timestamp: 0` follows the
    // `notify_key_event` precedent (no consumer reads it today). The primary
    // (event-level) fields mirror point 0: Down/Up translate from them.
    let pressed = !matches!(event_type, TouchEvent::Up);
    let touch_points: Vec<TouchPointData> = ids
        .iter()
        .zip(xs.iter().zip(ys.iter().zip(forces.iter())))
        .map(|(&id, (&x, (&y, &force)))| TouchPointData {
            id,
            screen_x: vp_to_px(x, scale64),
            screen_y: vp_to_px(y, scale64),
            x: vp_to_px(x, scale64),
            y: vp_to_px(y, scale64),
            event_type,
            size: 0.0,
            force: force as f32,
            timestamp: 0,
            is_pressed: pressed,
            event_tool_type: TouchPointTool::Unknown,
        })
        .collect();
    let data = TouchEventData {
        id: ids[0],
        screen_x: vp_to_px(xs[0], scale64),
        screen_y: vp_to_px(ys[0], scale64),
        x: vp_to_px(xs[0], scale64),
        y: vp_to_px(ys[0], scale64),
        event_type,
        size: 0.0,
        force: forces[0] as f32,
        device_id,
        timestamp: 0,
        touch_points,
        num_points: ids.len() as u32,
    };
    forwarder.forward_input(InputEvent::TouchEvent(data));
}

/// The vp→px multiplier a touch forward actually uses: the entry-captured
/// scale, or the 1.0 vp-passthrough degrade for a missing / non-positive /
/// non-finite capture (wrong place, never a crash). ArkTS `TouchObject` is
/// all-vp, `displayX` included (p1 G8 probe: 1560 px → 821.05 vp, density
/// exactly 1.9), so px = vp × scale is by construction the coordinate
/// system `pixels_per_point` divides back down — the finger and the pointer
/// land on the same point.
fn effective_touch_scale(captured: Option<f32>) -> f64 {
    f64::from(captured.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0))
}

/// One coordinate's vp→px conversion — the pump channel's coordinate
/// contract. The f64→f32 cast saturates to inf on pathological input
/// (defined FP semantics — no panic, no UB); coordinates are not clamped
/// here — the fork translator clamps force only — so a non-finite vp flows
/// on as-is, and the deliberate degrade boundary is the scale side
/// (effective_touch_scale).
fn vp_to_px(vp: f64, scale: f64) -> f32 {
    (vp * scale) as f32
}

/// The glow retry decision (design D13): main.rs's
/// `Err(e @ eframe::Error::Wgpu(_)) if retry_with_gl(...)` arm with the
/// ohos-constant retry condition — no `PDFCRAFT_RENDERER` env exists on
/// device, so the decision reduces to "was the failure wgpu". The caller
/// distills `matches!(result, Err(eframe::Error::Wgpu(_)))` into the bool;
/// a wgpu failure retries once with glow, anything else is reported as-is.
fn glow_retry_renderer(wgpu_failed: bool) -> Option<eframe::Renderer> {
    wgpu_failed.then_some(eframe::Renderer::Glow)
}

/// Builds the [`eframe::NativeOptions`] with the OHOS injections (design
/// D5): the `event_loop_builder` hook — the fork's `with_openharmony_app` +
/// `with_ohos_pump` land in the `PlatformSpecificEventLoopAttributes`
/// before the fork's `EventLoop::new` validates them, so eframe itself
/// stays unmodified — and the wgpu instance backends pinned to GL (wgpu
/// 30's only OHOS surface path is GLES; the default `PRIMARY | GL` set
/// would also enumerate Vulkan, whose surface creation has no OHOS path).
/// `display_handle` stays `None`: eframe injects the event loop's owned
/// display handle into the setup, which is how the fork's
/// `OhosDisplayHandle` reaches wgpu-hal's EGL. The renderer is a parameter
/// (the first launch passes `Renderer::Wgpu`); the glow retry leg builds
/// its options separately — see [`glow_retry_options`].
fn native_options(app: OpenHarmonyApp, pump: OhosPump<eframe::UserEvent>, renderer: eframe::Renderer) -> eframe::NativeOptions {
    let mut options = eframe::NativeOptions {
        event_loop_builder: Some(Box::new(move |builder| {
            builder.with_openharmony_app(app).with_ohos_pump(pump);
        })),
        renderer,
        ..Default::default()
    };
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create_new) = &mut options.wgpu_options.wgpu_setup {
        create_new.instance_descriptor.backends = eframe::wgpu::Backends::GL;
    }
    options
}

/// Constructs the payload with the embedded sample PDF open (design D6):
/// `PdfCraftApp::new()` then `open_bytes` over [`SAMPLE_PDF`] — the same
/// in-memory open path the desktop app uses for CLI-passed files, so the
/// first screen proves the hayro→vello_cpu→texture pipeline. A
/// construction-time open failure is logged and the app continues empty:
/// the shell still renders, and the error line is the diagnosis.
fn new_app_with_sample_pdf() -> PdfCraftApp {
    let mut app = PdfCraftApp::new();
    if let Err(error) = app.open_bytes("sample-content.pdf", None, SAMPLE_PDF.to_vec()) {
        hilog_error!(format!("[worker] embedded sample PDF failed to open: {error}").as_str());
    }
    app
}

/// Options for the glow retry leg (design D13): the renderer and nothing
/// else — no `event_loop_builder` hook. eframe's default `run_and_return`
/// path parks the event loop in a thread-local slot and reuses it across
/// `run_native` calls (this is the same mechanism pdfcraft main.rs's desktop
/// glow fallback rides), so the retry runs over the *first* event loop —
/// the one holding the ohos app handle and pump — and the first pump keeps
/// serving input (`INPUT_FORWARDER` stays valid). A second hook could never
/// fire anyway: `OhosPump::install` is process-single-shot (the ability
/// `run_loop` latch is first-install-wins), so a second pump is not a thing
/// this process can produce.
fn glow_retry_options(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions { renderer, ..Default::default() }
}

/// Worker body: launch with wgpu, and on a wgpu startup failure retry once
/// with glow (design D13 — main.rs's fallback with the ohos-constant retry
/// condition; both renderers are p1-proven on device, so glow is a genuine
/// second leg, not a placeholder).
fn run_eframe(app: OpenHarmonyApp, pump: OhosPump<eframe::UserEvent>) {
    hilog_info!("[worker] renderer wgpu — entering eframe::run_native");
    let result =
        eframe::run_native(APP_TITLE, native_options(app, pump, eframe::Renderer::Wgpu), Box::new(|_cc| Ok(Box::new(new_app_with_sample_pdf()))));
    match glow_retry_renderer(matches!(result, Err(eframe::Error::Wgpu(_)))) {
        Some(renderer) => {
            // `glow_retry_renderer` only returns Some over a wgpu failure,
            // so this report always carries the underlying error.
            report_run(&result);
            hilog_info!("[worker] retrying with glow");
            let retry = eframe::run_native(APP_TITLE, glow_retry_options(renderer), Box::new(|_cc| Ok(Box::new(new_app_with_sample_pdf()))));
            report_run(&retry);
        }
        None => report_run(&result),
    }
}

/// Logs the terminal outcome of an `eframe::run_native` run (both the first
/// and the retry leg).
fn report_run(result: &Result<(), eframe::Error>) {
    match result {
        Ok(()) => hilog_info!("[worker] eframe exited cleanly"),
        Err(error) => hilog_error!(format!("[worker] eframe error: {error}").as_str()),
    }
}

/// Minimal `tracing` subscriber forwarding events into the `log` crate: the
/// fork's pump instrumentation (task 2.3) diagnoses through `tracing`, which
/// is silent without a subscriber; the forwarded lines ride the existing
/// log→hilog bridge. Hand-rolled instead of `tracing-subscriber` (whose
/// filter/prelude stack is oversized for a HAP), spans untracked (the fork
/// emits events only), and forwarding is faithful: levels map `TRACE..ERROR`
/// onto `log::trace!..error!`, and the observation criterion is "the line
/// appears", not field completeness.
struct LogForwarder;

impl tracing::Subscriber for LogForwarder {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        // Forward everything: the fork's event volume is low by construction.
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        // Spans are never entered by the fork; a constant non-zero id keeps
        // the (unused) span bookkeeping happy.
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {
        // No span bookkeeping: nothing to record into.
    }

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {
        // Causal links between untracked spans carry no meaning here.
    }

    fn event(&self, event: &tracing::Event<'_>) {
        let mut visitor = MessageVisitor { message: None };
        event.record(&mut visitor);
        let message = visitor.message.unwrap_or_default();
        let meta = event.metadata();
        let target = meta.target();
        match *meta.level() {
            tracing::Level::TRACE => log::trace!(target: target, "{message}"),
            tracing::Level::DEBUG => log::debug!(target: target, "{message}"),
            tracing::Level::INFO => log::info!(target: target, "{message}"),
            tracing::Level::WARN => log::warn!(target: target, "{message}"),
            tracing::Level::ERROR => log::error!(target: target, "{message}"),
        }
    }

    fn enter(&self, _span: &tracing::span::Id) {
        // No span bookkeeping.
    }

    fn exit(&self, _span: &tracing::span::Id) {
        // No span bookkeeping.
    }
}

/// Field visitor extracting an event's `message` field (the human-readable
/// part of a `tracing` event).
struct MessageVisitor {
    /// The recorded message, if the event carried one.
    message: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use openharmony_ability::OpenHarmonyApp;
    use pdfcraft_ui_egui::PdfCraftApp;
    use winit::platform::ohos::OhosPump;

    /// UT for the launch options pins (design D5/D13): the ohos injections —
    /// the event-loop builder hook (app handle + pump), the wgpu instance
    /// backends pinned to GL (GLES is wgpu 30's only OHOS surface path), and
    /// the primary renderer wgpu.
    /// # Brief
    /// 1. `native_options` over a pure-memory app + pump, renderer wgpu.
    /// 2. `event_loop_builder` is `Some`, the wgpu setup stays `CreateNew`
    ///    with `backends == Backends::GL`, and `renderer == Wgpu`.
    #[test]
    fn native_options_pins_wgpu_gl_and_event_loop_builder() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<eframe::UserEvent>::install(&app);
        let options = super::native_options(app, pump, eframe::Renderer::Wgpu);
        assert!(options.event_loop_builder.is_some(), "event_loop_builder hook must be installed");
        assert_eq!(options.renderer, eframe::Renderer::Wgpu);
        let eframe::egui_wgpu::WgpuSetup::CreateNew(create_new) = &options.wgpu_options.wgpu_setup else {
            panic!("native_options must keep the default CreateNew wgpu setup");
        };
        assert_eq!(create_new.instance_descriptor.backends, eframe::wgpu::Backends::GL);
    }

    /// UT for the glow retry decision (design D13) — pdfcraft main.rs's
    /// `Err(e @ eframe::Error::Wgpu(_)) if retry_with_gl(...)` arm with the
    /// ohos-constant retry condition (no `PDFCRAFT_RENDERER` env on device).
    /// # Brief
    /// 1. A wgpu startup failure (`Err(Wgpu(_))`, distilled at the call site
    ///    into the decision input) retries once with glow.
    /// 2. Any other failure is reported, not retried (`None`).
    /// 3. The retry's options carry `Renderer::Glow`.
    #[test]
    fn renderer_falls_back_to_glow_on_wgpu_error() {
        assert_eq!(super::glow_retry_renderer(true), Some(eframe::Renderer::Glow));
        assert_eq!(super::glow_retry_renderer(false), None);
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<eframe::UserEvent>::install(&app);
        let options = super::native_options(app, pump, eframe::Renderer::Glow);
        assert_eq!(options.renderer, eframe::Renderer::Glow);
    }

    /// UT for the embedded sample PDF (design D6): the construction-time open
    /// path — `PdfCraftApp::new()` then `open_bytes` over the embedded
    /// fixture — succeeds on device (the hayro parse pipeline holds).
    /// # Brief
    /// 1. `PdfCraftApp::new()` builds.
    /// 2. `open_bytes("sample-content.pdf", None, SAMPLE_PDF)` returns `Ok(())`.
    #[test]
    fn sample_pdf_opens_in_app() {
        let mut app = PdfCraftApp::new();
        app.open_bytes("sample-content.pdf", None, super::SAMPLE_PDF.to_vec()).expect("the embedded sample PDF must open on device");
    }

    /// UT for the napi touch-forward guards (frame-spike discipline): a panic
    /// crossing the napi boundary is process-fatal, so malformed input is an
    /// early return, never a panic.
    /// # Brief
    /// 1. A forwarder is installed first, so the array/type guards — not the
    ///    no-forwarder guard — are the rejection path under test.
    /// 2. Empty arrays, mismatched array lengths and an unknown `event_type`
    ///    all return early: no panic, nothing forwarded.
    #[test]
    fn notify_touch_event_rejects_malformed_input() {
        let app = OpenHarmonyApp::new();
        let pump = OhosPump::<eframe::UserEvent>::install(&app);
        let _ = super::INPUT_FORWARDER.set(pump.input_forwarder());
        super::notify_touch_event(0, vec![], vec![], vec![], vec![], 0);
        super::notify_touch_event(0, vec![1], vec![1.0, 2.0], vec![1.0], vec![1.0], 0);
        super::notify_touch_event(99, vec![1], vec![1.0], vec![1.0], vec![1.0], 0);
    }

    /// UT for the vp→px conversion (frame-spike guard semantics; 1.9 = the
    /// p1 device truth, DECISIONS G8): the captured scale multiplies vp into
    /// px; a missing, non-positive or non-finite capture degrades to 1.0
    /// (vp passthrough — wrong place, never a crash).
    /// # Brief
    /// 1. Captured 1.9 and 2.0 convert representative vp coordinates to
    ///    `vp × scale` px, value by value.
    /// 2. Missing / 0 / negative / NaN / inf captures all degrade to 1.0.
    #[test]
    fn touch_coordinates_scale_from_vp_to_px() {
        let scale_19 = f64::from(1.9_f32);
        let scale_20 = f64::from(2.0_f32);
        assert_eq!(super::effective_touch_scale(Some(1.9)), scale_19);
        assert_eq!(super::effective_touch_scale(Some(2.0)), scale_20);
        for (vp, scale) in [(821.05, scale_19), (100.0, scale_19), (100.0, scale_20)] {
            assert_eq!(super::vp_to_px(vp, scale), (vp * scale) as f32);
        }
        assert_eq!(super::effective_touch_scale(None), 1.0);
        assert_eq!(super::effective_touch_scale(Some(0.0)), 1.0);
        assert_eq!(super::effective_touch_scale(Some(-1.9)), 1.0);
        assert_eq!(super::effective_touch_scale(Some(f32::NAN)), 1.0);
        assert_eq!(super::effective_touch_scale(Some(f32::INFINITY)), 1.0);
        assert_eq!(super::vp_to_px(100.0, 1.0), 100.0);
    }
}
