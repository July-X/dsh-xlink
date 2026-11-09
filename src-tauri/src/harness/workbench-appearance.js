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
 *       `null` 解除覆盖，让系统主题决定。这样 macOS / Windows 在系统级切换
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
  /** 壳偏好：`undefined` = 主壳还没上报，`null` = 跟随系统，否则是固定档。 */
  var shell;
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
    if (target === undefined || target === applied) {
      return;
    }
    var api = bridge();
    if (!api) {
      return; // 桥还没就绪：不记 applied，下一拍重试。
    }
    applied = target;
    try {
      // 命令名写成字面量：check-invariants 第 ③/⑥ 项按字面形状核对
      // 「已注册 + 已授权给 harness 窗口」，用常量会从扫描里整体消失。
      Promise.resolve(api.invoke('set_window_appearance', { theme: target })).catch(function (error) {
        // 失败不算已下发（下一拍重试），但只报一次，别把 console 刷满。
        applied = undefined;
        if (!warned) {
          warned = true;
          console.warn('[dsh-xlink] 工作台窗口外观同步失败：', error);
        }
      });
    } catch (error) {
      applied = undefined;
      if (!warned) {
        warned = true;
        console.warn('[dsh-xlink] 工作台窗口外观同步失败：', error);
      }
    }
  }

  /** 读主壳上报的偏好；主壳还没启动完时每拍重试。 */
  function readShell() {
    var api = bridge();
    if (!api) {
      return;
    }
    try {
      Promise.resolve(api.invoke('get_shell_theme', {})).then(function (view) {
        if (!view || !view.reported) {
          return;
        }
        var theme = view.theme || null;
        if (theme === shell) {
          return;
        }
        shell = theme;
        sync();
      }).catch(function () {
        // 读失败保持现状，下一拍重试。
      });
    } catch (error) {
      // 同上：只靠轮询重试。
    }
  }

  /** 壳偏好变化立刻生效：Rust 用 eval 推 DOM 事件，不等下一拍轮询。 */
  function followShell() {
    window.addEventListener('dsh-shell-theme', function (event) {
      shell = (event && event.detail) || null;
      sync();
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

  followShell();
  tick();
  setInterval(tick, POLL_MS);
  document.addEventListener('DOMContentLoaded', tick, { once: true });
})();
