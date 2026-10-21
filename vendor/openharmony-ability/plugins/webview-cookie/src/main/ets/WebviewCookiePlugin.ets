import webview from "@ohos.web.webview";
import {
  BridgeCallContext,
  BridgeTypedValue,
  BridgeContextRequirement,
  SyncPluginBase,
} from "@ohos-rs/ability";

const FETCH_COOKIES_REQUEST_TYPE = "ohos.webview.CookieFetchRequest";
const FETCH_COOKIES_RESPONSE_TYPE = "ohos.webview.CookieFetchResponse";

interface FetchCookiesRequest {
  url: string;
}

class FetchCookiesResponse {
  readonly cookies: string;

  constructor(cookies: string) {
    this.cookies = cookies;
  }
}

function parseFetchCookiesRequest(payload: BridgeTypedValue): FetchCookiesRequest {
  if (payload.typeName !== FETCH_COOKIES_REQUEST_TYPE || typeof payload.value !== "object") {
    throw new Error(
      `ohos.webview-cookie.fetch-cookies requires bridge type ${FETCH_COOKIES_REQUEST_TYPE}`,
    );
  }
  const request = payload.value as FetchCookiesRequest;
  if (typeof request?.url !== "string" || request.url.length === 0) {
    throw new Error("fetch-cookies.url must be a non-empty string");
  }
  return request;
}

/**
 * Synchronous cookie-manager bridge plugin for ArkTS.
 *
 * `WebCookieManager.fetchCookieSync` is a static, UI-thread-only API. The
 * async `ohos.webview` plugin cannot serve it to a main-thread caller (the
 * TSFN round-trip needs that same main thread to pump, i.e. deadlock), so the
 * sync fetch lives in this dedicated `MainThreadSync` plugin: the call always
 * executes on the UI thread, exactly where the official API requires it.
 *
 * Plugin ID: `ohos.webview-cookie`
 * Required contexts: none (the cookie manager is process-global, not
 * per-webview)
 */
export class WebviewCookiePlugin extends SyncPluginBase {
  readonly id = "ohos.webview-cookie";
  readonly requires: BridgeContextRequirement[] = [];

  invokeSync(
    action: string,
    payload: BridgeTypedValue,
    _context: BridgeCallContext,
  ): BridgeTypedValue {
    if (action === "fetch-cookies") {
      const request = parseFetchCookiesRequest(payload);
      try {
        const cookies = webview.WebCookieManager.fetchCookieSync(request.url);
        return {
          typeName: FETCH_COOKIES_RESPONSE_TYPE,
          value: new FetchCookiesResponse(cookies),
        };
      } catch (error) {
        throw new Error(`Unable to fetch WebView cookies: ${String(error)}`);
      }
    }

    throw new Error(`Unsupported ohos.webview-cookie action '${action}'`);
  }
}
