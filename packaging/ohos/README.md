| Saving | `a[download]` clicks — attached (document listener) and programmatic/detached (`HTMLAnchorElement.prototype.click` override) — are intercepted by a hook installed in `onPageEnd`; the blob is fetched, base64-chunked over a `MessageChannel` port to ArkTS, and written through `DocumentSavePicker` (with a filename field) |
| External links | dual path: `onLoadIntercept` for main-frame http(s) navigations + `multiWindowAccess(true)`/`onWindowNew` for `window.open` popups — both route to the system browser |
| IME input | the shell installs a port receive hook (`hookPortReceive`); typed CJK composition reaches the page's hidden input and the annotation paths render it |
| Loading | the committed `rawfile/web/index.html` carries a loading indicator and SRI-stripped asset refs — after rebuilding the wasm, rewrite its hash refs (a stale hash double-fetches the wasm behind a white window) |

## Build (Windows host, DevEco Studio toolchain)

Prerequisites: DevEco Studio (for `hvigorw`, `ohpm`, and the HarmonyOS SDK),
`hdc` on PATH, and `gh` (GitHub CLI) for the release download.

```powershell
cd packaging\ohos
.\scripts\fetch-web.ps1     # deploy the web build into rawfile
.\scripts\build-hap.ps1     # build the Release HAP and install to the connected device
```

### CJK (Chinese) variant — in this branch

Alongside the shell it carries the earlier phase's native OHOS app (apps/pdfcraft-ohos, Rust/wgpu — build it with the repo's own xtask flow, not fetch-web.ps1) and the CJK text work: Type 3 fallback faces with ToUnicode for text the standard fonts cannot draw (crates/fonts/src/type3.rs — added text and FreeText annotations render CJK correctly) and the Hans interface face whitelisted for the web build (crates/fonts/build.rs).

Self-build recipe (Windows host, Rust stable, `trunk` 0.21.x; pin versions
per the upstream repo — on networks where the wasm-opt download hangs,
build with the optimizer off or assemble the .stage outputs by hand):

```powershell
git clone -b hans-wqy-microhei https://github.com/ljy9812/craft-fonts.git craft-fonts-cjk
$env:CRAFT_FONTS_DIR = "<repo>\craft-fonts-cjk"      # absolute path
cd apps\pdfcraft-web
trunk build --release
# deploy dist\web\ into packaging\ohos\entry\src\main\resources\rawfile\web\:
# swap the wasm+glue pair, copy snippets\ and fonts\ when the build produced
# them, and rewrite index.html's asset hash — a stale hash (or a stale SRI
# integrity attribute) double-fetches the wasm behind a white window.
```

### Signing

The committed tree is deliberately signing-free (`"signingConfigs": []` in
`build-profile.json5`). To build a signed HAP, put your own signing fragment
at `packaging\ohos\local\signing.json` (gitignored) — one `signingConfigs`
entry as DevEco Studio writes it. `build-hap.ps1` injects it for the build
and restores the pristine file afterwards. Adjust `bundleName` in
`AppScope/app.json5` (currently the `com.example.pdfcraft.web` placeholder)
to match your signing material.

## Verified status and known limitations

Verified on a HarmonyOS laptop (HAD-W24, API 23, physical keyboard +
touchscreen): install & launch, first screen, open→edit→save→reopen
round-trip through the system pickers with byte-verified PDF output,
≥3 min continuous interaction, external links to the system browser,
CJK added-text and annotations rendering through the Type 3 fallback
faces, and IME text input through the port receive hook. The native
`apps/pdfcraft-ohos` app (same device) runs the full wgpu GLES
rendering-gate pass — see its own docs for that build.
