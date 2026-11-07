/**
 * 初始化脚本，注入到 MiniMax 工作台 webview（`src-tauri/src/harness/mcode_window.rs`）。
 *
 * MiniMax 0.1.7 的登录层认证成功后**不自己收起**：页面的 `onAuthenticated`
 * 只刷新用量与账号资料，没有关掉那一层（上游缺陷，已安装的包不能改，壳也不能
 * 依赖上游什么时候修）。登录层的开关 `loginOpen` 初值是 `false`——它只在用户
 * 点「登录 / 切换账号」时才打开——所以**重载一次页面**它就不会再出现。这里就在
 * 观测到「登录层从待登录变成已认证」时重载一次。
 *
 * 不用合成按键：Esc 只是恰好触发页面自己的关闭路由，不是稳定接口（交接文档
 * 明确不认它当生产修复）。
 *
 * 同一轮还要**先关掉授权弹窗**：登录页自己 `window.open` 一个标签页、再把它导航到
 * 设备登录地址；失败 / 取消路径它会自己 `close()`，**成功路径不关**。那扇窗留着没
 * 有意义（登录已经完成），所以重载前经 `close_mcode_popups` 收掉本次工作台开的
 * 弹窗。命令失败或被 ACL 拒只影响弹窗本身，收尾最多等 300ms 就照常重载。
 *
 * shortcut: 重载会丢页面内存态（没发出去的输入）——登录刚完成时通常没有；真要保住
 * 草稿就改成先把输入框内容存到会话再恢复。
 */
(function () {
  if (window.top !== window.self) {
    return;
  }

  var OVERLAY = '[data-testid="account-login-page-overlay"]';
  var PAGE = '[data-testid="account-login-page"]';
  var PHASE = 'data-webui-account-login-phase';
  var GUARD_KEY = 'xlink.mcode.login-reload-at';
  var GUARD_MS = 60000;
  /** 等关弹窗命令的上限：IPC 没有回程时也不能把登录收尾卡住。 */
  var POPUP_CLOSE_WAIT_MS = 300;

  /** 登录层当前阶段；层不在时返回 null。 */
  function phase() {
    if (!document.querySelector(OVERLAY)) {
      return null;
    }
    var page = document.querySelector(PAGE);
    return page ? page.getAttribute(PHASE) : null;
  }

  /** 同一分钟内只重载一次：页面若自己又打开登录层，不至于变成刷新循环。 */
  function allowed() {
    var now = Date.now();
    var last = 0;
    try {
      last = Number(sessionStorage.getItem(GUARD_KEY) || 0);
    } catch (error) {
      last = 0;
    }
    if (now - last < GUARD_MS) {
      return false;
    }
    try {
      sessionStorage.setItem(GUARD_KEY, String(now));
    } catch (error) {
      // 存不下就靠下面的 reloaded 标记挡住本页内的重复刷新。
    }
    return true;
  }

  /** 关掉本次工作台开的授权弹窗，再执行 `done`；拿不到桥或命令失败也照样 `done`。 */
  function closePopupsThen(done) {
    var finished = false;
    function finish() {
      if (finished) {
        return;
      }
      finished = true;
      done();
    }
    var api = window.__TAURI__ && window.__TAURI__.core;
    if (!api || typeof api.invoke !== 'function') {
      finish();
      return;
    }
    try {
      Promise.resolve(api.invoke('close_mcode_popups', {})).then(finish, finish);
    } catch (error) {
      finish();
      return;
    }
    setTimeout(finish, POPUP_CLOSE_WAIT_MS);
  }

  var reloaded = false;
  var last = null;

  function check() {
    var current = phase();
    // 只在**从别的阶段切过来**时刷新：登录层一打开就是「已登录」时（用户已登录
    // 还去点登录）不刷新，那是页面自己的正常界面。
    if (current === 'authenticated' && last && last !== 'authenticated') {
      if (!reloaded && allowed()) {
        reloaded = true;
        closePopupsThen(function () {
          location.reload();
        });
      }
    }
    last = current;
  }

  function start() {
    if (!document.documentElement) {
      return;
    }
    new MutationObserver(check).observe(document.documentElement, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: [PHASE],
    });
    check();
  }

  if (document.documentElement) {
    start();
  } else {
    document.addEventListener('DOMContentLoaded', start);
  }
})();
