import { DefaultXComponent } from "@ohos-rs/ability";
import hilog from "@ohos.hilog";
import { notifyTouchEvent } from "libpdfcraft_ohos.so";

// Must match the module name in EntryAbility (resolved to libpdfcraft_ohos.so).
const MODULE_NAME = "pdfcraft_ohos";
const LOG_TAG = "OHOS_SPIKE";

@Entry
@Component
struct Index {
  // Demo-page fallback (mirrors the vendored demo): this page bypasses the
  // HSP's MainPage, so the bridge session id arrives via AppStorage —
  // written by NativeAbility.onCreate before loadContent.
  @StorageProp("bridgeSessionId") bridgeSessionId: string = "";

  build() {
    Stack({ alignContent: Alignment.TopStart }) {
      DefaultXComponent({ moduleName: MODULE_NAME, bridgeSessionId: this.bridgeSessionId })
        .width("100%")
        .height("100%")
    }
    .width("100%")
    .height("100%")
    // Mirror the HSP MainPage root: extend into the system safe areas
    // (setWindowLayoutFullScreen(true) is applied by EntryAbility).
    .expandSafeArea([SafeAreaType.SYSTEM], [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM])
    // The NDK XComponent touch callback is structurally unreachable in the
    // node-mode layout (frame-spike G8 R2), so every touch ArkUI dispatches
    // to this page is forwarded into the native pump instead. Down/Up carry
    // the changed point; Move/Cancel carry all active points (the Rust
    // translator maps Move/Cancel per point).
    // Coordinates: displayX/Y in vp — every TouchObject flavor is vp
    // (frame-spike G8 probe: 1560 px injected → x = screenX = displayX =
    // 821.05 vp); the native side multiplies by the display scale captured
    // at entry (effective_touch_scale / vp_to_px in pdfcraft-ohos lib.rs) to
    // reach the pump's px contract. The Down/Up-only evidence line pairs
    // x/screenX with displayX/Y — all vp.
    .onTouch((event: TouchEvent) => {
      const down = event.type === TouchType.Down;
      if (down || event.type === TouchType.Up) {
        const t = event.changedTouches[0];
        hilog.info(0x0, LOG_TAG,
          `[index] forward ${event.type} id=${t ? t.id : -1}` +
          ` x=${t ? t.x : -1} screenX=${t ? t.screenX : -1}` +
          ` displayX=${t ? t.displayX : -1} displayY=${t ? t.displayY : -1}`);
      }
      const points = (down || event.type === TouchType.Up)
        ? event.changedTouches : event.touches;
      if (points.length === 0) {
        return;
      }
      // TouchObject (API 24 common.d.ts) exposes no force or deviceId —
      // zeros mean "no pressure data" (the fork translator clamps force to
      // [0,1]) and the primary device.
      notifyTouchEvent(
        event.type as number,
        points.map((p): number => p.id),
        points.map((p): number => p.displayX),
        points.map((p): number => p.displayY),
        points.map((): number => 0),
        0,
      );
    })
  }
}
