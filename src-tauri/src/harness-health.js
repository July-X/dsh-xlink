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

  window.addEventListener("error", function (event) {
    // 资源错误没有有用的 JS 栈，且通常无害（例如可选的图片），
    // 这里只上报可执行错误。
    var error = event && event.error;
    var message = event && event.message;
    if (!error && !message) return;
    invokeReport("runtime-error", message || errorText(error), errorText(error));
  }, true);

  window.addEventListener("unhandledrejection", function (event) {
    var detail = errorText(event && event.reason) || "未处理的 Promise 异常";
    invokeReport("unhandled-rejection", detail, event && event.reason && event.reason.stack);
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
