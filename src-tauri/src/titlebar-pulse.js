/**
 * Initialization script injected into the `harness` workbench webview
 * and each remote content webview created by `open_official_chat`. It owns
 * the chrome-row sweep on every surface the shell opens so the pulse reads as
 * shell chrome rather than workbench / vendor content. The local
 * `official-chat-strip` webview intentionally does not receive this script.
 *
 * Two surfaces, two palettes:
 *
 * - dsh web workbench (`http://127.0.0.1:<port>`): Gitea green (#609926,
 *   rgb 96,152,38) so the workbench reads as a separate brand surface.
 *   The workbench ships its own `body > [data-titlebar-pulse='2']` DOM
 *   node, so the second sweep matches that pre-existing element.
 *
 * - DeepSeek official chat (`https://chat.deepseek.com`): DeepSeek
 *   official blue (#4D6BFE, rgb 77,107,254) so the chat window reads
 *   as the official brand surface. The chat page does NOT ship the
 *   second-sweep node, so this script appends it before applying the
 *   styles — without that the half-cycle offset would have nothing to
 *   match.
 *
 * Both surfaces stay on the same 6.01s period so the chrome-row sweep
 * carries one rhythm across surfaces; the colors are the only
 * differentiator, picked from `location.hostname` at injection time.
 *
 * The keyframe endpoints are computed from `getBoundingClientRect()` and
 * re-computed on `resize`: in WKWebView, `documentElement.clientWidth`
 * returns physical pixels (e.g. 2560 on a 2x retina display), while CSS
 * layout and `transform` operate in CSS pixels (e.g. 1280). Hard-coding
 * endpoints from `clientWidth` therefore produces a transform range that
 * is 2x the actual visible width — the band reaches the right edge of
 * the compositing layer halfway through the animation and is clipped,
 * then the cycle resets and the band snaps back to the left edge. The
 * visual symptom is "the band vanishes mid-screen and reappears at the
 * left". `getBoundingClientRect().width` returns CSS pixels, matching
 * the coordinate system that `transform` and `width` use. A `resize`
 * listener re-computes the endpoints whenever the window is resized or
 * the display scale changes, so the band always travels exactly one
 * viewport-width plus one band-width.
 *
 * Loading order between this script and the page's own stylesheets is
 * not guaranteed, so every rule carries `!important` — the script also
 * appends the style node on `DOMContentLoaded` (or immediately, if the
 * document has already finished parsing) so the injected sheet lives at
 * the end of `<head>` and therefore wins specificity ties even without
 * `!important`. Belt and braces.
 *
 * Top-frame guard: Tauri runs initialization scripts in every frame, so
 * without the guard an iframe inside chat.deepseek.com (login widgets,
 * analytics) would mount its own copy of the bar and double-paint the
 * sweep. The workbench page is a single document so the guard is a
 * harmless no-op there.
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var STYLE_ID = "dsh-titlebar-pulse";

  // Palette lookup by hostname. The official-chat content webviews
  // (DeepSeek, Qianwen, MiniMax) all run in the same dedicated window
  // and share the official brand blue; anything else (the dsh web
  // workbench lives at 127.0.0.1) falls through to the Gitea green.
  var OFFICIAL_HOSTNAMES = ["chat.deepseek.com", "www.qianwen.com", "agent.minimaxi.com"];
  var isOfficial = OFFICIAL_HOSTNAMES.indexOf(window.location.hostname) !== -1;
  var PALETTE = isOfficial
    ? {
        rgb: "77, 107, 254",
        hex: "#4D6BFE",
        hover: "#7C92FF",
      }
    : {
        rgb: "96, 152, 38",
        hex: "#609926",
        hover: "#7dbd45",
      };
  var HALO = "rgba(" + PALETTE.rgb + ", 0.45)";

  function cssViewportWidth() {
    return document.documentElement.getBoundingClientRect().width
      || document.documentElement.clientWidth
      || window.innerWidth;
  }

  function ensureSecondBar() {
    // The dsh web workbench ships `body > [data-titlebar-pulse='2']` in
    // its DOM; chat.deepseek.com (and any other remote page) does not.
    // Append it here so the half-cycle second sweep has a node to ride,
    // matching the workbench by structure rather than relying on the page
    // to ship the DOM itself.
    var existing = document.querySelector("body > [data-titlebar-pulse='2']");
    if (existing) {
      return existing;
    }
    var node = document.createElement("div");
    node.setAttribute("data-titlebar-pulse", "2");
    node.setAttribute("aria-hidden", "true");
    document.body.appendChild(node);
    return node;
  }

  function buildCss() {
    var viewportPx = cssViewportWidth();
    // Width: 18.24% of the layout viewport, expressed in CSS pixels.
    var bandPx = viewportPx * 0.1824;
    // Keyframe endpoints in CSS pixels, not vw:
    //   0%   → band leading edge one band-width past the left edge
    //   100% → band trailing edge one band-width past the right edge
    var startPx = -bandPx;
    var endPx = viewportPx + bandPx;
    var gradient =
      "linear-gradient(90deg, transparent 0%, " + HALO + " 15%, " + PALETTE.hex + " 50%, " + HALO + " 85%, transparent 100%)";
    return [
      /* Hide a static brand band if the page ever ships one. */
      "body::before { content: none !important; display: none !important; }",
      /* First sweep — left to right across the chrome row. */
      "body::after {",
      "  content: '' !important;",
      "  position: fixed !important;",
      "  top: 0 !important;",
      "  left: 0 !important;",
      "  height: 3px !important;",
      "  width: " + bandPx + "px !important;",
      "  z-index: 1000 !important;",
      "  pointer-events: none !important;",
      "  background: " + gradient + " !important;",
      "  filter: blur(0.4px) !important;",
      "  border-radius: 999px !important;",
      "  animation: dsh-titlebar-pulse-sweep 6.01s linear infinite !important;",
      "  box-shadow: 0 0 8px " + HALO + " !important;",
      "}",
      /* Second sweep — same width, half-cycle offset so the eye reads a
         continuous scan rather than a single dash crossing then dead
         space. On the dsh web workbench this matches the node the page
         ships; on chat.deepseek.com this matches the node ensureSecondBar
         appended above. */
      "body > [data-titlebar-pulse='2'] {",
      "  position: fixed !important;",
      "  top: 0 !important;",
      "  left: 0 !important;",
      "  height: 3px !important;",
      "  width: " + bandPx + "px !important;",
      "  z-index: 1000 !important;",
      "  pointer-events: none !important;",
      "  background: " + gradient + " !important;",
      "  filter: blur(0.4px) !important;",
      "  border-radius: 999px !important;",
      "  animation: dsh-titlebar-pulse-sweep 6.01s linear infinite !important;",
      "  animation-delay: 3.005s !important;",
      "  box-shadow: 0 0 8px " + HALO + " !important;",
      "}",
      "@keyframes dsh-titlebar-pulse-sweep {",
      "  0% { transform: translateX(" + startPx + "px); opacity: 1; }",
      "  100% { transform: translateX(" + endPx + "px); opacity: 1; }",
      "}",
    ].join("\n");
  }

  function inject() {
    ensureSecondBar();
    var existing = document.getElementById(STYLE_ID);
    if (existing) {
      // Recompute on resize: replace the sheet content so the keyframes
      // track the current CSS viewport width.
      existing.textContent = buildCss();
      return;
    }
    var style = document.createElement("style");
    style.id = STYLE_ID;
    style.textContent = buildCss();
    document.head.appendChild(style);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", inject);
  } else {
    inject();
  }

  // Recompute the keyframes whenever the viewport changes size or scale.
  // WKWebView fires `resize` on zoom, window resize, and display scale
  // changes, so this single listener covers every case where the CSS
  // pixel width of the viewport shifts.
  var resizeTimer;
  window.addEventListener("resize", function () {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(inject, 100);
  });
})();