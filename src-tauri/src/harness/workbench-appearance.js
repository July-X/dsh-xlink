/**
 * 初始化脚本，注入到 `harness` 工作台 webview（两条建窗链都要）与 `mcode-workbench`
 * MiniMax 工作台（那里的偏好信号由 `mcode-appearance.js` 翻译成同一套语义）。
 *
 * 让工作台的原生外观与页面内容同色，规则**先看内核、再问外壳**：
 *
 *   · 内核 `ui-theme = light / dark`（用户在内核设置里明确选了固定档）
 *     ⇒ 钉成同一档，壳主题不参与。用户在内核里的明确选择优先。
 *   · 内核 `ui-theme = system`（工作台设置里选的是「跟随系统」）
 *     ⇒ 跟**外壳**主题走：壳固定浅/深就钉这一档；壳也是「跟随系统」就下发
 *       `null`，由原生侧读取系统明暗而不是读取当前宿主的外观。这样在系统级切换
 *       明暗时，外壳与工作台一起变（壳自己的 `matchMedia` 监听负责外壳那半）。
 *   · 内核版本太旧、没有 `data-ds-theme-source` 信号 ⇒ 什么都不做，降级行为
 *     与「没有这个脚本」一致。旧内核只有解析结果（body 的 dark 属性），只敢
 *     认深色；切回浅色时没有信号，不猜。
 *
 * 内核为此发布了只读信号（`dsh-client-ui-layout` 的 ThemePresenter，
 * `THEME_SOURCE_ATTRIBUTE` 的注释写明是给原生壳镜像 nativeTheme 用的）：
 * `html[data-ds-theme-source]` = `light` / `dark` / `system`（**偏好**；固定
 * 偏好发布的是它解析出来的那一档）。壳偏好由主壳经 `report_shell_theme`
 * 上报，Rust 用 `eval` 推一个 `dsh-shell-theme` DOM 事件（不走 Tauri event：
 * 工作台是 remote origin，页面侧 `listen` 要 `core:event:allow-listen` 授权，
 * 漏了是静默失败），`get_shell_theme` 的 1s 轮询是推送丢失时的兜底。
 *
 * 建窗时仍然不钉主题（`appearance::initial_theme(WORKBENCH_LABEL)` 返回
 * `None`）：壳在上报之前没有任何答案，此时冻住 `prefers-color-scheme` 会让
 * 内核的「跟随系统」第二次失效（2026-10-18 实测）。
 *
 * 命令授权在 `capabilities/harness-remote.json`（`allow-window-appearance` +
 * `allow-get-shell-theme`）：工作台是 remote origin，漏一条就是真机 ACL 静默拒、
 * `.catch` 吞掉，症状是「标题栏不跟主题」且没有任何线索——由
 * `check-invariants` 第 ⑥ 项与 `ui/test/windowChrome.test.js` 钉住。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var SOURCE_ATTRIBUTE = 'data-ds-theme-source';
  var DARK_ATTRIBUTE = 'data-ds-dark-theme';
  var POLL_MS = 1000;

  /** 已下发的值：`undefined` = 还没下发过，`null` = 跟随系统，否则是固定档。 */
  var applied;
  var appliedSystem;
  var applying = false;
  /** 壳偏好：`undefined` = 主壳还没上报，`null` = 跟随系统，否则是固定档。 */
  var shell;
  var systemTheme;
  var pendingRead;
  var warned = false;
  var observer = null;

  /** 内核声明的偏好；读不到返回 `null`（**无信号**，不是 system）。 */
  function readKernelSource() {
    var root = document.documentElement;
    var source = root && root.getAttribute(SOURCE_ATTRIBUTE);
    if (source === 'system' || source === 'light' || source === 'dark') {
      return source;
    }
    // 兜底：旧内核只有解析结果（body 的 dark 属性），没有偏好属性。
    if (document.body && document.body.hasAttribute(DARK_ATTRIBUTE)) {
      return 'dark';
    }
    return null;
  }

  function bridge() {
    var api = window.__TAURI__ && window.__TAURI__.core;
    return api && typeof api.invoke === 'function' ? api : null;
  }

  /**
   * 该下发什么：内核固定档优先；内核 system 时用壳偏好；
   * 任一侧还没答案时返回 `undefined`（保持现状，下一拍再问）。
   */
  function wanted() {
    var source = readKernelSource();
    if (!source) {
      return undefined;
    }
    if (source !== 'system') {
      return source;
    }
    if (shell === undefined) {
      return undefined;
    }
    return shell;
  }

  function sync() {
    var target = wanted();
    var system = target === null ? systemTheme : undefined;
    if (applying || target === undefined || (target === applied && system === appliedSystem)) {
      return;
    }
    var api = bridge();
    if (!api) {
      return; // 桥还没就绪：不记 applied，下一拍重试。
    }
    // 串行下发；原生调用在途时若换主题或迁移，完成后再应用最新状态。
    applying = true;
    applied = target;
    appliedSystem = system;
    function failed(error) {
      applying = false;
      applied = undefined; // 下一拍重试，避免失败时立即递归。
      if (!warned) {
        warned = true;
        console.warn('[dsh-xlink] 工作台窗口外观同步失败：', error);
      }
    }
    try {
      // 命令名保留字面量，由 check-invariants 核对注册及授权。
      Promise.resolve(api.invoke('set_window_appearance', { theme: target })).then(function () {
        applying = false;
        sync();
      }, failed);
    } catch (error) {
      failed(error);
    }
  }

  /** 读主壳上报的偏好；主壳还没启动完时每拍重试。 */
  function readShell() {
    var api = bridge();
    if (!api || pendingRead) {
      return;
    }
    var read = {};
    pendingRead = read;
    function finish() {
      if (pendingRead !== read) return false;
      pendingRead = null;
      return true;
    }
    try {
      Promise.resolve(api.invoke('get_shell_theme', {})).then(function (view) {
        if (!finish() || !view || !view.reported) return;
        shell = view.theme || null;
        systemTheme = view.systemTheme || undefined;
        sync();
      }, finish);
    } catch (error) {
      finish(); // 读失败保持现状，下一拍重试。
    }
  }

  /** 壳偏好变化立刻生效：Rust 用 eval 推 DOM 事件，不等下一拍轮询。 */
  function followShell() {
    window.addEventListener('dsh-shell-theme', function (event) {
      pendingRead = null; // 不让较旧的轮询结果盖掉推送。
      shell = (event && event.detail) || null;
      systemTheme = undefined;
      sync();
      if (shell === null) readShell();
    });
  }

  /** 注入脚本在 document-start 跑，`documentElement` 可能还没有：观察者尽力挂，挂了就靠定时。 */
  function attach() {
    if (observer || typeof MutationObserver !== 'function' || !document.documentElement) {
      return;
    }
    observer = new MutationObserver(sync);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: [SOURCE_ATTRIBUTE],
    });
  }

  function tick() {
    attach();
    readShell();
    sync();
  }

  window.addEventListener('dsh-xlink-surface-host-changed', function () {
    applied = undefined;
    sync();
  });
  // 系统明暗变化只能靠轮询发现。窗口重新可见或拿到焦点时立刻补读一次，
  // 别让后台节流把「跟随系统」拖到下一拍之后——最小化再切回来的人看到的
  // 第一眼就是旧配色。
  window.addEventListener('focus', readShell);
  document.addEventListener('visibilitychange', function () {
    if (!document.hidden) readShell();
  });
  followShell();
  tick();
  setInterval(tick, POLL_MS);
  document.addEventListener('DOMContentLoaded', tick, { once: true });
})();
