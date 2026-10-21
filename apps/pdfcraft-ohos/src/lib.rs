//! OpenHarmony entry crate for PdfCraft (ohos-craft-p2).
//!
//! Host face: empty — the crate is compiled out entirely off the ohos target
//! (crate-level cfg below), so host builds and `cargo test -p pdfcraft-ohos`
//! see an empty crate. The ohos face ports the frame-spike blueprint
//! (ohos-winit p1): `#[ability]` napi entry, hilog/tracing forwarding, the
//! input pump and touch forwarding, and `eframe::run_native` driving
//! `PdfCraftApp` with an embedded sample PDF.

#![cfg(target_env = "ohos")]
