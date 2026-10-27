import { webview } from '@kit.ArkWeb';
import { picker, fileIo as fs } from '@kit.CoreFileKit';
import { util } from '@kit.ArkTS';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { common } from '@kit.AbilityKit';

const TAG: string = 'PdfCraft';
const MAIN_URL: string = 'https://local.pdfcraft/index.html';
const URL_PREFIX: string = 'https://local.pdfcraft/';
const RAW_PREFIX: string = 'web/';
const CHUNK_SIZE: number = 262144; // 256 KiB per message port frame (base64)
const URL_PATTERN: string = 'https://local.pdfcraft/*';

interface ExportMsg {
  t: string;
  name?: string;
  data?: string;
}

// Injected before page scripts run:
// 1. receive the MessageChannel port handed over by ArkTS (window.__pc_port)
// 2. intercept <a download href="blob:..."> clicks and stream the blob through
//    the port in chunks. Two interception paths, because the apps differ:
//    - photocraft's wasm attaches its download anchor to the DOM before
//      clicking, so the click bubbles and the document listener (path 1) sees it.
//    - pdfcraft's wasm download() (ui-egui/src/editing.rs) creates the anchor,
//      sets download/href and clicks it WITHOUT appending — the click event of
//      a detached node never propagates to document, so no document listener
//      can see it. Desktop Chrome still runs the download activation for a
//      detached anchor (which is why upstream never noticed); ArkWeb does
//      nothing at all. Path 2 overrides HTMLAnchorElement.prototype.click to
//      catch programmatic clicks on detached (and attached) anchors.
const START_SCRIPT: string = `(function() {
  if (window.__pc_hook) { return; }
  window.__pc_hook = true;
  console.log('[pc] hook installed');
  function pcSend(href, name, src) {
    var port = window.__pc_port;
    console.log('[pc] download click: ' + name + ' port=' + (port ? 'yes' : 'NO') + (src ? ' [' + src + ']' : ''));
    if (!port) { return; }
    fetch(href).then(function(r) { return r.blob(); }).then(function(b) {
      console.log('[pc] blob fetched: ' + b.size + ' B');
      var fr = new FileReader();
      fr.onload = function() {
        var du = String(fr.result);
        var b64 = du.substring(du.indexOf(',') + 1);
        port.postMessage(JSON.stringify({ t: 'begin', name: name, total: Math.ceil(b64.length / 262144) }));
        var i = 0;
        while (i * 262144 < b64.length) {
          port.postMessage(JSON.stringify({ t: 'chunk', i: i, data: b64.substr(i * 262144, 262144) }));
          i++;
        }
        port.postMessage(JSON.stringify({ t: 'end' }));
      };
      fr.onerror = function() {};
      fr.readAsDataURL(b);
    }).catch(function() { console.log('[pc] blob fetch failed'); });
  }
  window.addEventListener('message', function(e) {
    if (e.ports && e.ports.length > 0) {
      window.__pc_port = e.ports[0];
      console.log('[pc] port received');
    }
  });
  // Path 1: real user clicks on attached a[download] anchors.
  document.addEventListener('click', function(e) {
    var t = e.target;
    if (!t || !t.closest) { return; }
    var a = t.closest('a[download]');
    if (!a) { return; }
    var href = a.getAttribute('href') || '';
    if (href.indexOf('blob:') !== 0) { return; }
    e.preventDefault();
    e.stopPropagation();
    pcSend(href, a.getAttribute('download') || 'export.bin', 'doc');
  }, true);
  // Path 2: programmatic .click() — including detached anchors.
  var origClick = HTMLAnchorElement.prototype.click;
  if (!origClick.__pc_patched) {
    var patched = function() {
      try {
        var href = this.getAttribute('href') || '';
        var name = this.getAttribute('download');
        if (name && href.indexOf('blob:') === 0 && window.__pc_port) {
          pcSend(href, name, 'proto');
          return; // swallow: the native activation is useless in ArkWeb anyway
        }
      } catch (e) {}
      return origClick.apply(this, arguments);
    };
    try { Object.defineProperty(patched, '__pc_patched', { value: true }); } catch (e) {}
    HTMLAnchorElement.prototype.click = patched;
  }
})();`; // (the stray ;; from an earlier edit is gone — this line ends START_SCRIPT)

function mimeFor(path: string): string {
  const dot: number = path.lastIndexOf('.');
  if (dot < 0) {
    return 'application/octet-stream';
  }
  switch (path.substring(dot).toLowerCase()) {
    case '.html':
    case '.htm':
      return 'text/html';
    case '.js':
      return 'text/javascript';
    case '.css':
      return 'text/css';
    case '.wasm':
      return 'application/wasm';
    case '.txt':
      return 'text/plain';
    case '.json':
      return 'application/json';
    case '.otf':
      return 'font/otf';
    case '.ttf':
      return 'font/ttf';
    case '.png':
      return 'image/png';
    case '.jpg':
    case '.jpeg':
      return 'image/jpeg';
    case '.svg':
      return 'image/svg+xml';
    case '.ico':
      return 'image/x-icon';
    default:
      return 'application/octet-stream';
  }
}

@Entry
@Component
struct Index {
  controller: webview.WebviewController = new webview.WebviewController();
  private sendPort: webview.WebMessagePort | null = null;
  private exportName: string = '';
  private exportChunks: string[] = [];

  private toast(msg: string): void {
    this.getUIContext().getPromptAction().showToast({ message: msg });
  }

  // F7 fix: the shell only serves the virtual source, so page links to the
  // outside web used to 404 silently (dead click). Hand external navigations
  // to the system browser instead.
  private openExternal(url: string): void {
    try {
      const context = getContext(this) as common.UIAbilityContext;
      context.openLink(url, { appLinkingOnly: false })
        .then(() => hilog.info(0x0000, TAG, 'external link opened: ' + url))
        .catch((e: BusinessError) => hilog.warn(0x0000, TAG, 'openLink failed: ' + e.code + ' ' + url));
    } catch (e) {
      hilog.warn(0x0000, TAG, 'openLink threw: ' + url);
    }
  }

  private notFound(path: string): WebResourceResponse {
    const resp: WebResourceResponse = new WebResourceResponse();
    resp.setResponseData('404 Not Found: ' + path);
    resp.setResponseMimeType('text/plain');
    resp.setResponseEncoding('utf-8');
    resp.setResponseCode(404);
    resp.setReasonMessage('Not Found');
    resp.setResponseIsReady(true);
    return resp;
  }

  // Map https://local.pdfcraft/<path> to rawfile web/<path>.
  private handleRequest(url: string): WebResourceResponse {
    if (!url.startsWith(URL_PREFIX)) {
      return this.notFound(url);
    }
    let path: string = url.substring(URL_PREFIX.length);
    const q: number = path.indexOf('?');
    if (q >= 0) {
      path = path.substring(0, q);
    }
    const h: number = path.indexOf('#');
    if (h >= 0) {
      path = path.substring(0, h);
    }
    if (path.length === 0) {
      path = 'index.html';
    }
    if (path.includes('..') || !/^[A-Za-z0-9._\-/]+$/.test(path)) {
      return this.notFound(path);
    }
    try {
      const data: Uint8Array = getContext(this).resourceManager.getRawFileContentSync(RAW_PREFIX + path);
      const mime: string = mimeFor(path);
      const resp: WebResourceResponse = new WebResourceResponse();
      resp.setResponseData(data.buffer as ArrayBuffer);
      resp.setResponseMimeType(mime);
      if (mime.startsWith('text/') || mime === 'application/json' || mime === 'image/svg+xml') {
        resp.setResponseEncoding('utf-8');
      }
      resp.setResponseCode(200);
      resp.setReasonMessage('OK');
      const hdr: Header = { headerKey: 'Cache-Control', headerValue: 'no-store' };
      resp.setResponseHeader([hdr]);
      resp.setResponseIsReady(true);
      return resp;
    } catch (e) {
      hilog.warn(0x0000, TAG, 'rawfile miss: ' + path);
      return this.notFound(path);
    }
  }

  // PhotoCraft web opens files via <input type=file>; take over with the system picker.
  private pickFiles(event: OnShowFileSelectorEvent): boolean {
    hilog.info(0x0000, TAG, 'file selector requested (multi=' +
      (event.fileSelector.getMode() === FileSelectorMode.FileOpenMultipleMode) + ')');
    const multi: boolean = event.fileSelector.getMode() === FileSelectorMode.FileOpenMultipleMode;
    const opts: picker.DocumentSelectOptions = new picker.DocumentSelectOptions();
    opts.maxSelectNumber = multi ? 10 : 1;
    const dvp: picker.DocumentViewPicker = new picker.DocumentViewPicker(getContext(this));
    dvp.select(opts).then((uris: Array<string>) => {
      event.result.handleFileList(uris);
    }).catch((err: BusinessError) => {
      hilog.warn(0x0000, TAG, 'file picker failed: ' + err.code);
      event.result.handleFileList([]);
    });
    return true;
  }

  private setupPorts(): void {
    const ports: Array<webview.WebMessagePort> = this.controller.createWebMessagePorts();
    const recv: webview.WebMessagePort = ports[1];
    recv.onMessageEvent((msg: webview.WebMessage) => {
      this.onExportMessage(msg);
    });
    this.sendPort = ports[0];
  }

  private onExportMessage(msg: webview.WebMessage): void {
    if (typeof msg !== 'string') {
      return;
    }
    let m: ExportMsg;
    try {
      m = JSON.parse(msg) as ExportMsg;
    } catch (e) {
      return;
    }
    if (m.t === 'begin') {
      this.exportChunks = [];
      this.exportName = m.name ? m.name : 'export.bin';
      hilog.info(0x0000, TAG, 'export begin: ' + this.exportName);
    } else if (m.t === 'chunk') {
      if (m.data) {
        this.exportChunks.push(m.data);
      }
    } else if (m.t === 'end') {
      hilog.info(0x0000, TAG, 'export end: chunks=' + this.exportChunks.length);
      this.saveExport();
    }
  }

  private saveExport(): void {
    const b64: string = this.exportChunks.join('');
    const name: string = this.exportName;
    this.exportChunks = [];
    this.exportName = '';
    if (b64.length === 0) {
      return;
    }
    try {
      const data: Uint8Array = new util.Base64Helper().decodeSync(b64);
      const opts: picker.DocumentSaveOptions = new picker.DocumentSaveOptions();
      opts.newFileNames = [name];
      const dvp: picker.DocumentViewPicker = new picker.DocumentViewPicker(getContext(this));
      dvp.save(opts).then((uris: Array<string>) => {
        if (uris.length === 0) {
          this.toast('保存已取消');
          return;
        }
        hilog.info(0x0000, TAG, 'export write: ' + data.byteLength + ' B -> ' + uris[0]);
        const file: fs.File = fs.openSync(uris[0], fs.OpenMode.READ_WRITE | fs.OpenMode.CREATE | fs.OpenMode.TRUNC);
        try {
          fs.writeSync(file.fd, data.buffer as ArrayBuffer);
        } finally {
          fs.closeSync(file);
        }
        this.toast('已保存 ' + name);
      }).catch((err: BusinessError) => {
        hilog.warn(0x0000, TAG, 'save failed: ' + err.code);
        this.toast('保存失败或已取消');
      });
    } catch (e) {
      hilog.warn(0x0000, TAG, 'export decode/write error');
      this.toast('保存失败');
    }
  }

  build() {
    Column() {
      Web({ src: MAIN_URL, controller: this.controller })
        .fileAccess(true)
        .domStorageAccess(true)
        .javaScriptAccess(true)
        .zoomAccess(false)
        .onLoadIntercept((event: OnLoadInterceptEvent) => {
          // F7: our virtual source passes through; external http(s) main-frame
          // navigations go to the system browser, external subresources are
          // blocked (same semantics as the old 404, but visible for links).
          const url: string = event.data.getRequestUrl();
          if (url.startsWith(URL_PREFIX)) {
            return false;
          }
          if (url.startsWith('http://') || url.startsWith('https://')) {
            if (event.data.isMainFrame()) {
              hilog.info(0x0000, TAG, 'external nav -> browser: ' + url);
              this.openExternal(url);
            } else {
              hilog.warn(0x0000, TAG, 'external subresource blocked: ' + url);
            }
            return true;
          }
          return false; // blob:/data:/about: — the webview's own business
        })
        .multiWindowAccess(true)
        .onWindowNew((event: OnWindowNewEvent) => {
          // F7 part 2: the wasm apps open their links via window.open(url,
          // '_blank') (eframe web open_url -> web_sys Window.open). With
          // multiWindowAccess(false) ArkWeb drops that popup silently — no
          // onLoadIntercept event fires, so part 1 alone cannot catch it.
          // Accept the request, route the URL to the system browser, and
          // decline the popup itself: a null controller means "no new window".
          // The setWebController call is mandatory — skipping it blocks the
          // renderer.
          hilog.info(0x0000, TAG, 'window.open -> browser: ' + event.targetUrl);
          this.openExternal(event.targetUrl);
          event.handler.setWebController(null);
        })
        .onInterceptRequest((event: OnInterceptRequestEvent) => {
          return this.handleRequest(event.request.getRequestUrl());
        })
        .onShowFileSelector((event: OnShowFileSelectorEvent) => {
          return this.pickFiles(event);
        })
        .javaScriptOnDocumentStart([
          {
            script: START_SCRIPT,
            scriptRules: [URL_PATTERN]
          }
        ])
        .onPageEnd(() => {
          // Re-hand the port to every (re)loaded page instance. A WebMessagePort
          // transfers once and dies with its page, so each page instance needs a
          // fresh pair — not just the first controller attach.
          this.setupPorts();
          // Note: javaScriptOnDocumentStart shows no trace on this ArkWeb build
          // (hook/port console lines never surface), so the hook is (re)installed
          // from the main world here BEFORE the port is posted — the message
          // listener must exist when the port arrives, or the port is lost.
          // __pc_hook guard makes the double injection idempotent.
          if (this.sendPort) {
            this.controller.runJavaScript(START_SCRIPT)
              .then(() => {
                this.controller.postMessage('pdfcraft_port', [this.sendPort], '*');
                hilog.info(0x0000, TAG, 'port posted to page');
                this.controller.runJavaScript(
                  "console.log('[pc] main world: hook=' + (window.__pc_hook ? 'y' : 'n') + ' port=' + (window.__pc_port ? 'y' : 'n'))");
              })
              .catch((e: BusinessError) => hilog.warn(0x0000, TAG, 'hook inject failed: ' + e.code));
          }
        })
        .backgroundColor('#141414')
        .width('100%')
        .height('100%')
    }
    .width('100%')
    .height('100%')
  }
}
