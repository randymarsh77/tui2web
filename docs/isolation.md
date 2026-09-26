# Isolation and security limits

[Project overview](../README.md) · [Browser API](browser.md)

## Trusted modules: `mount`

A same-origin Worker keeps computation off the UI thread but does not isolate imported JavaScript from browser networking, same-origin storage, or other Worker APIs. Use `mount` only for modules you trust.

WASM receives imports from its JavaScript glue. Compiling an app to WASM does not by itself prevent network access.

## Restricted guests: `mountIsolated`

```js
import { mountIsolated } from "./runtime/index.js";

const terminal = await mountIsolated(container, {
  moduleUrl: new URL("./pkg/my_app.js", document.baseURI),
  persistence: "isolated:my-document",
});
```

The host fetches the configured app glue, WASM, and trusted runtime assets with size limits. The caller controls these URLs. Do not let guest input choose asset URLs or `runtimeBaseUrl`; use an allowlist for uploads or build selections.

The iframe uses `sandbox="allow-scripts"` without `allow-same-origin`. Only trusted runtime code executes in the frame document. App glue runs in a dedicated classic blob Worker, which dynamically imports a blob ES module. Classic loading supports Workers in Chromium's opaque-origin frames.

Bundle app dependencies into one ES module. Isolated mode does not support relative, import-map, or remote dependencies.

## Network and storage boundary

The frame and Worker inherit a CSP that denies network access:

```text
default-src 'none'
connect-src 'none'
script-src 'unsafe-inline' 'wasm-unsafe-eval' blob:
worker-src blob:
```

Images, fonts, forms, and base URL overrides are also blocked. There is no asset-origin network exception for guests. `unsafe-inline` starts trusted frame code; `unsafe-eval` is not granted.

Browser tests check denied fetch, WebSocket, dynamic HTTP imports, `importScripts`, and IndexedDB from inside the guest Worker, plus parent-DOM, network, and storage denial from the frame.

Persistence remains in the parent through a per-frame `MessagePort`. Initial connection checks the parent Window and accepts one port. Guest Workers never receive it.

The parent binds one store and namespace, accepts only load/save/clear, and validates snapshot schema and quotas. It never uses guest-supplied namespaces or paths on the host filesystem. Storage access transfers virtual-file data, not host files. Disposal removes the frame, closes ports, and frees blob URLs.

## Limits

This is browser isolation for static hosting, not an adversarial multi-tenant compute service. There are no portable hard CPU or memory quotas. Guests can allocate before validation, exhaust a browser process, or spawn blob Workers. Watchdogs cannot guarantee recovery from process or renderer out-of-memory failures.

Browser bugs, side channels, and untrusted extensions are outside this boundary. A stalled guest Worker can be restarted. If the trusted frame or channel stalls, its timeout removes the frame; remount to recover.

The embedding site's CSP must allow the required sandbox, srcdoc, blob, and WASM capabilities. A stricter ancestor policy can block startup; failure is reported without falling back to trusted mode.

Automated browser coverage is Chromium. Other engines and OS IMEs need separate qualification.
