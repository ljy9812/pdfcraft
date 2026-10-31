//! Multi-display enumeration for the windowing backend (issue
//! Eulogizethesun/tauri#106).
//!
//! Two data sources are merged:
//!
//! 1. **Display list** (id/name/size/refresh/scale) from the NDK C API
//!    `OH_NativeDisplayManager_CreateAllDisplays` (`libnative_display_manager.so`,
//!    @since 14). compatibleSdkVersion is API 12, so the symbols are resolved
//!    lazily via dlopen+dlsym (same pattern as the cursor-grab C API in
//!    [`crate::window`]) — a load-time link would keep the app from starting on
//!    API 12/13 devices. Symbol absence ⇒ single-display fallback from the
//!    default-display queries.
//!
//! 2. **Positions** (x/y) from the ArkTS `display.getAllDisplays()` +
//!    `Display.x`/`Display.y` (@since 19, optional fields — `undefined` below
//!    API 19). The C `NativeDisplayManager_DisplayInfo` struct has NO x/y
//!    fields, so positions can only come from ArkTS. `NativeAbility.ets`
//!    pushes the whole layout via the `set_display_layout` NAPI function at
//!    window-stage creation and on every `display.on('add'|'remove'|'change')`
//!    event; a missing/absent entry degrades to (0,0).

use std::ffi::c_char;
use std::sync::{Mutex, OnceLock};

use napi_derive_ohos::napi;
use ohos_display_binding::{
    default_display_height, default_display_id, default_display_refresh_rate,
    default_display_scaled_density, default_display_width,
};

// ─── FFI (api-14 layout of libnative_display_manager.so) ────────────────────
//
// Layout copied verbatim from the `ohos-display-sys` 0.1.3 api-14 bindings —
// the struct is @since 14 and stable from then on, and the array returned by
// `OH_NativeDisplayManager_CreateAllDisplays` is indexed by this layout, so
// every field (including the trailing hdr/colorSpace pointers we never read)
// must be present.

#[repr(C)]
#[allow(non_snake_case)]
struct NativeDisplayManager_DisplayHdrFormat {
    hdrFormatLength: u32,
    hdrFormats: *mut u32,
}

#[repr(C)]
#[allow(non_snake_case)]
struct NativeDisplayManager_DisplayColorSpace {
    colorSpaceLength: u32,
    colorSpaces: *mut u32,
}

#[repr(C)]
#[allow(non_snake_case)]
struct NativeDisplayManager_DisplayInfo {
    id: u32,
    name: [c_char; 33],
    isAlive: bool,
    width: i32,
    height: i32,
    physicalWidth: i32,
    physicalHeight: i32,
    refreshRate: u32,
    availableWidth: u32,
    availableHeight: u32,
    densityDPI: f32,
    densityPixels: f32,
    scaledDensity: f32,
    xDPI: f32,
    yDPI: f32,
    rotation: u32,
    state: u32,
    orientation: u32,
    hdrFormat: *mut NativeDisplayManager_DisplayHdrFormat,
    colorSpace: *mut NativeDisplayManager_DisplayColorSpace,
}

#[repr(C)]
#[allow(non_snake_case)]
struct NativeDisplayManager_DisplaysInfo {
    displaysLength: u32,
    displaysInfo: *mut NativeDisplayManager_DisplayInfo,
}

type CreateAllDisplaysFn = unsafe extern "C" fn(*mut *mut NativeDisplayManager_DisplaysInfo) -> u32;
type DestroyAllDisplaysFn = unsafe extern "C" fn(*mut NativeDisplayManager_DisplaysInfo);

struct AllDisplaysApi {
    create_all_displays: CreateAllDisplaysFn,
    destroy_all_displays: DestroyAllDisplaysFn,
}

extern "C" {
    fn dlopen(filename: *const c_char, flags: std::ffi::c_int) -> *mut std::ffi::c_void;
    fn dlsym(handle: *mut std::ffi::c_void, symbol: *const c_char) -> *mut std::ffi::c_void;
}

static ALL_DISPLAYS_API: OnceLock<Option<AllDisplaysApi>> = OnceLock::new();

/// Resolve `OH_NativeDisplayManager_CreateAllDisplays`/`DestroyAllDisplays`
/// once per process; `None` on API < 14 (symbol missing). The handle is
/// intentionally never closed — the library stays loaded for the process
/// lifetime (same as [`crate::window`]'s cursor-grab resolver).
fn all_displays_api() -> Option<&'static AllDisplaysApi> {
    ALL_DISPLAYS_API
        .get_or_init(|| unsafe {
            // RTLD_NOW | RTLD_LOCAL = 2 on OHOS musl.
            let handle = dlopen(
                b"libnative_display_manager.so\0".as_ptr() as *const c_char,
                2,
            );
            if handle.is_null() {
                crate::warn!(
                    "[ohos-display] dlopen libnative_display_manager.so failed — \
                     single-display fallback"
                );
                return None;
            }
            let create = dlsym(
                handle,
                b"OH_NativeDisplayManager_CreateAllDisplays\0".as_ptr() as *const c_char,
            );
            let destroy = dlsym(
                handle,
                b"OH_NativeDisplayManager_DestroyAllDisplays\0".as_ptr() as *const c_char,
            );
            if create.is_null() || destroy.is_null() {
                crate::warn!(
                    "[ohos-display] OH_NativeDisplayManager_CreateAllDisplays not exported \
                     (API < 14?) — single-display fallback"
                );
                return None;
            }
            Some(AllDisplaysApi {
                create_all_displays: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    CreateAllDisplaysFn,
                >(create),
                destroy_all_displays: std::mem::transmute::<
                    *mut std::ffi::c_void,
                    DestroyAllDisplaysFn,
                >(destroy),
            })
        })
        .as_ref()
}

// ─── Position overlay (pushed from ArkTS) ───────────────────────────────────

/// Display positions (id, x, y) in physical pixels, replaced wholesale by
/// `set_display_layout`. `Vec::new()` is const-constructible so a plain
/// `Mutex` static works (same as `PENDING_WINDOW_STATUS`).
static DISPLAY_POSITIONS: Mutex<Vec<(u32, i32, i32)>> = Mutex::new(Vec::new());

/// NAPI function called from ArkTS with the full display layout as JSON
/// (`[[id, x, y], ...]`, physical px). Pushed at window-stage creation and on
/// `display.on('add'|'remove'|'change')`. Replaces the previous layout so
/// unplugged displays disappear. `x`/`y` are 0 when the device is below API 19
/// (`Display.x`/`y` undefined) — no worse than the pre-fix constant (0,0).
#[napi]
pub fn set_display_layout(layout_json: String) {
    let layout: Vec<(u32, i32, i32)> = match serde_json::from_str(&layout_json) {
        Ok(layout) => layout,
        Err(e) => {
            crate::warn!("[ohos-display] set_display_layout parse failed: {e}");
            return;
        }
    };
    match DISPLAY_POSITIONS.lock() {
        Ok(mut positions) => *positions = layout,
        Err(poisoned) => *poisoned.into_inner() = layout,
    }
}

fn position_of(display_id: u32) -> Option<(i32, i32)> {
    DISPLAY_POSITIONS.lock().ok().and_then(|positions| {
        positions
            .iter()
            .find(|(id, _, _)| *id == display_id)
            .map(|&(_, x, y)| (x, y))
    })
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// One display snapshot, consumed by tao's `MonitorHandle` accessors.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplaySnapshot {
    /// OHOS display id (`OH_NativeDisplayManager_GetDefaultDisplayId` domain).
    pub id: u32,
    pub name: String,
    /// Top-left offset in the global display coordinate space (ArkTS
    /// `Display.x`/`y`; (0,0) when unknown — API < 19 or not yet pushed).
    pub x: i32,
    pub y: i32,
    /// Physical size in pixels.
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    /// `scaledDensity` — the same source `OpenHarmonyApp::scale()` uses for
    /// the default display.
    pub scale: f64,
}

/// Enumerate all displays. Falls back to a single (default) display snapshot
/// when the C API is unavailable (API < 14) or returns nothing; the fallback
/// can still carry an ArkTS-pushed position for the default display.
pub fn all_displays() -> Vec<DisplaySnapshot> {
    if let Some(api) = all_displays_api() {
        let mut info_ptr: *mut NativeDisplayManager_DisplaysInfo = std::ptr::null_mut();
        let ret = unsafe { (api.create_all_displays)(&mut info_ptr) };
        if ret != 0 {
            crate::warn!("[ohos-display] CreateAllDisplays failed with code {ret}");
            return default_display_fallback();
        }
        if info_ptr.is_null() {
            return default_display_fallback();
        }
        let snapshots = unsafe {
            let info = &*info_ptr;
            let len = info.displaysLength as usize;
            let base = info.displaysInfo;
            if base.is_null() || len == 0 {
                Vec::new()
            } else {
                (0..len)
                    .map(|i| {
                        let raw = &*base.add(i);
                        let (x, y) = position_of(raw.id).unwrap_or((0, 0));
                        DisplaySnapshot {
                            id: raw.id,
                            name: c_name_to_string(&raw.name),
                            x,
                            y,
                            width: raw.width.max(0) as u32,
                            height: raw.height.max(0) as u32,
                            refresh_rate: raw.refreshRate,
                            scale: raw.scaledDensity as f64,
                        }
                    })
                    .collect()
            }
        };
        unsafe { (api.destroy_all_displays)(info_ptr) };
        if snapshots.is_empty() {
            default_display_fallback()
        } else {
            snapshots
        }
    } else {
        default_display_fallback()
    }
}

/// Single-snapshot fallback built from the default-display queries (API 12).
/// Width/height may be 0 when even those fail — tao's `MonitorHandle::size()`
/// then degrades to the content-rect fallback (pre-fix behavior).
fn default_display_fallback() -> Vec<DisplaySnapshot> {
    let id = default_display_id() as u32;
    let (x, y) = position_of(id).unwrap_or((0, 0));
    vec![DisplaySnapshot {
        id,
        name: "OpenHarmony Device".to_string(),
        x,
        y,
        width: default_display_width().max(0) as u32,
        height: default_display_height().max(0) as u32,
        refresh_rate: default_display_refresh_rate(),
        scale: default_display_scaled_density() as f64,
    }]
}

/// The default (primary) display id.
pub fn default_display() -> u32 {
    default_display_id() as u32
}

/// Read the fixed-size C char array as a Rust String (NUL-terminated).
fn c_name_to_string(raw: &[c_char]) -> String {
    let bytes: Vec<u8> = raw
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
