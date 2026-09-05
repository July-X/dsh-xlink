/**
 * 初始化脚本，仅注入到 `open_harness` 创建的 dsh web 工作台 webview 中。
 *
 * 工作台是一个 SPA，路由切换完全由内核在 JS 内部用 `history.pushState`
 * 驱动，与浏览器历史栈解耦。用户的「上一页 / 下一页」语义应由 SPA 自己
 * 实现，不应该被 WKWebView / WebView2 把工作台真的搬回到空白的 launch
 * token 页面、甚至直接退出工作台窗口。本脚本让浏览器侧的前进/后退操作
 * 在工作台内变成无操作，避免破坏 SPA 状态或误退到空白页。
 *
 * 三层防护：
 *
 * 1. `history.back` / `history.forward` 直接覆盖成 no-op，覆盖触发这两个
 *    方法的所有来源（包括 Chromium 的菜单、上下文菜单、macOS 触控板手势
 *    等调用栈最终都走 `history.back()`）。
 * 2. `history.go(delta)` 在 delta 不为 0 时也 no-op；delta 为 0 时仍允许
 *    刷新页面，否则连同地址栏的回车刷新都会被这条守卫一起废掉。
 * 3. `popstate` 事件兜底：极少数平台/键盘组合（Alt+←、Alt+→）可能绕过
 *    JS API 直接让 webview 后退，这时浏览器仍会派发 `popstate`，我们在
 *    这里把当前 URL 重新 push 回去，让历史栈回到当前位置，避免工作台
 *    真的被搬走。SPA 自身在 `popstate` 里做的状态恢复继续按原顺序触发，
 *    我们只是把 URL 钉在原处。
 *
 * 顶层 frame 守卫：Tauri 会在每个 frame 中运行初始化脚本；只加固顶层
 * 文档，避免页面里的 iframe 也被卷进来。
 *
 * 不破坏的功能：
 *
 * - 工作台自身的 SPA 路由继续工作（pushState / replaceState 不触发
 *   popstate，也不在被覆盖的 API 列表中）。
 * - 顶部 chrome、灯、刷新按钮等不走历史栈的交互不受影响。
 * - `replaceState`（launch token 刷新等场景）依旧生效 —— 工作台会用它
 *   把过期的 token 替换掉，与本守卫正交。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  // 已经被同源脚本加固过的页面就不再覆盖（例如 HMR 重载时这个脚本会
  // 再次运行；标记位避免对 history 的 descriptor 反复 defineProperty）。
  if (window.__DSH_WORKBENCH_HISTORY_GUARD__) {
    return;
  }
  window.__DSH_WORKBENCH_HISTORY_GUARD__ = true;

  function noop() {
    // 故意留空：保留原型的 `this` 绑定，调用方拿到的返回值依然是
    // undefined，与浏览器侧的实现一致。
  }

  function overrideMethod(target, name, replacement) {
    try {
      Object.defineProperty(target, name, {
        value: replacement,
        writable: true,
        configurable: true,
        enumerable: false,
      });
    } catch (e) {
      // 极少数构建会把 history 的这些方法冻结成不可重定义；保持原状
      // 也不致命，键盘快捷键在那种 webview 里通常也不可达。
      console.warn("workbench-history-guard: cannot override " + name + ": " + e);
    }
  }

  var originalGo = history.go && history.go.bind(history);
  overrideMethod(history, "back", noop);
  overrideMethod(history, "forward", noop);
  overrideMethod(history, "go", function (delta) {
    // delta 为 0 仍走原版（按规范表示刷新当前条目），非零直接吞掉。
    if (delta === 0 && typeof originalGo === "function") {
      return originalGo(0);
    }
    return undefined;
  });

  // 兜底：少数情况下 webview 自身绕过 JS 直接后退，仍会派发 popstate；
  // 这里把当前 URL 重新入栈钉住，不影响 SPA 内部 popstate handler 的运行。
  window.addEventListener("popstate", function () {
    try {
      history.pushState(history.state, "", window.location.href);
    } catch (e) {
      // 当前 origin 不允许修改历史（例如某些 webview 把 history 接管为只读）；
      // 此时覆盖方法已经够用，这里静默吞掉即可。
    }
  });
})();
