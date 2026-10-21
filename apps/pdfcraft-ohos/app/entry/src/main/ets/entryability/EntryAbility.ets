import { NativeAbility } from "@ohos-rs/ability";
import bundleManager from "@ohos.bundle.bundleManager";
import hilog from "@ohos.hilog";
import window from "@ohos.window";

// The single native module of this app: `libpdfcraft_ohos.so` (built from the
// pdfcraft-ohos crate). NativeModuleLoader resolves this name to
// `libpdfcraft_ohos.so` and checks the #[ability]-generated Module contract.
const NATIVE_MODULES = ["pdfcraft_ohos"];

// hilog tag aligned with the Rust side (HILOG_TAG in pdfcraft-ohos/src/lib.rs
// and the winit fork's pump instrumentation): one
// `hdc shell hilog | grep OHOS_SPIKE` covers the whole reachability evidence
// trail, ArkTS shell lines included (ported from frame-spike, design D10).
const LOG_TAG = "OHOS_SPIKE";

export default class EntryAbility extends NativeAbility {
  public moduleName: string[] = NATIVE_MODULES;
  public defaultPage: boolean = true;

  // Tauri-shell ordering (verified on-device 2026-10-15, frame-spike): apply
  // the full-screen layout BEFORE super's content loading. Awaiting here does
  // NOT stall loadContent on PC free windows (the vendored checkout's
  // warning about SetUIContent timeouts applies to the fire-and-forget
  // variant racing loadContent, not to this ordering).
  async onWindowStageCreate(windowStage: window.WindowStage): Promise<void> {
    const win = windowStage.getMainWindowSync();
    // Narrow the supported modes to FULL_SCREEN at runtime too — layered on
    // module.json5 supportWindowMode so the restriction also applies where
    // the declarative list is not honored. WindowStage method (@since 15,
    // Promise); failure is observed, not fatal — the fullscreen layout below
    // still applies either way.
    try {
      await windowStage.setSupportedWindowModes([bundleManager.SupportWindowMode.FULL_SCREEN]);
      hilog.info(0x0, LOG_TAG, "[entry] E2 setSupportedWindowModes([FULL_SCREEN]) ok");
    } catch (error) {
      hilog.error(0x0, LOG_TAG, `[entry] E2 setSupportedWindowModes failed: ${String(error)}`);
    }
    // The free-window dead-zone diagnosis axis (frame-spike p4): record
    // whether this launch runs in free window mode. @since 22: guard with
    // try/catch so an older device firmware degrades to a warning instead of
    // breaking the startup chain.
    try {
      hilog.info(0x0, LOG_TAG, `[entry] isInFreeWindowMode = ${win.isInFreeWindowMode()}`);
    } catch (error) {
      hilog.warn(0x0, LOG_TAG, `[entry] isInFreeWindowMode unavailable: ${String(error)}`);
    }
    await win.setWindowLayoutFullScreen(true);
    await super.onWindowStageCreate(windowStage);
  }

  protected override async loadWindowStageContent(windowStage: window.WindowStage): Promise<void> {
    try {
      await windowStage.loadContent("pages/Index");
    } catch (error) {
      throw new Error(`Unable to load PdfCraft WindowStage content: ${String(error)}`);
    }
  }
}
