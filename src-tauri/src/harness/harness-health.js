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

  /**
   * 内核客户端渲染器（`dsh-client-ui-renderer`）抛出的槽位装配不变量。
   *
   * 命中它们意味着：模块图在页面活着的时候被换掉了，而换图的那一瞬间
   * `dsh-client-ui-session` 被销毁——`slots.installScope` 的 effect disposer
   * 把 `session` 作用域摘掉了（渲染器 `client.js:1466`），`ScopeProvider` 恰好在
   * 「已删除、尚未重装」的窗口里重渲染，于是整个工作台抛这一句、页面变死。
   *
   * 2026-09-30 定案的触发机制（此前注释把它归给「文件监视器事件风暴」，不准）：
   * pnpm 的内容寻址 store 让两个壳的内核树与 store **共享 inode**，而 NTFS 上
   * 硬链接数增减会更新 ChangeTime——对面装 / 删内核时，内核 `dsh-client-hmr`
   * 每 500ms 的 bundle stat 轮询把这些 ctime 噪声当成「bundle 重建」推给**活页面**
   * （SSE `rebuilt` 帧 → 换模块 → 本条不变量）。当天五次装 / 删全部在 4~6 秒内
   * 打死对面的工作台页面，与 CPU / 磁盘负载无关。
   *
   * 与文案一并取自渲染器的抛点，列表与 `guard.rs` 的 `SLOT_PHRASES` 对齐。
   */
  var SLOT_ASSEMBLY_PHRASES = [
    "rendered without an installed adapter",
    "renderSlot('root') before any 'root' registration",
    "rendered outside the root standard-source provider",
    "rendered outside its scope provider"
  ];

  // 自动恢复只允许试一次，且这个额度是**整个窗口会话**的（存在 sessionStorage，
  // 跨刷新存活），所以绝不会出现「刷新 → 又坏 → 再刷新」的循环。
  var RECOVERY_FLAG = "dsh-harness-slot-recovery";

  function recoverySpent() {
    try {
      return window.sessionStorage.getItem(RECOVERY_FLAG) === "1";
    } catch (error) {
      return true;
    }
  }

  function spendRecovery() {
    try {
      window.sessionStorage.setItem(RECOVERY_FLAG, "1");
      return true;
    } catch (error) {
      return false;
    }
  }

  function isSlotAssemblyFailure(text) {
    var haystack = String(text || "");
    for (var i = 0; i < SLOT_ASSEMBLY_PHRASES.length; i += 1) {
      if (haystack.indexOf(SLOT_ASSEMBLY_PHRASES[i]) >= 0) return true;
    }
    return false;
  }

  /**
   * 装配不变量撞上之后：记一笔，然后自愈一次。
   *
   * 报告先发、刷新后延——刷新会连同本页的内存一起丢掉，证据必须先落到
   * `last-incident.json`。这一次走 `slot-assembly`，管理面板只上横幅不弹面板
   * （工作台会自己回来，没有需要用户立刻处理的事）；额度用掉之后再撞上同样
   * 的错，就退回普通的 `runtime-error` 弹面板——那时刷新救不回来，用户确实
   * 需要动作（多半是换一个内核版本）。
   *
   * 刷新的**落点**要对（2026-09-30 实测：落在对面卸载内核风暴中间的那次自愈
   * 刷新，几秒后又撞死一次）：先问壳要一个退避毫秒数（`harness_reload_backoff`
   * 读跨壳装包信标），风没停就等，风停了再刷。IPC 不可用 / 失败时按老行为立刻
   * 刷新——退避是优化，不是前提；轮数有上限，防一次卡死的装包把页面无限期晾黑。
   */
  var RELOAD_BACKOFF_MAX_POLLS = 30;

  function reloadWhenQuiet(polls) {
    if (polls >= RELOAD_BACKOFF_MAX_POLLS) {
      window.setTimeout(function () { window.location.reload(); }, 3000);
      return;
    }
    var tauri = window.__TAURI__ && window.__TAURI__.core;
    if (!tauri || typeof tauri.invoke !== "function") {
      window.setTimeout(function () { window.location.reload(); }, 3000);
      return;
    }
    Promise.resolve(tauri.invoke("harness_reload_backoff")).then(function (ms) {
      if (ms > 0) {
        window.setTimeout(function () {
          reloadWhenQuiet(polls + 1);
        }, Math.min(ms + 500, 5000));
        return;
      }
      // 风已停：保底 3 秒再刷——报告先落地（`last-incident.json`），
      // 刷新会连同本页内存一起丢掉。
      window.setTimeout(function () { window.location.reload(); }, 3000);
    }).catch(function () {
      window.setTimeout(function () { window.location.reload(); }, 3000);
    });
  }

  function handleSlotAssemblyFailure(text, stack) {
    if (recoverySpent() || !spendRecovery()) {
      invokeReport("runtime-error", text, stack);
      return;
    }
    invokeReport("slot-assembly", text, stack);
    reloadWhenQuiet(0);
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
    var text = message || describeError(error) || errorText(error);
    if (isSlotAssemblyFailure(text) || isSlotAssemblyFailure(errorText(error))) {
      handleSlotAssemblyFailure(text, errorText(error));
      return;
    }
    invokeReport("runtime-error", text, errorText(error));
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
