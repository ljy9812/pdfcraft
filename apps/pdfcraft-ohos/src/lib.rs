//! OpenHarmony entry crate for PdfCraft (ohos-craft-p2).
//!
//! Host face: empty — the crate is compiled out entirely off the ohos target
//! (crate-level cfg below), so host builds and `cargo test -p pdfcraft-ohos`
//! see an empty crate. The ohos face ports the frame-spike blueprint
//! (ohos-winit p1): `#[ability]` napi entry, hilog/tracing forwarding, the
//! input pump and touch forwarding, and `eframe::run_native` driving
//! `PdfCraftApp` with an embedded sample PDF.

#![cfg(target_env = "ohos")]

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
        assert!(
            options.event_loop_builder.is_some(),
            "event_loop_builder hook must be installed"
        );
        assert_eq!(options.renderer, eframe::Renderer::Wgpu);
        let eframe::egui_wgpu::WgpuSetup::CreateNew(create_new) = &options.wgpu_options.wgpu_setup
        else {
            panic!("native_options must keep the default CreateNew wgpu setup");
        };
        assert_eq!(
            create_new.instance_descriptor.backends,
            eframe::wgpu::Backends::GL
        );
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
        assert_eq!(
            super::glow_retry_renderer(true),
            Some(eframe::Renderer::Glow)
        );
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
    /// 2. `open_bytes("form.pdf", None, SAMPLE_PDF)` returns `Ok(())`.
    #[test]
    fn sample_pdf_opens_in_app() {
        let mut app = PdfCraftApp::new();
        app.open_bytes("form.pdf", None, super::SAMPLE_PDF.to_vec())
            .expect("the embedded sample PDF must open on device");
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
