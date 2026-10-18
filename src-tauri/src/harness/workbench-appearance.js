/**
 * 初始化脚本，注入到 `harness` 工作台 webview（两条建窗链都要）。
 *
 * 把内核**声明的外观偏好**镜像到本窗口的原生外观，让原生标题栏与页面内容同色。
 *
 * 为什么壳要做这件事：工作台窗口走原生装饰，而页面内容（内核 Web UI）的明暗由
 * 内核的 `ui-theme` 设置决定。壳两个方向都不能做过头——
 *   · 建窗 / 页加载时替它钉一个固定档：页内的 `prefers-color-scheme` 恒为那一
 *     档，内核的「跟随系统」永远跟随不了（2026-10-18 用户实测：系统浅色 + 偏好
 *     system，工作台仍渲染深色）。所以 `appearance::initial_theme(WORKBENCH_LABEL)`
 *     两个平台都返回 `None`；
 *   · 干脆不管：浅色系统 + 内核选深色时就成了「深色内容 + 浅色标题栏」
 *     （2026-10-07 用户截图）。
 *
 * 内核为此发布了只读信号（`dsh-client-ui-layout` 的 ThemePresenter，
 * `THEME_SOURCE_ATTRIBUTE` 的注释写明是给原生壳镜像 nativeTheme 用的）：
 *   · `html[data-ds-theme-source]` = `light` / `dark` / `system`（**偏好**；固定
 *     偏好发布的是它解析出来的那一档）。
 *
 * 规则只有一条：**按偏好镜像，绝不按解析结果钉住**。
 *   · `system` ⇒ 下发 `null`，解除本窗覆盖：窗口跟随系统，页内的
 *     `prefers-color-scheme` 才活着。若在这里钉住当时解析出来的那一档，这个查询
 *     就被自己冻住——系统切档时内核再也收不到变化，「跟随系统」第二次失效。
 *   · `light` / `dark` ⇒ 钉成同一档：内核在这两档本来就不查
 *     `prefers-color-scheme`，不存在回路。
 *
 * 读不到信号（内核没写 / 版本更早）时**什么都不做**：窗口保持跟随系统，降级行为
 * 与「没有这个脚本」完全一致。失败只进 console，绝不打断页面。
 *
 * 命令授权在 `capabilities/harness-remote.json`（`allow-window-appearance`）：工作台
 * 是 remote origin，漏了那条授权就是真机 ACL 静默拒、`.catch` 吞掉，症状是
 * 「浅色系统下选深色 → 标题栏还是白的」——由 `check-invariants` 第 ⑥ 项钉住。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var COMMAND = 'set_window_appearance';
  var SOURCE_ATTRIBUTE = 'data-ds-theme-source';
  var DARK_ATTRIBUTE = 'data-ds-dark-theme';
  var POLL_MS = 1000;

  /** 已下发的值：`null` = 解除覆盖，`'light'` / `'dark'` = 固定档；`undefined` = 还没下发过。 */
  var applied;
  var warned = false;
  var observer = null;

  /** 内核声明的偏好；没有信号时返回 `null`（**不下发**，不是「解除覆盖」）。 */
  function readSource() {
    var root = document.documentElement;
    var source = root && root.getAttribute(SOURCE_ATTRIBUTE);
    if (source === 'system' || source === 'light' || source === 'dark') {
      return source;
    }
    // 兜底：偏好属性缺失、但 body 上已有解析结果（旧版内核）时按它当固定档。
    if (document.body && document.body.hasAttribute(DARK_ATTRIBUTE)) {
      return 'dark';
    }
    return null;
  }

  function bridge() {
    var api = window.__TAURI__ && window.__TAURI__.core;
    return api && typeof api.invoke === 'function' ? api : null;
  }

  function sync() {
    var source = readSource();
    if (!source) {
      return;
    }
    var wanted = source === 'system' ? null : source;
    if (wanted === applied) {
      return;
    }
    var api = bridge();
    if (!api) {
      return; // 桥还没就绪：不记 applied，下一拍重试。
    }
    applied = wanted;
    try {
      Promise.resolve(api.invoke(COMMAND, { theme: wanted })).catch(function (error) {
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
    sync();
  }

  tick();
  setInterval(tick, POLL_MS);
  document.addEventListener('DOMContentLoaded', tick, { once: true });
})();
