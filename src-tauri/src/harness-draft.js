/* 工作台「还没发出去的那段话」在重载 / 重建窗口时的存续。
 *
 * 为什么需要它：工作台页面有两条会把它整个换掉的路径——页面内自愈
 * （harness-health.js 命中槽位装配不变量后 `window.location.reload()`）与壳侧看门狗
 * （reload / recreate）。**两条都不保留任何页面状态**，而用户此刻最可能在做的正是
 * 在输入框里打一段还没发出去的话。会话不会丢（它在服务端），丢的只有这一段。
 *
 * 为什么经过壳而不是只放 sessionStorage：`recreate` 会换掉整个 webview，新窗口拿
 * 不到旧窗口的 sessionStorage（自愈额度 flag `dsh-harness-slot-recovery` 就是这么
 * 丢的）。草稿比额度重要得多。
 *
 * 存的是输入框里那**一段纯文本** + 当时的页面地址，不存会话内容、不存任何凭据。
 */
(function () {
  if (window.top !== window.self || window.__DSH_HARNESS_DRAFT__) return;
  window.__DSH_HARNESS_DRAFT__ = true;

  // 停止输入多久之后记一次草稿。取 600ms：足够把连续打字合并成一次写盘，又短到
  // 「打完最后一个字 → 页面被换掉」之间几乎一定已经记过。
  var IDLE_MS = 600;
  // 等输入框出现多久。页面起来后内核的客户端模块要装配，composer 不是立刻就在的。
  var FIND_TIMEOUT_MS = 15000;
  var FIND_INTERVAL_MS = 250;

  var idleTimer = null;
  var restored = false;

  function tauri() {
    var api = window.__TAURI__ && window.__TAURI__.core;
    return api && typeof api.invoke === "function" ? api : null;
  }

  function isVisible(el) {
    if (el.disabled || el.readOnly) return false;
    var rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  /* 可见的、用户可以在里面打字的地方。刻意不认具体选择器：内核换一版就可能换掉
   * class 名，而「一个可见的可编辑元素」这件事比任何选择器都稳定。 */
  function editables() {
    var nodes = document.querySelectorAll('textarea, [contenteditable="true"], [contenteditable=""]');
    var out = [];
    for (var i = 0; i < nodes.length; i += 1) {
      if (isVisible(nodes[i])) out.push(nodes[i]);
    }
    return out;
  }

  function textOf(el) {
    if (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') {
      return String(el.value || '');
    }
    // contenteditable 用 innerText：它就是**用户看见的那段字**，而 Lexical 的内部
    // state 拿不到（内核把编辑器实例挂在元素上，但没有可用的公开读写 API）。
    return String(el.innerText || el.textContent || '');
  }

  /* 读当前正在输入的那段话。优先「用户正在敲的那个元素」——它一定是 composer；
   * 页面刚打开、焦点还在别处时退回到「有内容的那个可编辑元素」。
   *
   * 聚焦元素**也要过可见性**：面板折叠之后那个输入框仍可能是 `activeElement`，
   * 照着它存草稿就会把用户根本看不见的东西当成「正在输入的话」。 */
  function currentText() {
    var active = document.activeElement;
    if (
      active &&
      (active.tagName === 'TEXTAREA' || active.isContentEditable) &&
      isVisible(active)
    ) {
      return textOf(active);
    }
    var list = editables();
    var best = '';
    for (var i = 0; i < list.length; i += 1) {
      var text = textOf(list[i]);
      if (text.length > best.length) best = text;
    }
    return best;
  }

  function stashNow() {
    var api = tauri();
    if (!api) return;
    var text = currentText();
    if (!text || !text.trim()) return;
    try {
      // 不 await：输入框还在用，抢着等一次 IPC 只会让打字发涩。失败也不重试——
      // 下一次停顿还会再记。
      Promise.resolve(api.invoke('stash_harness_draft', {
        href: String(window.location && window.location.href || ''),
        text: text
      })).catch(function () { /* 管理面板已关闭等情况，下一次停顿再说 */ });
    } catch (error) { /* 同上 */ }
  }

  function scheduleStash() {
    if (idleTimer) window.clearTimeout(idleTimer);
    idleTimer = window.setTimeout(function () {
      idleTimer = null;
      stashNow();
    }, IDLE_MS);
  }

  /* 把一段文字写进空着的输入框，**返回它到底有没有真的进去**。
   *
   * 为什么用 `execCommand('insertText')` 而不是直接改 DOM：它触发的是浏览器真实的
   * 输入路径。已装内核 0.2.0-rc.2 的 `dsh-client-ui-conversation/lib/client.js` 里，
   * Lexical 在编辑器根上注册了 `beforeinput` 处理器并按 `inputType` 分支（含
   * `insertText`）——**execCommand 走的正是真实打字走的那一条**，编辑器会据此更新
   * 自己的内部 state。
   *
   * 直接写 `textContent` 则是**「看起来有字、一发送就没」**：那比不恢复更糟，所以
   * 这里不做那种假恢复。写不进去就如实告诉调用方，让它把草稿**放回去**等下一次
   * 页面加载——用户丢过一次的东西，不能因为恢复失败再丢一次。
   *
   * 只有 `textarea` / `input` 才走赋值 + `input` 事件的退路：那两个是普通控件，赋值
   * 就是它们的真实状态。 */
  function fill(el, text) {
    el.focus();
    if (el.tagName === 'TEXTAREA' || el.tagName === 'INPUT') {
      el.value = text;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      return String(el.value || '') === String(text);
    }
    var placed = false;
    try {
      placed = document.execCommand('insertText', false, text);
    } catch (error) {
      placed = false;
    }
    if (!placed) return false;
    // execCommand 返回 true 也可能被编辑器吞掉（beforeinput 被 preventDefault 却没有
    // 真的应用），所以**读回来核对**。只比对开头几个字：编辑器可能规范化空白，逐字
    // 相等会把一次成功的恢复判成失败。
    var landed = String(textOf(el) || '');
    var head = String(text || '').trim().slice(0, 4);
    if (!head) return true;
    return landed.indexOf(head) >= 0;
  }

  /* 找一个**空着**的可编辑元素。取 DOM 顺序最后一个：composer 在页面底部，而页面
   * 里若有别的（搜索框、标题输入），它们在上面。绝不覆盖已有内容——那可能是用户
   * 回来之后自己新敲的。 */
  function emptyEditable() {
    var list = editables();
    for (var i = list.length - 1; i >= 0; i -= 1) {
      if (!textOf(list[i]).trim()) return list[i];
    }
    return null;
  }

  /* 恢复分两步，顺序不能反：先在页面上找到能写的地方，**再**去壳里取草稿。
   * `take_harness_draft` 是读走即删的，顺序反了就会在「页面还没装配好」的那一刻
   * 把草稿吞掉——那正是它最该被保住的时候。 */
  function tryRestore() {
    if (restored) return;
    if (!emptyEditable()) return;
    var api = tauri();
    if (!api) return;
    restored = true;
    Promise.resolve(api.invoke('take_harness_draft')).then(function (draft) {
      if (!draft || !draft.text) return;
      // 地址变了说明用户已经换到别的会话 / 页面，把上一处的草稿塞进当前输入框
      // 是帮倒忙——宁可丢掉，也不能把话写到错的地方。
      if (draft.href && String(window.location.href) !== String(draft.href)) return;
      var el = emptyEditable();
      if (!el) return;
      var ok = false;
      try {
        ok = fill(el, draft.text);
      } catch (error) {
        ok = false;
      }
      if (ok) {
        window.__DSH_HARNESS_DRAFT_RESTORED__ = true;
        return;
      }
      // **写不进去就把草稿放回去**。`take` 已经把它从盘上拿走了，这时候不还回去
      // 就等于「恢复失败」直接等于「内容丢失」——而用户为这件事抱怨过一次了。
      // 放回去之后下一次页面加载还会再试（这期间输入框仍然是空的，不会覆盖）。
      try {
        Promise.resolve(api.invoke('stash_harness_draft', {
          href: String(draft.href || window.location.href),
          text: String(draft.text)
        })).catch(function () { /* 管理面板已关闭，那真没辙了 */ });
      } catch (error) { /* 同上 */ }
    }).catch(function () { /* 管理面板已关闭 */ });
  }

  document.addEventListener('input', scheduleStash, true);
  document.addEventListener('change', scheduleStash, true);

  // 页面被换掉的那一刻**不再**指望 IPC 能回程（页面已经没了）；上面那个「停止输入
  // 600ms 就记一次」才是真正兜底的那一层。这里只作为「用户刚好在停顿的边缘被换掉」
  // 的补充，且刻意不阻塞。
  window.addEventListener('beforeunload', function () { stashNow(); });
  window.addEventListener('pagehide', function () { stashNow(); });

  // 恢复：轮询到 composer 出现为止。内核的客户端模块要装配一段时间。
  var waited = 0;
  var finder = window.setInterval(function () {
    waited += FIND_INTERVAL_MS;
    if (restored || waited > FIND_TIMEOUT_MS) {
      window.clearInterval(finder);
      return;
    }
    tryRestore();
  }, FIND_INTERVAL_MS);
})();
