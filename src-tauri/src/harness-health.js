/* 在页面有充足挂载时间后，检测真正空白或崩溃的工作台文档。 */
(function () {
  if (window.top !== window.self || window.__DSH_HARNESS_HEALTH_PROBE__) return;
  window.__DSH_HARNESS_HEALTH_PROBE__ = true;
  var reported = false;
  var reportAttempts = 0;
  var reportTimer = null;
  var reportInFlight = false;
  var pendingReport = null;
  var launcherSelector = "#dsh-shell-launcher";
  var maxReportAttempts = 4;

  function clip(value, limit) {
    return String(value || "").slice(0, limit || 4000);
  }

  function scheduleReportRetry() {
    if (reported || !pendingReport || reportTimer || reportAttempts >= maxReportAttempts) return;
    reportTimer = window.setTimeout(function () {
      reportTimer = null;
      sendReport();
    }, Math.min(5000, 500 * Math.max(1, reportAttempts)));
  }

  function sendReport() {
    if (reported || reportInFlight || reportTimer || !pendingReport || reportAttempts >= maxReportAttempts) return;
    var tauri = window.__TAURI__ && window.__TAURI__.core;
    if (!tauri || typeof tauri.invoke !== "function") {
      reportAttempts += 1;
      scheduleReportRetry();
      return;
    }
    reportAttempts += 1;
    var payload = pendingReport;
    reportInFlight = true;
    try {
      Promise.resolve(tauri.invoke("report_harness_fault", payload)).then(function () {
        reportInFlight = false;
        reported = true;
        pendingReport = null;
        window.__DSH_HARNESS_HEALTH_REPORTED__ = true;
      }).catch(function () {
        reportInFlight = false;
        // 对短暂的 IPC 竞态进行重试；管理面板已关闭时，不能把页面永久标记为已上报。
        scheduleReportRetry();
      });
    } catch (error) {
      reportInFlight = false;
      scheduleReportRetry();
    }
  }

  function invokeReport(kind, message, stack) {
    if (reported) return;
    if (!pendingReport) {
      pendingReport = {
        kind: clip(kind, 80),
        message: clip(message, 2000),
        stack: clip(stack, 8000),
        pageUrl: clip(window.location && window.location.href, 1000)
      };
    }
    sendReport();
  }

  function errorText(value) {
    if (!value) return "";
    if (typeof value === "string") return value;
    return value.stack || value.message || String(value);
  }

  /**
   * 取「类型 + 消息」以及 cause 链，作为 message 上报。
   *
   * WebKit 的 `Error.prototype.stack` 只有帧（`fn@url:行:列`），没有 V8 那样的
   * `TypeError: …` 首行；只上报 stack 会让事故面板里没有任何可读的错误原因，
   * 归因也就无从谈起。`error` 事件自带 message，但 `unhandledrejection` 只有
   * reason 对象，必须在探针里把它拆出来。
   */
  function describeError(value) {
    if (!value) return "";
    if (typeof value === "string") return value;
    var parts = [];
    var current = value;
    for (var depth = 0; current && depth < 3; depth += 1) {
      var name = typeof current.name === "string" ? current.name : "";
      var text = typeof current.message === "string" ? current.message : "";
      var label = name && text ? name + ": " + text : name || text;
      if (!label) {
        if (depth > 0) break;
        label = String(current);
      }
      parts.push(depth === 0 ? label : "cause: " + label);
      current = current.cause;
    }
    return parts.join(" ← ");
  }

  /**
   * 客户端模块 bundle 的 `<script>` 加载失败所对应的地址，不是地址就返回空串。
   *
   * 资源失败事件不带 JS 栈，但事件目标带 `src`——而 `src` 是**唯一**带完整
   * `/plugins/??<包名>/client.js,…&rev=…` 组合路由的地方。包名只活在这个查询串
   * 里：内核把加载失败的模块行静默丢掉之后，工作台随后只会抛
   * `renderSlot('root') before any 'root' registration (boot order)` 这类启动
   * 顺序错误，而那类堆栈落在**多成员**组合上，壳按设计拒绝据此归因（见
   * `guard.rs` 的 `is_ambiguous_combo_line`），于是证据里一个包名都没有。
   *
   * 只认 `/plugins/`：那条路由只服务客户端模块 bundle，页面上其它资源失败
   * （图片、字体、第三方脚本）仍然无害，不该占用一次性上报。
   */
  function bundleScriptUrl(target) {
    if (!target || String(target.tagName || "").toUpperCase() !== "SCRIPT") return "";
    var src = String(target.src || "");
    return src.indexOf("/plugins/") >= 0 ? src : "";
  }

  function recordBundleFailure(url) {
    // 一次事故只报第一条：内核按启动顺序分批请求组合路由，先失败的那批才是
    // 因，后面那些「退而改用自己的单资源 URL」的重试失败只是它的连带。
    // 也因此后面真正的 `runtime-error` 不再覆盖它——因果先于症状。
    if (reported || pendingReport) return;
    pendingReport = {
      kind: "bundle-load-failure",
      message: "内核客户端模块 bundle 加载失败：" + url,
      stack: url,
      pageUrl: clip(window.location && window.location.href, 1000)
    };
    sendReport();
  }

  window.addEventListener("error", function (event) {
    // bundle 加载失败优先于一切可执行错误上报：它是因，上面那条不是。
    var bundleUrl = bundleScriptUrl(event && event.target);
    if (bundleUrl) {
      recordBundleFailure(bundleUrl);
      return;
    }
    // 其余资源错误没有有用的 JS 栈，且通常无害（例如可选的图片），
    // 这里只上报可执行错误。
    var error = event && event.error;
    var message = event && event.message;
    if (!error && !message) return;
    invokeReport(
      "runtime-error",
      message || describeError(error) || errorText(error),
      errorText(error)
    );
  }, true);

  window.addEventListener("unhandledrejection", function (event) {
    var reason = event && event.reason;
    var detail = describeError(reason) || errorText(reason) || "未处理的 Promise 异常";
    invokeReport("unhandled-rejection", detail, reason && reason.stack);
  });

  function isVisible(element) {
    if (!element || (element.closest && element.closest(launcherSelector))) return false;
    var style = window.getComputedStyle(element);
    if (!style || style.display === "none" || style.visibility === "hidden" || style.opacity === "0") return false;
    var rect = element.getBoundingClientRect();
    return rect.width > 2 && rect.height > 2;
  }

  function hasRenderedContent() {
    var body = document.body;
    if (!body) return false;
    var text = (body.innerText || "").replace(/\s+/g, "").trim();
    if (text) return true;

    var candidates = body.querySelectorAll("canvas, iframe, video, img, svg, [role='main'], main, button, input, textarea, select, [data-testid]");
    for (var i = 0; i < candidates.length; i += 1) {
      if (isVisible(candidates[i])) return true;
    }

    var roots = body.querySelectorAll("#app, #root");
    for (var j = 0; j < roots.length; j += 1) {
      if (!roots[j].querySelector(launcherSelector) && roots[j].children.length > 0 && isVisible(roots[j])) return true;
    }
    return false;
  }

  var blankChecks = 0;

  function checkBlank() {
    if (hasRenderedContent()) return;
    blankChecks += 1;
    if (blankChecks >= 2) {
      invokeReport("blank", "工作台页面加载完成后仍为空白，未发现可见内容", "");
    } else {
      window.setTimeout(checkBlank, 4000);
    }
  }

  function schedule() {
    window.setTimeout(checkBlank, 5000);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", schedule, { once: true });
  } else {
    schedule();
  }
})();
