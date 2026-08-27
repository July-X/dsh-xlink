/**
 * Initialization script injected ONLY into the `official-chat` webview (the
 * dedicated WebviewWindow that loads `https://chat.deepseek.com`).
 *
 * Strategy — be an honest desktop browser, not a disguised one. The engine
 * behind WebView2 IS a genuine desktop Edge/Chromium build, and it stays
 * self-consistent across every layer a site can compare: the User-Agent
 * header (no longer overridden), the `Sec-CH-UA` client hints derived from
 * the real brand, native `navigator.userAgentData`, plugins, permissions,
 * canvas/WebGL reads. Earlier revisions of this script faked those surfaces
 * as "Google Chrome" with plain-JS shims; each shim was itself detectable
 * (a `{}` where the spec puts a `NavigatorUAData` instance, a scripted
 * function where the spec puts a native one), and the JS-side Chrome claim
 * contradicted the HTTP-side Edge brand — exactly the kind of mismatch
 * environment checks look for. All of that is gone.
 *
 * What remains is only the removal of embedded-webview traces:
 *
 * 1. Top-frame guard — initialization scripts run in every frame; only
 *    harden the top document.
 * 2. `navigator.webdriver` — pinned to `false` (the value a normal,
 *    non-automated desktop browser reports). The launcher args in
 *    `OFFICIAL_CHAT_BROWSER_ARGS` already disable the automation switches
 *    at the engine level; the prototype pin is belt-and-braces for builds
 *    where the flag is unavailable. Note `false`, not `undefined`: a
 *    missing property is itself a bot signal.
 * 3. Tauri globals — deleted outright (`__TAURI__`, `__TAURI_INTERNALS__`,
 *    `__TAURI_METADATA__`, `__TAURI_IPC__`). A normal browser shows
 *    nothing there, so page probes must see nothing too — exposing a
 *    Proxy "so typeof checks pass" still announces an embedded webview.
 *    The pull-string lamp lives on the official-chat strip webview
 *    (label `official-chat-strip`), which a local webview that does not
 *    run this fingerprint, so it keeps the live bridge untouched. (A
 *    brief stint on a separate `official-chat-launcher` webview was
 *    reverted because WebView2 child HWND transparency doesn't work on
 *    this Tauri 2.11 / wry 0.55.1 stack — the launcher kept painting
 *    an opaque dark square regardless of the controller's transparent
 *    DefaultBackgroundColor, so the lamp moved back into the strip
 *    and the strip grew from 38px to 66px to fit the 66px SVG.)
 *
 * The script is pure JavaScript (no TypeScript) and string-safe — it is
 * `include_str!`-embedded into Rust source at compile time, so any nested
 * backtick / unescaped quote / non-ASCII literal will fail the build.
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  // --- navigator.webdriver ------------------------------------------------
  // Frozen on the prototype so every fresh document inherits the spoofed
  // value before any script can re-define it on the instance.
  try {
    Object.defineProperty(Navigator.prototype, "webdriver", {
      get: function () { return false; },
      configurable: true,
      enumerable: true,
    });
  } catch (e) {
    // Older engines may forbid overriding the prototype — fall back to
    // an instance-level defineProperty, which still satisfies the probe
    // for the current document.
    try {
      Object.defineProperty(navigator, "webdriver", {
        get: function () { return false; },
        configurable: true,
        enumerable: true,
      });
    } catch (_e2) { /* nothing else we can do here */ }
  }

  // --- Tauri globals ------------------------------------------------------
  // Delete the IPC surface instead of masking it: a normal browser has no
  // `__TAURI_*` properties at all, so page probes must observe exactly
  // that. `delete` wins when the property is configurable (the common
  // case); if some build made it permanent, fall back to an undefined
  // getter so at least the VALUE reads as absent.
  var tauriGlobals = ["__TAURI__", "__TAURI_INTERNALS__", "__TAURI_METADATA__", "__TAURI_IPC__"];
  for (var i = 0; i < tauriGlobals.length; i++) {
    var name = tauriGlobals[i];
    try {
      delete window[name];
    } catch (e) { /* not configurable — mask below */ }
    if (window[name] !== undefined) {
      try {
        Object.defineProperty(window, name, {
          get: function () { return undefined; },
          configurable: true,
          enumerable: false,
        });
      } catch (e2) { /* leave as-is rather than crash the document */ }
    }
  }
})();
