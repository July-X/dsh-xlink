/**
 * Client 入口：在会话头右侧的 `conversation.session.header.utilities` 插槽挂
 * 一枚「本轮记忆 N 条」小徽标——回答「**这个会话最近一次，模型实际看到了哪几条记忆**」。
 * 正文自带主语与单位（不是「记忆 N」）：旁边没有别的字可对照时，「N」必须自己说清是
 * 哪一层的计数。面板里那张「最近注入」卡片是同一份数据的另一个视图（跨会话、可翻）；
 * 这里是会话头上一眼的诚实标签。
 *
 * 束格式是内核客户端模块系统的既定方言：`window.__ModuleLoader__.load({id,
 * factory})` 以普通脚本供给，factory 内 `require` 由浏览器模块图解析（react
 * 来自基础图）。因此本文件**必须自包含**：不能出现顶层 ESM import，也不能
 * `require` 同目录的另一个文件——它是单文件服务在 `/plugins/<id>/client.js`
 * 上的，多一个文件就是 404。与 openai-oauth 那份不同，本插件的 entries 里
 * **没有 `locales/`**，所以下方字典没有孪生副本要同步，改一处即改全部。
 *
 * ## 全部声明都必须放进 factory（2026-10-15 真机实测踩出来的硬约束）
 *
 * 内核的 client-hmr 会在束文件变化时**重新求值**它，而求值发生在页面的全局词法
 * 作用域里：顶层 `const SLOT` 第二次求值就是
 * `SyntaxError: Identifier 'SLOT' has already been declared`，整条 entry 当场死掉
 * ——症状是徽标静默消失、控制台一条 SyntaxError，而面板上「已启用」照常显示。
 * 所以本文件**不写任何顶层 `const` / `let` / `class`**（一个字都不往全局挂）：
 * 字典、样式、常量、助手函数全在 factory 闭包里，factory 每次求值重跑一遍，
 * 作用域天然是新的。`test/smoke.test.mjs` 有一条判据钉住它。
 * 同款坑的对照见 openai-oauth 那张账户卡：它同样把字典挂在顶层，只是那几行是
 * `var` 才没炸——`var` 可重复声明，`const` 不行。新增客户端束时别照抄那个形状。
 *
 * 为什么挂 `.utilities`（`kind: "list"`、`scope: "session"`）：
 *  - `.leading` 是 `scope: "root"`，**拿不到 sessionId**——而「这是哪个会话的
 *    注入」正是这枚徽标的全部含义，所以那条路一开始就被排除；
 *  - `.lineage` / `.corner` 都是 `kind: "single"`，已被内核自己占着；
 *  - `.actions` 是子智能体 / 智能体团队 / 后台任务那一簇，塞进去是串味的；
 *  - `.utilities` 是「打开右侧边栏」那一排的紧邻，正是这类小徽标的落点。
 *    order 取 0：排在 `open-in-app`（-10）与 `schedule-catalog`（-5）之后。
 *
 * ## 那两个 `inject` 不是一回事（照抄之前先读消费方：2026-10-15）
 *
 *  · package.json 里的 `dsh.client.inject` 装的是**包名**（`@deepseek-ai/dsh-client-ui-slots`）：
 *    `dsh-client-modules` 逐个把它们注册在本客户端半边**之前**（`client.js` 里那圈
 *    「Register each injected package … before its consumer」），是依赖/顺序声明。
 *  · `return { apply, inject: ["slots"] }` 里的 `inject` 装的是**服务名**：runner 按它决定
 *    `ctx.slots` 给不给，没声明就当场拒（`ctx.get` 守卫），所以它**必须**写。
 *  · **别顺手把 `"timer"` 也加进去**：runner 的提示说「动态包里没有浏览器 timer 全局，
 *    要 `inject: ['timer']`」，而它对**声明了却拿不到**的服务是 parking 等它
 *    （`waitingFor: … ctx.get(name) === undefined`）——投机加一个当前拿不到的服务，会让整枚
 *    徽标静默不加载。本束走的是组合包路径（普通脚本求值），`setInterval` 实测可用（真机上
 *    轮询真的跑起来了），所以就只声明 `slots`。
 *
 * 数据走桌面壳的 Tauri 命令 `memory_recent_injections`（授权见
 * `src-tauri/capabilities/harness-remote.json`）。**只拿标题与 id**，正文
 * 一个字节都不出壳；`__TAURI__` 没就绪或命令被 ACL 拒时本束静默退化成「没有
 * 徽标」，绝不在页面上留红字——它是装饰，不是报错面。
 *
 * 旁边还有第二枚徽标：**待确认候选数**（`memory_pending_count`，只回一个整数）。
 * 模型用 `memory_propose` 提的候选、自动提取里带冲突 / 取代标记的候选，都要
 * 用户到管理面板「记忆 → 待确认」里裁决——而批准入口只存在于面板，用户不去
 * 就看不见。这枚徽标回答的是「现在有没有东西在等我」：count > 0 才渲染，
 * tooltip 说清去哪里审阅；读不到 / 读失败就当成 0（消失比显示错的数好）。
 * 两枚徽标共用同一条轮询，但各自拉各自的数据：一边被 ACL 拒不影响另一边。
 *
 * 徽标画的是**这个会话最近一次上报**，所以「显示什么」有四种形状，逐条对着壳与插件核过
 * （2026-10-18）：
 *  1. 这个会话还没有过上报（壳刚重启 / 轮询还没轮到它）→ 不渲染；
 *  2. 注入了 ≥1 条 → 徽标「本轮记忆 N 条」，tooltip「最近一次注入：<时刻> · 共 N 条：
 *     <标题…> · 读范围：…」；
 *  3. 检索跑了但**一条都没匹配到**（`hits` 为空、开关仍开着）→ 照常渲染「本轮记忆 0 条」，
 *     tooltip 说「这次检索没有匹配到任何记忆」——**不是**「标题取不到」：插件在结果被采用
 *     时就上报（`host/index.js` 的 note 挂在 `applied.applied` 上），一条都没匹配到也是一次
 *     被采用的结果，所以这条是**可达**的（`applyStep` 在 `enabled=false` 或零命中时都回
 *     `applied: true`）；
 *  4. 会话中途把「使用已批准的记忆」关掉（`enabled === false`）→ 同样渲染，tooltip 说清
 *     「记录时开关是关着的」，并且**不提读范围**（那一轮没有检索结果要解释）。
 * 另一条边界：壳进程重启过 → 环形缓冲是进程内的，重启后没有「最近一次」可说了
 * （面板那张卡片同此，不是徽标独有的缺陷）。
 */
window.__ModuleLoader__.load({
  id: "xlink-memory",
  factory: (require) => {
    const React = require("react");
    const e = React.createElement;

    const SLOT = "conversation.session.header.utilities";
    /** 轮询间隔。徽标是「刚刚发生的事」，5 秒足够跟手，又不至于每秒问一次壳。 */
    const POLL_MS = 5000;

    const STRINGS = {
      zh: {
        // 徽标正文**自带主语与单位**：只写「记忆 2」读不出 2 是什么（库里有几条？这一轮
        // 注入几条？），而它旁边没有别的字可对照。「本轮」= 最近一次上报的那一轮，与
        // tooltip 的「最近一次注入 / 最近一次检索」说的是同一件事。
        badgeText: (count) => `本轮记忆 ${count} 条`,
        badgeTip: "模型在这个会话里实际看到了哪几条记忆（只列标题，不列正文）",
        injectedAt: "最近一次注入：",
        checkedAt: "最近一次检索：",
        hits: (count) => `共 ${count} 条：`,
        hitsJoin: "、",
        none: "这次检索没有匹配到任何记忆：这一回合没有记忆进入会话",
        switchOff: "记录时「使用已批准的记忆」是关着的：这一回合没有记忆进入会话",
        tipJoin: " · ",
        scopeWorkspace: "读范围：本工作区",
        scopeGeneral: "读范围：通用偏好（这一轮没有绑定工作区）",
        // 待确认徽标：自带主语与单位，同上——「待确认 2」读不出 2 是什么。
        pendingBadge: (count) => `待确认候选 ${count} 条`,
        pendingTip: "模型提出的记忆 / 经验候选在等审阅：到管理面板的「记忆 → 待确认」里保存或忽略——批准入口只在那里",
      },
      en: {
        // 复数照英文语法走：`1 memory` / `2 memories`——徽标是常驻可见的一行字，
        // 单复数错了和中文写错别字一样显眼。
        badgeText: (count) => `${count} ${count === 1 ? "memory" : "memories"} this turn`,
        badgeTip: "Which memories the model actually saw in this session (titles only, no bodies)",
        injectedAt: "Last injection: ",
        checkedAt: "Last retrieval: ",
        hits: (count) => `${count} in total: `,
        hitsJoin: ", ",
        none: "That retrieval matched no memories: nothing was added to the session this turn",
        switchOff: "\"Use approved memories\" was off when this was recorded: nothing was added to the session this turn",
        tipJoin: " · ",
        scopeWorkspace: "Read scope: this workspace",
        scopeGeneral: "Read scope: general preferences (no workspace bound)",
        pendingBadge: (count) => `${count} ${count === 1 ? "candidate" : "candidates"} to review`,
        pendingTip: "Memory/experience candidates proposed by the model are waiting for review: accept or dismiss them in the Memory panel (「待确认」) — approval lives only there",
      },
    };
    const L = STRINGS[(globalThis.navigator?.language ?? "zh").toLowerCase().startsWith("zh") ? "zh" : "en"];

    const BADGE_STYLE_ID = "xlink-memory-badge-style";
    const BADGE_CSS = `
.xlink-memory-badge{display:inline-flex;align-items:center;flex:none;height:22px;padding:0 8px;border-radius:var(--dsw-radius-md,6px);border:.5px solid var(--dsw-alias-border-l3,rgba(127,127,127,.35));background:0 0;color:var(--dsw-alias-label-secondary,#8b8f96);font-size:12px;line-height:1;font-weight:400;white-space:nowrap;cursor:default}
`;

    function invokeOrNull() {
      const api = globalThis.window?.__TAURI__?.core;
      return api && typeof api.invoke === "function" ? api.invoke : null;
    }

    /** 样式只注入一次（幂等：重复 apply 不重复追加 <style>）。 */
    function ensureBadgeStyle(documentRef) {
      if (documentRef.getElementById(BADGE_STYLE_ID)) return;
      const style = documentRef.createElement("style");
      style.id = BADGE_STYLE_ID;
      style.textContent = BADGE_CSS;
      documentRef.head.appendChild(style);
    }

    /** `at` 是壳给的 RFC3339（UTC，带 Z），按本机时区显示到秒。 */
    function clockText(at) {
      const parsed = new Date(at);
      return Number.isNaN(parsed.getTime()) ? String(at ?? "") : parsed.toLocaleTimeString();
    }

    /** 把任意实参变成一行能贴进控制台的文字，取不到就说取不到（**不抛**）。 */
    function safeText(value) {
      try {
        if (typeof value === "string") return value;
        return value && typeof value.message === "string" ? value.message : String(value);
      } catch {
        return "（原因取不到）";
      }
    }

    /** 同一条原因只说一次：一条 ACL 拒绝不该每 5 秒往控制台写一行。 */
    let lastComplaint = null;
    function complain(error) {
      const text = safeText(error);
      if (text === lastComplaint) return;
      lastComplaint = text;
      (globalThis.console?.error ?? (() => {}))("[xlink-memory] 读最近注入失败：", text);
    }

    /** 悬停文案。**取不到就退化成最短的那句**——这一段在渲染里，抛出去是整棵会话头
     *  渲染失败（比「没有徽标」严重得多），而它只是装饰。
     *
     *  也不靠换行分段：`title` 提示在 WebView2（Chromium）与 WKWebView 上是否渲染成多行
     *  并不一致，而两个平台都得支持。用 ` · ` 连接时两种渲染都读得通——押注其中一边的
     *  换行行为就是给另一个平台埋一根刺。
     *
     *  壳给的三样东西各自都有话说，所以逐个分开：
     *   · `hits` 是**标题数组**（取不到标题的条目退回 id，原样列出来——那是壳的取舍：
     *     宁可给一个认不出的编号，也不要静默少一条）；
     *   · `hits` 为空**不等于**故障：插件只要结果被采用就上报，一条都没匹配到也是一次
     *     被采用的结果；
     *   · `enabled` 是**上报那一刻**的开关：会话中途关掉开关后这一轮同样会有一条上报，
     *     那时说「没有记忆进入会话」是真话，说「没有匹配到」是假话。 */
    function tooltipFor(note, hits) {
      try {
        const parts = [L.badgeTip];
        if (hits.length > 0) {
          parts.push(`${L.injectedAt}${clockText(note.at)}`);
          parts.push(`${L.hits(hits.length)}${hits.join(L.hitsJoin)}`);
          // workspace 为空串 = 这一轮被按 G15 夹紧成「只读通用偏好」（子智能体的
          // 工作区与父级不同）。分得清这两种，是因为它们回答的问题不一样。
          parts.push(note.workspace ? L.scopeWorkspace : L.scopeGeneral);
        } else {
          parts.push(`${L.checkedAt}${clockText(note.at)}`);
          // `enabled === false` 才说开关；字段缺失（老壳）时按「开着但没匹配到」说，
          // 那是更保守的那一种——不替用户声称他关过一个开关。
          parts.push(note.enabled === false ? L.switchOff : L.none);
          // 开关关着那一轮压根没有检索结果，读范围无从谈起，所以只在没关时说。
          if (note.enabled !== false) {
            parts.push(note.workspace ? L.scopeWorkspace : L.scopeGeneral);
          }
        }
        return parts.join(L.tipJoin);
      } catch (error) {
        return String(L.badgeTip ?? "");
      }
    }

    function MemoryBadge(props) {
      const sessionId = props.sessionId;
      const [note, setNote] = React.useState(null);
      const [pending, setPending] = React.useState(0);
      React.useEffect(() => {
        // 拿不到 sessionId 就什么都不画（见文件头：`.leading` 那类 root 作用域
        // 的插槽会这样）。空返回即「不建 effect」，React 允许。
        if (!sessionId) return undefined;
        let alive = true;
        const refresh = async () => {
          // 轮询里的任何异常都**不许冒出去**：冒出去就是一条 unhandledrejection，
          // 而 harness-health.js 把未处理异常当成工作台故障上报、壳会拿它去重建
          // 工作台窗口（预算 3 次）——一枚装饰性徽标不该有能力让用户的窗口闪三次。
          try {
            const invoke = invokeOrNull();
            // `__TAURI__` 可能晚于本束注入：不另做重试轮，interval 本身就是
            // 重试（下一拍再问一次），代价是最多晚 5 秒出现。
            if (invoke === null) return;
            // 两枚徽标各自拉各自的数据：一边被 ACL 拒 / 读失败只影响自己那枚
            // （complain 同一条原因也只说一次），不许把另一边也拖没了。
            invoke("memory_recent_injections")
              .then((notes) => {
                if (!alive) return;
                const list = Array.isArray(notes) ? notes : [];
                lastComplaint = null;
                // 环形缓冲是**跨会话**的（最多 20 条，新→旧），所以必须按 session 过滤。
                // `note.session` 由 host 侧填 `agent.session.id`，与这里的 sessionId 是
                // 同一个内核会话 id（同一份注册表里的字符串），故按字符串比。
                setNote(list.find((item) => item && String(item.session) === String(sessionId)) ?? null);
              })
              .catch((error) => complain(error));
            invoke("memory_pending_count")
              .then((count) => {
                if (!alive) return;
                // 回来的不是整数（老壳 / 形状漂了）就当成 0：待确认徽标消失，
                // 比显示一个不知道是什么的数好。
                setPending(Number.isInteger(count) ? count : 0);
              })
              .catch((error) => complain(error));
          } catch (error) {
            complain(error);
          }
        };
        refresh();
        const timer = setInterval(refresh, POLL_MS);
        return () => {
          alive = false;
          clearInterval(timer);
        };
      }, [sessionId]);
      const hits = note === null ? [] : Array.isArray(note.hits) ? note.hits : [];
      const noteBadge =
        note === null
          ? null
          : e(
              "span",
              { className: "xlink-memory-badge", title: tooltipFor(note, hits), "data-xlink-memory-badge": "recent-injections" },
              `${L.badgeText(hits.length)}`,
            );
      const pendingBadge =
        pending > 0
          ? e(
              "span",
              { className: "xlink-memory-badge", title: L.pendingTip, "data-xlink-memory-badge": "pending-candidates" },
              L.pendingBadge(pending),
            )
          : null;
      if (noteBadge === null && pendingBadge === null) return null;
      // Fragment：两枚徽标平级并排、共一条轮询同一套清理。返回数组也能排，
      // 但没有 key 的组件级数组在真 React 里每拍刷一条 console 警告——装饰性
      // 徽标不该往控制台灌噪声。
      return e(React.Fragment, null, noteBadge, pendingBadge);
    }

    function apply(ctx) {
      try {
        // 样式走 globalThis.window.document 而不走 ctx：dsh 的插件 ctx 是服务
        // 代理，访问未注册的服务名会直接抛错，而这一步在 activation 里，抛错
        // 就是整条 entry 不激活（2026-10-09 实测 openai-oauth 的现象）。
        const documentRef = globalThis.window?.document;
        if (documentRef?.head) ensureBadgeStyle(documentRef);
      } catch (error) {
        // 注入失败不阻断激活：徽标退化成无边框的一行字。**取文字时也不能抛**——
        // 这一步在 activation 里，抛出去整条 entry 就不激活了。
        (globalThis.console?.error ?? (() => {}))("[xlink-memory] 样式注入失败：", safeText(error));
      }
      ctx.slots.inject(SLOT, () =>
        ctx.slots.register(
          {
            name: SLOT,
            id: "xlink-memory",
            order: 0,
          },
          MemoryBadge,
        ),
      );
    }

    return { apply, inject: ["slots"] };
  },
});
