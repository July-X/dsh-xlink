/* 工作台「还没发出去的东西」在重载 / 重建窗口时的存续：一段文字，外加还没发出去的
 * 那几张图。
 *
 * 为什么需要它：工作台页面有两条会把它整个换掉的路径——页面内自愈
 * （harness-health.js 命中槽位装配不变量后 `window.location.reload()`）与壳侧看门狗
 * （reload / recreate）。**两条都不保留任何页面状态**，而用户此刻最可能在做的正是
 * 在输入框里打一段还没发出去的话、或者刚甩进来一张截图。会话不会丢（它在服务端），
 * 丢的只有这些。
 *
 * 为什么经过壳而不是只放 sessionStorage：`recreate` 会换掉整个 webview，新窗口拿
 * 不到旧窗口的 sessionStorage（自愈额度 flag `dsh-harness-slot-recovery` 就是这么
 * 丢的）。草稿比额度重要得多。
 *
 * 存的是输入框里那**一段纯文本**、附件轨道上那**几张 `blob:` 图**，外加当时的页面
 * 地址，不存会话内容、不存任何凭据。图片必须在页面还活着的时候读出来——blob URL
 * 随文档一起消失，**页面上没有任何服务端副本**。
 *
 * 另一半职责是**作废**：用户把话发出去之后（Lexical 程序化清空编辑器，不派发
 * `input`），盘上那份必须删掉——否则下次打开工作台，那条**已经发出去的消息**会
 * 自己坐回输入框（2026-09-30 用户原话：「已经发送的消息，下次打开工作台不应该
 * 再次填充在输入区域」）。所以本页的每次动作都过一次 `stashNow`：有字或有图就存，
 * 两者都空且本页存过就清。
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
  // 多久看一眼「输入框是不是已经空了」。用户按下发送时 Lexical 是**程序化**清空
  // 编辑器的，不派发 `input`——光靠监听看不到「发出去了」，而那一刻正是盘上那份
  // 必须作废的时候（见下面的 pending）。
  var WATCH_INTERVAL_MS = 1000;

  var idleTimer = null;
  var restored = false;
  /* 上一次看到输入框里是什么；null = 页面上一个可见的可编辑元素都没有。监视只
   * 在它**变了**时才动作，否则输入框里一直有字时每秒写一次盘。 */
  var lastSeen = null;
  /* 「这一页里，输入框有过内容（文字或图），而且已经交给壳了」。
   *
   * 它是**清盘**的闸：只删本页自己存过的那一份。恢复失败时放回盘上的草稿不算——
   * 那时输入框正是空的，若也算进去，下一轮监视会把刚放回去的东西删掉，而用户为
   * 「内容丢了」抱怨过一次了。 */
  var pending = false;
  /* 已经取回、正写回去的草稿。放回盘上要**连图一起**放，所以得留着手。
   * 只在「本次恢复确实失败了」时用到，成功之后立刻丢掉。 */
  var takenDraft = null;

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

  /* composer 卡片：编辑器、附件轨道、文件选择框都渲染在它里面。
   *
   * `data-composer-card` 是内核自己渲染的**语义钩子**（实测 0.2.0-rc.2 的工作台
   * DOM 上确实有），不是那种一改版就变的哈希 class 名。找不到就当作「这页没有可
   * 保管的 composer」——**宁可不存，也不要把别处的图当草稿存下来。** */
  function composerCard() {
    return document.querySelector('[data-composer-card]');
  }

  /* 草稿图片：卡片里那排 `blob:` 缩略图。
   *
   * 判据是 **`blob:` 而不是任何 class**：图片在核心里是 `URL.createObjectURL(file)`
   * 的对象 URL，只存在于「还没发出去的草稿」这一种状态；会话历史里的图走的是
   * http(s) 地址（`dsh-api-session-controller` 的媒体引用）。所以 `img[src^=blob:]`
   * 精确地就是「用户放了还没发的那几张」。 */
  function draftImages() {
    var card = composerCard();
    if (!card) return [];
    var nodes = card.querySelectorAll('img[src^="blob:"]');
    var out = [];
    for (var i = 0; i < nodes.length; i += 1) {
      if (isVisible(nodes[i])) out.push(nodes[i]);
    }
    return out;
  }

  function base64Of(buffer) {
    var bytes = new Uint8Array(buffer);
    var chunk = 0x8000;
    var parts = [];
    for (var i = 0; i < bytes.length; i += chunk) {
      parts.push(String.fromCharCode.apply(null, bytes.subarray(i, i + chunk)));
    }
    return window.btoa(parts.join(''));
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

  /* `__TAURI__` 在注入脚本执行的那一刻**不一定已经就绪**——`harness-health.js`
   * 为此专门写了 `scheduleReportRetry`，那份代码是踩过的证据。这里同样不能「拿不到
   * 就当没事」：那等于这一次的草稿静默不落盘，正是用户抱怨的那件事。取不到就
   * 退避重试几次；重试期间输入框里的内容还在，下一次停顿还会再记。 */
  var MAX_TAURI_RETRIES = 4;
  var tauriRetries = 0;

  function stashNow() {
    var api = tauri();
    if (!api) {
      if (tauriRetries >= MAX_TAURI_RETRIES) return;
      tauriRetries += 1;
      window.setTimeout(stashNow, 200 * tauriRetries);
      return;
    }
    tauriRetries = 0;
    // 页面上一个可见的可编辑元素都没有 ⇒ composer 还没装配，或者页面正在崩。
    // **这时候的「空」不是「用户把话发出去了」**：黑屏那一刻很可能正是 composer
    // 消失的时候，在这里清盘等于亲手删掉用户唯一没发出去的那一段。
    if (!editables().length) return;
    var text = currentText();
    if ((text && text.trim()) || draftImages().length) {
      pending = true;
      collectAndStash(text);
      return;
    }
    // 输入框空了，而本页确实存过东西：那句话要么发出去了，要么被用户自己删了。
    // 两种情况都不该再有草稿——留着它，下次打开工作台就会把**已经发出去的那条
    // 消息**填回输入框（2026-09-30 用户原话）。
    if (!pending) return;
    pending = false;
    clearOnShell();
  }

  /* 把图读成 base64 交给壳。**必须在页面还活着的时候做**——blob URL 随文档一起
   * 消失，页面一被换掉这些字节就再也拿不到了。
   *
   * 逐张 `fetch(blobUrl)` 而不是 `img.src`：前者给的是**原始字节**（恢复出来的图
   * 与用户贴进来的一模一样），后者是浏览器按视口缩放 / 色彩管理转码过的显示图。 */
  function collectAndStash(text) {
    var api = tauri();
    if (!api) return;
    var images = draftImages();
    var href = String(window.location && window.location.href || '');
    var payload = text || '';
    if (!images.length) {
      invokeStash(href, payload, []);
      return;
    }
    Promise.all(images.map(readImage)).then(function (read) {
      invokeStash(href, payload, read.filter(Boolean));
    }).catch(function () {
      // 读图失败就退回到「只存文字」：图丢了用户还能知道（轨道上没了），
      // 静默地把一段半截的内容存进去才是更糟的。
      invokeStash(href, payload, []);
    });
  }

  function readImage(el) {
    return window.fetch(el.src).then(function (response) {
      if (!response.ok) return null;
      return response.arrayBuffer().then(function (buffer) {
        if (!buffer || !buffer.byteLength) return null;
        return {
          // alt 是内核给的文件名（`attachment.file.name`），没有时它是一句本地化的
          // 「待发送」文案——那种情况下退回一个由 MIME 推出来的名字。
          name: usableName(el.alt) || nameFromMime(response.headers.get('content-type')),
          mime: normalizeMime(response.headers.get('content-type')),
          data: base64Of(buffer)
        };
      });
    }).catch(function () { return null; });
  }

  function usableName(alt) {
    var name = String(alt || '').trim();
    if (!name || name.length > 80) return '';
    // 本地化文案（中文 / 带空格与标点的句子）不是文件名，回退到 MIME 推出来的。
    if (/[^\x20-\x7e.]/.test(name) || /\s/.test(name)) return '';
    return name;
  }

  function normalizeMime(value) {
    var mime = String(value || '').split(';')[0].trim().toLowerCase();
    // 内核只认这四种（`isImageMediaType`），别的别递上去——它会在采集侧就拒掉。
    return mime === 'image/png' || mime === 'image/jpeg' || mime === 'image/webp' || mime === 'image/gif'
      ? mime
      : 'image/png';
  }

  function nameFromMime(mime) {
    var type = normalizeMime(mime);
    return type === 'image/jpeg' ? 'image.jpg' : type.replace('image/', 'image.');
  }

  function invokeStash(href, text, images) {
    var api = tauri();
    if (!api) return;
    try {
      // 不 await：输入框还在用，抢着等一次 IPC 只会让打字发涩。
      Promise.resolve(api.invoke('stash_harness_draft', {
        href: href,
        text: text,
        images: images
      })).catch(function () { /* 管理面板已关闭等情况，下一次停顿还会再记 */ });
    } catch (error) { /* 同上 */ }
  }

  /* 监视输入框变空。刻意不点「发送」按钮、不监听 Enter：内核换一版就可能换掉
   * class 名与文案，而「可编辑元素里的字没了 / 附件轨道空了」这件事本身与内核的
   * 版本无关。
   *
   * 两条纪律：① **只在内容真的变了时才动作**——否则输入框里一直有字时每秒写一次
   * 盘；② 用户还在打字（停顿判据排着队）时只更新 `lastSeen`，不抢在 600ms 之前
   * 写盘，连续打字仍然合并成一次。
   *
   * 签名里带上**图的张数**：用户甩进来一张图又删掉它、或发出去一条只有图的消息，
   * 文字一个字都没变——只盯文字的话这两种都看不见，草稿就会一直留在盘上。 */
  function watchComposer() {
    if (!editables().length) {
      if (lastSeen === null) return;
      lastSeen = null;
      stashNow();
      return;
    }
    var text = currentText();
    var signature = text + ' ' + draftImages().length;
    if (signature === lastSeen) return;
    lastSeen = signature;
    if (idleTimer) return;
    stashNow();
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
   * 把草稿吞掉——那正是它最该被保住的时候。
   *
   * 多一道前提：**必须有 composer 卡片**（`[data-composer-card]`）。它必须在 `take`
   * **之前**判——取之前没人知道草稿里有没有图，而图回到那排附件轨道上需要这张卡片，
   * 没有落点还去取，取走一张图却没地方放，等于删掉它。没有卡片就整份都不取。 */
  function tryRestore() {
    if (restored) return;
    if (!emptyEditable()) return;
    if (!composerCard()) return;
    var api = tauri();
    if (!api) return;
    restored = true;
    Promise.resolve(api.invoke('take_harness_draft')).then(function (draft) {
      if (!draft) return;
      var images = Array.isArray(draft.images) ? draft.images : [];
      if (!draft.text && !images.length) return;
      // 地址变了说明用户已经换到别的会话 / 页面，把上一处的草稿塞进当前输入框
      // 是帮倒忙——宁可丢掉，也不能把话写到错的地方。
      if (draft.href && String(window.location.href) !== String(draft.href)) return;
      takenDraft = draft;
      var el = emptyEditable();
      if (!el) {
        putBack(draft);
        return;
      }
      // 文字先写：它没有异步部分，能立刻判定成不成。图随后注入。
      var textOk = false;
      if (draft.text) {
        try {
          textOk = fill(el, draft.text);
        } catch (error) {
          textOk = false;
        }
      }
      if (!images.length) {
        if (textOk) {
          takenDraft = null;
          window.__DSH_HARNESS_DRAFT_RESTORED__ = true;
          return;
        }
        // **写不进去就把草稿放回去**。`take` 已经把它从盘上拿走了，这时候不还回去
        // 就等于「恢复失败」直接等于「内容丢失」——而用户为这件事抱怨过一次了。
        // 放回去之后下一次页面加载还会再试（这期间输入框仍然是空的，不会覆盖）。
        putBack(draft);
        return;
      }
      putImages(draft, el, function (imagesOk) {
        if (!imagesOk) {
          if (textOk) {
            // 文字已经回到输入框了，这时再把整份放回去，只会让**下一次空的输入框**
            // 收到一份重复的文字；而那份重复比少一张图更难解释。宁可丢掉图，也
            // 不留一份会自己冒出来的草稿。丢掉这件事要留痕，不是悄悄发生。
            clearOnShell();
            reportPartialRestore(draft);
          } else {
            // 两样都没成 ⇒ 输入框仍然是空的 ⇒ 整份放回是**干净的重试**：下次页面
            // 加载输入框还是空的，不会覆盖用户回来之后自己新写的内容。
            putBack(draft);
          }
          return;
        }
        takenDraft = null;
        window.__DSH_HARNESS_DRAFT_RESTORED__ = true;
      });
    }).catch(function () { /* 管理面板已关闭 */ });
  }

  function putBack(draft) {
    var api = tauri();
    if (!api) return;
    try {
      Promise.resolve(api.invoke('stash_harness_draft', {
        href: String(draft.href || window.location.href),
        text: String(draft.text || ''),
        images: Array.isArray(draft.images) ? draft.images : []
      })).catch(function () { /* 管理面板已关闭，那真没辙了 */ });
    } catch (error) { /* 同上 */ }
  }

  function clearOnShell() {
    var api = tauri();
    if (!api) return;
    try {
      Promise.resolve(api.invoke('clear_harness_draft')).catch(function () { /* 壳已关闭 */ });
    } catch (error) { /* 同上 */ }
  }

  /* 「图没能回去」不是可以悄悄发生的事。页面上没有能给用户看的通道（那是内核的
   * 页面），所以至少留下两处可查的痕迹：控制台（排查时看得到）与页面上的标志
   * （`__DSH_HARNESS_DRAFT_PARTIAL__`，能被别人一眼看出「这次恢复只成了一半」）。 */
  function reportPartialRestore(draft) {
    window.__DSH_HARNESS_DRAFT_PARTIAL__ = true;
    if (window.console && typeof window.console.warn === 'function') {
      window.console.warn(
        '[harness-draft] 文字恢复了，' +
          ((draft.images && draft.images.length) || 0) +
          ' 张图没能回到附件轨道；草稿已清掉，不会重复回填文字。',
      );
    }
  }

  /* 把图片放回附件轨道。
   *
   * **走 paste，不走文件选择框。** 这是实测出来的（2026-09-30，无头 Chromium 连着
   * 正在跑的 0.2.0-rc.2 工作台）：往 `<input type="file">` 塞 `DataTransfer.files`
   * 再派发 `change`，`input.files` 确实被填上了（值是 `C:\fakepath\…`），但内核
   * 一张也没收下——React 的 value tracker 会把合成 change 事件判成「值没变」而丢掉
   * 整个事件。往编辑器派发带 `clipboardData` 的 `paste` 则一次就中：那正是**用户自己
   * 粘贴截图时内核走的同一条路**（Lexical `PASTE_COMMAND` → `intakeFiles` →
   * `createDrafts`），不是我们另造的一条旁路。
   *
   * 返回的是**核对过的结果**：派发成功不等于内核收下了（事件可能根本没被
   * 处理器看见），所以数一遍轨道上的图有没有变多。没变多就是假恢复——那比不恢复
   * 更糟：用户会以为图片还在，一发送就没了。
   *
   * 编辑器元素由调用方**在填文字之前**取好传进来：填完字它就不再「空」了，而
   * `emptyEditable()` 只认空的——在填字之后再去找那个元素，这里就永远拿不到。 */
  function putImages(draft, editor, done) {
    var api = tauri();
    if (!api) { done(false); return; }
    if (!editor || typeof window.ClipboardEvent !== 'function' || typeof window.DataTransfer !== 'function') {
      done(false);
      return;
    }
    var files = [];
    for (var i = 0; i < draft.images.length; i += 1) {
      var image = draft.images[i];
      var bytes = decodeBase64(String(image.data || ''));
      if (!bytes || !bytes.length) continue;
      try {
        files.push(new window.File([bytes], String(image.name || 'image.png'), { type: String(image.mime || 'image/png') }));
      } catch (error) { /* 这张跳过，如实少给一张 */ }
    }
    if (!files.length) { done(false); return; }
    var before = draftImages().length;
    var transfer = new window.DataTransfer();
    for (var j = 0; j < files.length; j += 1) {
      try {
        transfer.items.add(files[j]);
      } catch (error) { /* 同上 */ }
    }
    try {
      var event = new window.ClipboardEvent('paste', {
        clipboardData: transfer,
        bubbles: true,
        cancelable: true
      });
      editor.dispatchEvent(event);
    } catch (error) {
      done(false);
      return;
    }
    // 等一拍再看：内核是同步收下再渲染的，但 React 的一帧要等。
    var waited = 0;
    var check = window.setInterval(function () {
      waited += 100;
      var grew = draftImages().length > before;
      if (grew || waited >= 3000) {
        window.clearInterval(check);
        done(grew);
      }
    }, 100);
  }

  function decodeBase64(data) {
    try {
      var binary = window.atob(data);
      var bytes = new Uint8Array(binary.length);
      for (var i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
      return bytes;
    } catch (error) {
      return null;
    }
  }

  document.addEventListener('input', scheduleStash, true);
  document.addEventListener('change', scheduleStash, true);

  // 页面被换掉的那一刻**不再**指望 IPC 能回程（页面已经没了）；上面那个「停止输入
  // 600ms 就记一次」才是真正兜底的那一层。这里只作为「用户刚好在停顿的边缘被换掉」
  // 的补充，且刻意不阻塞。
  window.addEventListener('beforeunload', function () { stashNow(); });
  window.addEventListener('pagehide', function () { stashNow(); });

  // 监视：这一整页都在跑，直到页面自己被换掉。它存在的唯一理由是「用户发出去了」
  // 这件事不会派发 `input`——而那一秒盘上还留着那句已经送达的消息，下一次打开
  // 工作台它就会自己坐回输入框。
  window.setInterval(watchComposer, WATCH_INTERVAL_MS);

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
