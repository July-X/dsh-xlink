/**
 * dsh-xlink 官网对话引用插件（客户端半边，G1 可行性验证用）。
 *
 * ## 它要证明什么
 *
 * 在**真实内核工作台**里证明：插件自有的 `@` 来源能完成「选择 → 异步读取 →
 * 发送门控」，且过程中不碰 Lexical、不用 DOM 覆盖编辑器、不插隐藏占位符。
 *
 * ## 束格式（内核客户端模块系统的既定方言）
 *
 * `window.__ModuleLoader__.load({id, factory})` 以普通脚本供给，factory 内的
 * `require` 由浏览器模块图解析。**本文件必须自包含**：它是单文件服务，
 * 多一个同级文件就是 404（`plugins/memory/client/client.js:10-13` 记着这条）。
 *
 * **全部声明都在 factory 闭包里**：内核 client-hmr 会在束文件变化时重新求值，
 * 而求值发生在页面的全局词法作用域里——顶层 `const` 第二次求值就是
 * `SyntaxError: Identifier ... has already been declared`，整条 entry 当场死掉，
 * 症状是功能静默消失而界面照常显示「已启用」（`plugins/memory/client/client.js:15-25`）。
 *
 * ## 插入时机：只有一条路（PM 2026-10-20 拍板：主线取「甲」）
 *
 * 内核的 `settle()` **同步**调用 `src.onPick(...)` 并紧接着同步
 * `this.execute(outcome, hit.span)`，**不等 Promise**
 * （`input-trigger/client.js@0.2.1-rc.2:1459-1473`）；`execute()` 只认
 * 普通结果对象的 `claim` / `text` / `insert` 三个键
 * （`input-trigger/client.js@0.2.1-rc.2:1364-1380`）。**所以 `async onPick()` 是错的**：返回 Promise 时
 * `"claim" in outcome` 等判定全为 false，会掉到 `slash/input-insert-reference`
 * 分支并插入 `undefined`。
 *
 * 于是读取只能是**发送时**取的：`onPick` 同步返回一个**引用句柄（pending 态）**，
 * 正文由 `codec.serialize` 在内核的发送路径上取（内核 await 它，见
 * `ui-conversation/client.js@0.2.1-rc.2:15486-15527`）。发送路径上一定有一个
 * 属于本来源的 occurrence，于是内核一定走 `serializeReference` → 读取未完成就
 * **等**、读取失败就**拒**（不会漏发）。
 *
 * ## 措辞纪律：pending 句柄 ≠ 就绪（PM 2026-10-20 拍板第 2 条）
 *
 * pick 时刻插入的是**引用句柄（pending 态）**，对应 P1 状态机里的 `selected`；
 * `ready` 只在 `codec.serialize` 成功那一刻产生，且**只影响模型输入、不回填
 * 编辑器**。因此：
 *
 *  · 面向用户的文案**不得**把这个句柄说成「已就绪」；
 *  · chip 的 `label` 保持中性（就是那条对话的标题），不加「就绪」字样；
 *  · 源侧必须能区分 pending 与 ready——`openReference`（点 chip 打开引用详情）
 *    按 ref 报当前状态。
 *
 * ## 互斥：为什么不能「就绪才插 chip」（违反的是 G1-1，需要上游改 onPick）
 *
 * 计划 §5 G1 的原话是「正文校验+快照保存都成功才插就绪 chip」。这条与
 * 「不得漏发」**互斥**，推导只有两条：
 *
 *  · 发送路径上唯一能拦住「有 chip 但正文未就绪」的钩子是 `codec.serialize`，
 *    而内核**只在草稿里已有本来源 occurrence（即 chip）时**才调用它
 *    （`ui-conversation/client.js@0.2.1-rc.2:15501-15527` 的 occurrences 分支）；
 *  · 于是：chip 在 ⇒ 发送会等/拒（不漏发）；chip 不在 ⇒ 内核走
 *    `occurrences.length===0` 分支，**把纯文本直接发出去**，引用被静默丢弃
 *    （＝漏发，且界面零提示）。
 *
 * 「就绪才插 chip」要求发送时草稿里**没有** chip，必然落进第二条；反过来，
 * 要有 chip 才不漏发，就不能等就绪再插。两边不可兼得——**这不是实现取舍，
 * 是接口限制**。
 *
 * **违反的是哪一条**：要两者同时成立，只能改上游——内核 await `onPick`
 * （或新增 `onPickAsync`），也就是接口缺口 **G1-1**。在那之前，甲形态是
 * 唯一同时满足「不漏发」与「不冒充就绪」的路。
 *
 * ## 将来上游支持 onPick 异步化时，甲形态怎么退让
 *
 * 若 G1-1 落地（内核 await onPick 的返回值），退让顺序是：
 *
 *  1. `onPick` 改成 async：正文校验 + 快照保存成功后才返回 insert；失败时
 *     **不插 chip**——此时草稿里没有 occurrence，内核会走纯文本分支，所以
 *     失败必须同时给出可见提示，否则那一条本身就是漏发；
 *  2. `codec.serialize` 的两条路径（等 / reject）**保留为兜底**：快照落盘了
 *     正文读取仍可能失败，那时仍要拒，而不是把空正文发给模型；
 *  3. chip 的 `label` 继续中性：即便上游让 chip 能表示「就绪」，状态也
 *     **不写进 label**，由详情/状态位承载。
 *
 * 也就是说，甲形态不是「等上游再说」的临时妥协，它是 G1-1 落地后的
 * **可退化形态**：退让只发生在 `onPick` 这一层，`serialize` 的等/拒语义不动。
 *
 * ## 证据基线（引内核行号必须写全版本号 + SHA-256 + 绝对路径）
 *
 * 现役 release 树：`C:\Users\zxx\.dsh-xlink\dsh\desktop\kernels\0.2.1-rc.2`
 * （旧的 `C:\Users\zxx\.dsh\` 是平铺旧树，**行号对不上，不要引用**）：
 *
 *  · `node_modules\@deepseek-ai\dsh-client-ui-input-trigger\lib\client.js`
 *    SHA-256 `4A9605B0A811354ED49495790B331BF4F5198FF41614920D9D0C488E22A0F2AF`
 *    （77121 bytes）——`settle()` `input-trigger/client.js@0.2.1-rc.2:1459-1473`、
 *    `execute()` `input-trigger/client.js@0.2.1-rc.2:1364-1380`、
 *    `owner?.codec` 检查 `input-trigger/client.js@0.2.1-rc.2:1251`、
 *    `registerSource` `input-trigger/client.js@0.2.1-rc.2:1556`
 *  · 同包 `lib\types\types.d.ts`
 *    SHA-256 `2D5B004229C1657BC0C484D1F881E2498FA9F45549589A3F1E40E82C1F637582`
 *    ——`onPick` `types.d.ts@0.2.1-rc.2:161`、`matchEnter?` `types.d.ts@0.2.1-rc.2:174`、
 *    `codec.serialize` `types.d.ts@0.2.1-rc.2:126`、
 *    `candidates` `types.d.ts@0.2.1-rc.2:147`
 *  · `node_modules\@deepseek-ai\dsh-client-ui-conversation\lib\client.js`
 *    SHA-256 `8554738CBEEB5218AD1BFA4773EF094363B066D052A1893D7A96797C5BC33A63`
 *    ——`serializeReference` await `ui-conversation/client.js@0.2.1-rc.2:15511`、
 *    `insertReference` 的 draftRev CAS `ui-conversation/client.js@0.2.1-rc.2:15309-15314`、
 *    `slash/input-insert-reference` 监听 `ui-conversation/client.js@0.2.1-rc.2:15805`
 *  · `node_modules\@deepseek-ai\dsh-client-ui-reference\lib\client.js`
 *    SHA-256 `3D179111C51381733C7D0EC13C60EE4D9444C1DFB474EF28A0127654B8BA28A3`
 *    ——官方 `@` 来源模板：`codec` `ui-reference/client.js@0.2.1-rc.2:362-364`、
 *    `registerSource` `ui-reference/client.js@0.2.1-rc.2:368`
 *  · `node_modules\@deepseek-ai\dsh-client-ui-commands\lib\client.js`
 *    SHA-256 `8582C8FA0442F88BC6D28287011EF33032E69A59DB461F9F00908C1D63B5BF64`
 *    ——`notify(level, text)` `ui-commands/client.js@0.2.1-rc.2:1786`
 *
 * ## 探针
 *
 * `window.__DSH_XLINK_OFFICIAL_CHAT_PROBE__` 是本插件的**自报**：它记录
 * onPick / 读取开始与结束 / chip 插入结果 / 序列化结果 / 通知。
 * **它不是独立证据**——独立证据是「`@` 菜单里真的多了一组」（页面上看得见）
 * 与内核日志里的加载记录。§7 #16 要求的就是「自报不可信」这条纪律。
 *
 * @module xlink-official-chat/client
 */
window.__ModuleLoader__.load({
  id: 'xlink-official-chat',
  factory: () => {
    const PROBE_KEY = '__DSH_XLINK_OFFICIAL_CHAT_PROBE__';
    const CONFIG_KEY = '__DSH_G1_CONFIG__';
    const SOURCE_NAME = 'official-chat';

    /** 固定夹具：不接真实官网、不用真实账号、不含任何私密正文。 */
    const FIXTURES = [
      { id: 'g1-alpha', title: 'G1 固定对话 Alpha', body: '【G1 固定正文 Alpha】第一行固定正文。\n第二行固定正文。' },
      { id: 'g1-beta', title: 'G1 固定对话 Beta', body: '【G1 固定正文 Beta】只有一行。' },
    ];

    /**
     * 探针对象。**跨 HMR 重复求值要保留同一份**：HMR 会重新执行整个 factory，
     * 探针被换掉的话驱动侧会丢掉之前的事件。
     */
    const probe = (globalThis[PROBE_KEY] ??= {
      version: 1,
      events: [],
      env: {},
      counters: {},
    });

    /**
     * 读一次配置：驱动侧可以随时改（延迟、失败、挂住发送）。
     *
     * **没有 `mode`**：PM 2026-10-20 拍板只留一条路（甲），P0 不并行做两个
     * 变体。备选路径与未验证原因记在 `official-chat-feasibility.md`，不在代码里
     * 留一个开关。
     */
    function cfg() {
      const raw = globalThis[CONFIG_KEY] ?? {};
      return {
        readDelayMs: Number.isFinite(raw.readDelayMs) ? raw.readDelayMs : 1500,
        failRead: raw.failRead === true,
        blockSerialize: raw.blockSerialize === true,
      };
    }

    /** 记一条事件。`at` 是相对插件加载时刻的毫秒数。 */
    function record(kind, detail) {
      const t = probe.events.length;
      const entry = { seq: t + 1, kind, ...detail };
      probe.events.push(entry);
      const mark = (globalThis.console?.info ?? (() => {}))('[xlink-official-chat]', kind, detail);
      try {
        mark();
      } catch {
        /* 探针不许因为日志失败而抛 */
      }
      return entry;
    }

    /** 计数：驱动侧用它对「到底跑了几次」做硬断言。 */
    function count(key, delta) {
      probe.counters[key] = (probe.counters[key] ?? 0) + (delta ?? 1);
      return probe.counters[key];
    }

    /**
     * 正在进行的读取：`ref` → `{promise, state, cancel}`。
     *
     * **一个 ref 只有一次读取**：onPick 起读取，serialize 等的是同一条 promise
     * （不是重新读）——「发送时重读」会让「固定版本」这条纪律失效。
     */
    const reads = new Map();

    /**
     * 起一次读取。返回 {promise, cancel}。
     *
     * @param {string} ref - 来源内的 id。
     * @param {string} body - 固定夹具正文。
     * @param {Function} [onSettled] - 读取落定（成功/失败）时的回调。
     *   用来**让用户看到状态变化**（PM 2026-10-20 拍板第 3 条）：内核没有
     *   「更新既有 chip 状态」的接口（chip 是内核自己的草稿节点，插件拿不到
     *   编辑器），所以这条变化只能由内核的 `notify` 承载——不新增强制预览面板。
     */
    function startRead(ref, body, onSettled) {
      const options = cfg();
      let cancel = () => {};
      const promise = new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          if (options.failRead) {
            reject(new Error('G1: 固定读取失败（failRead=1）'));
            return;
          }
          resolve(body);
        }, options.readDelayMs);
        cancel = () => {
          clearTimeout(timer);
          reject(new Error('G1: 读取被取消'));
        };
      });
      const entry = { promise, cancel, state: 'pending' };
      reads.set(ref, entry);
      promise.then(
        () => {
          entry.state = 'ready';
          record('read-ready', { ref, delayMs: options.readDelayMs });
          onSettled?.('ready', ref);
        },
        (error) => {
          entry.state = 'failed';
          record('read-failed', { ref, message: String(error?.message ?? error) });
          onSettled?.('failed', ref);
        },
      );
      return entry;
    }

    /** 驱动侧可调用的取消入口（模拟用户取消读取）。 */
    globalThis.__DSH_G1_CANCEL__ = () => {
      for (const [ref, entry] of reads) {
        if (entry.state === 'pending') {
          record('cancel-requested', { ref });
          entry.cancel();
        }
      }
    };

    /** 解析候选行带回来的 JSON 值。 */
    function parseValue(value) {
      try {
        return JSON.parse(String(value ?? ''));
      } catch {
        return undefined;
      }
    }

    /**
     * 注册 `@` 来源。
     *
     * `onPick` **同步**返回：读取在后台起，不等它。
     */
    function makeSource(ctx, sessions) {
      return {
        trigger: '@',
        name: SOURCE_NAME,
        // 内核自带 reference source 没有 order（=0），本来源排后面。
        order: 20,
        // 自有分组带标题——内核支持「一个 @ 菜单里两个分组，其中一组带标题」。
        showGroupTitle: true,

        /**
         * 候选：**固定夹具**，与真实官网无关。
         *
         * @param session - 当前会话投影（`{sessionId}`）。
         * @param req - `{query, quoted, drilled, signal}`。
         * @returns 候选行数组。
         */
        async candidates(session, req) {
          const query = String(req?.query ?? '').trim();
          const rows = FIXTURES.filter((item) => query === '' || item.title.includes(query));
          record('candidates', { sessionId: session?.sessionId, query, count: rows.length });
          return rows.map((item) => ({
            name: item.title,
            description: '固定夹具 · G1 验证用',
            icon: 'session',
            section: '官方对话',
            value: JSON.stringify({ kind: SOURCE_NAME, id: item.id, title: item.title }),
          }));
        },

        /**
         * 选中后的动作。**同步**（内核不等 Promise）。
         *
         * @param pick - `{candidate, session, span, action, ...}`。
         * @returns `PickOutcome` 或 `undefined`。
         */
        onPick(pick) {
          const value = parseValue(pick?.candidate?.value);
          if (value === undefined || value.kind !== SOURCE_NAME) return undefined;
          const sessionId = pick?.session?.sessionId;
          const fixture = FIXTURES.find((item) => item.id === value.id);
          if (fixture === undefined) return undefined;

          count('onPick');
          record('onPick', {
            id: fixture.id,
            sessionId,
            span: pick?.span ?? null,
          });

          const read = startRead(fixture.id, fixture.body, (state) => {
            // 读取落定 → **让用户看到状态变化**（PM 2026-10-20 拍板第 3 条）。
            // 走内核 `notify`：内核没有「更新既有 chip」的接口，而状态写进
            // label 是纪律明令禁止的（pending 句柄不得冒充就绪）。
            showState(ctx, sessions, sessionId, fixture.id, state);
          });
          reads.set(fixture.id, read);

          // **同步**返回一个引用句柄（pending 态）。内核的 `execute()` 只认
          // `claim` / `text` / `insert` 三个键；返回 Promise 会掉进
          // `slash/input-insert-reference` 分支插 `undefined`。
          //
          // `label` 保持**中性**：它就是那条对话的标题。pending 句柄**不是**
          // 就绪引用——措辞纪律见文件头。
          return {
            insert: {
              source: SOURCE_NAME,
              ref: fixture.id,
              label: fixture.title,
              appearance: 'session',
              clipboardText: fixture.id,
            },
          };
        },

        /**
         * 点 chip 打开「引用详情」：报这个句柄**当前**的状态。
         *
         * 这是「源侧能区分 pending 与 ready」的那条通道（PM 2026-10-20 拍板第 5 条）。
         * 措辞按状态分四种，**pending 不得说成就绪**：
         *
         *  · `pending`——正文还在读，这个句柄尚未就绪；
         *  · `ready`——正文已取到（只影响模型输入，**不回填编辑器**）；
         *  · `failed`——读取失败，这个句柄带不出正文；
         *  · `unknown`——没有对应的读取记录。
         *
         * G1 没有详情面板，用内核的 `notify` 报出来（内核自己的命令源用同一招，
         * `ui-commands/client.js@0.2.1-rc.2:1786`；`notify` 的 level 只有
         * `'info' | 'error'` 两个取值，见
         * `ui-conversation/lib/types/client/contract/input.d.ts@0.2.1-rc.2:194`）。
         *
         * @param session - 会话投影（`{sessionId}`）。
         * @param reference - `{ref, appearance}`。
         * @returns 受理了这次详情请求即 true（内核据此不保留原手势）。
         */
        openReference(session, reference) {
          const ref = String(reference?.ref ?? '');
          const state = stateOf(ref);
          record('open-reference', { ref, state });
          showState(ctx, sessions, session?.sessionId, ref, state);
          return true;
        },

        /**
         * **序列化器必须挂在 `codec` 下**——这是内核 source 契约的一部分，
         * 不是风格问题。
         *
         * 内核发送路径上按 owner 找序列化器：
         * `input-trigger/client.js@0.2.1-rc.2:1251`
         * ```js
         * const owner = this.deps.roster.all().find((s) => s.name === source);
         * if (owner?.codec === void 0) return Promise.reject(new Error(`slash: no serializer for reference source "${source}"`));
         * return owner.codec.serialize(ref, signal);
         * ```
         * 内核自带的 reference source 也是这个形状
         * （`ui-reference/client.js@0.2.1-rc.2:362-364`）。
         *
         * **G1 第一版把 `serialize` 写在 source 顶层**：chip 能插进去、
         * 菜单能出来，但内核**永远不会调用它**——owner.codec 是 undefined，
         * 发送路径直接拒绝。症状是「插件看起来装好了，读取也跑了，就是没有
         * 序列化事件」。
         */
        codec: {
          /**
           * 剪贴板文本（**不是**模型形式）。
           *
           * 内核用它做草稿的持久化/剪贴板投影：chip 在草稿文本里占
           * `clipboardText.length` 个字符（`ui-conversation/client.js@0.2.1-rc.2`
           * 的 draft schema 校验 `draft.text.slice(offset, end) === clipboardText`）。
           */
          clipboardText(ref) {
            return String(ref);
          },

          /**
           * 序列化：**发送路径上的异步闸门**。
           *
           * 内核在发送时 await 它（`ui-conversation/client.js@0.2.1-rc.2:15486-15527`）：
           * 读取未完成 → **等**；读取失败 → reject → 内核把这次发送变成
           * `settleDetachedFailure`（可见错误 + 草稿保留），**不会漏发**。
           *
           * @param ref - chip 带的来源内 id。
           * @param signal - 本次发送的取消信号。
           * @returns 交给模型的文本。
           */
          async serialize(ref, signal) {
            count('serialize');
            const entry = reads.get(String(ref));
            record('serialize-begin', { ref: String(ref), known: entry !== undefined, state: entry?.state });
            if (entry === undefined) {
              throw new Error(`G1：找不到 ref "${String(ref)}" 的读取（chip 早于本次选择？）`);
            }
            if (cfg().blockSerialize) {
              // 人为挂住：模拟「读取一直不回来」。
              await new Promise((_resolve, reject) => {
                signal?.addEventListener?.('abort', () => reject(new Error('G1：发送被取消')));
              });
            }
            try {
              const body = await entry.promise;
              record('serialize-ok', { ref: String(ref) });
              return body;
            } catch (error) {
              record('serialize-failed', { ref: String(ref), message: String(error?.message ?? error) });
              throw error;
            }
          },
        },
      };
    }

    /**
     * 一个 ref 的当前状态：`pending` / `ready` / `failed` / `unknown`。
     *
     * **状态名与 P1 状态机对齐**（`selected → reading → … → ready`）：
     * pick 那一刻是 `selected`（句柄已插、正文未取），读取成功才是 `ready`。
     *
     * @param ref - 来源内的 id。
     * @returns 状态字符串；从未选过的 ref 是 `unknown`。
     */
    function stateOf(ref) {
      const entry = reads.get(String(ref));
      if (entry === undefined) return 'unknown';
      return entry.state;
    }

    /**
     * 把状态报给用户（内核的 `notify`）。
     *
     * **pending 不许说成就绪**——这是措辞纪律，不是文案偏好。
     *
     * @param ctx - 客户端根上下文。
     * @param sessions - sessions 服务（拿会话作用域）。
     * @param sessionId - 会话 id。
     * @param ref - 来源内的 id。
     * @param state - `stateOf` 的结果。
     */
    function showState(ctx, sessions, sessionId, ref, state) {
      const text =
        state === 'ready'
          ? `G1：引用「${ref}」的正文已取到（只影响模型输入，不回填编辑器）`
          : state === 'failed'
            ? `G1：引用「${ref}」的正文读取失败，发送时会拒绝这次提交`
            : state === 'pending'
              ? `G1：引用「${ref}」的正文还在读取中，尚未就绪`
              : `G1：引用「${ref}」没有对应的读取记录`;
      const level = state === 'failed' ? 'error' : 'info';
      try {
        const actx = sessions?.scope?.(sessionId);
        const conversation = actx?.get?.('conversation');
        if (conversation === undefined) {
          record('notify-failed', { sessionId, why: 'no-conversation-service' });
          return;
        }
        conversation.input.for(actx).notify(level, text);
        record('notify', { sessionId, level, text, state });
      } catch (error) {
        record('notify-failed', { sessionId, why: String(error?.message ?? error) });
      }
    }

    /**
     * 插件入口。
     *
     * @param ctx - 客户端根上下文（服务代理）。
     */
    function apply(ctx) {
      const inputTriggers = ctx.get('inputTriggers');
      const sessions = ctx.get('sessions');
      record('apply', { hasInputTriggers: inputTriggers !== undefined, hasSessions: sessions !== undefined });

      // **环境快照**：把「插件到底能拿到哪些服务」记下来。第一轮实跑靠它回答
      // 「inputActions / fresh span 到底可不可达」，而不是靠读接口名推断。
      probe.env = {
        inputTriggers: inputTriggers !== undefined,
        sessions: sessions !== undefined,
        uiSession: probeService(ctx, 'uiSession'),
        uiConversation: probeService(ctx, 'uiConversation'),
        slots: probeService(ctx, 'slots'),
        locale: probeService(ctx, 'locale'),
        remote: probeService(ctx, 'remote'),
        inputTriggerServiceKeys: inputTriggers === undefined ? [] : ownNames(inputTriggers),
        sessionServiceKeys: sessions === undefined ? [] : ownNames(sessions),
      };

      if (inputTriggers === undefined || sessions === undefined) {
        record('apply-aborted', { reason: 'missing-service' });
        return;
      }

      const source = makeSource(ctx, sessions);
      ctx.effect(() => inputTriggers.registerSource(source), 'xlink-official-chat: @ source');
      record('registered', { name: SOURCE_NAME, trigger: '@' });
    }

    /** 记一个服务在不在，以及它的方法名（探测用，不抛）。 */
    function probeService(ctx, name) {
      try {
        const service = ctx.get(name);
        if (service === undefined) return { present: false };
        return { present: true, keys: ownNames(service) };
      } catch (error) {
        return { present: false, error: String(error?.message ?? error) };
      }
    }

    /** 取一个对象自己的键名（跨 realm 安全，不抛）。 */
    function ownNames(value) {
      try {
        const names = new Set();
        let cursor = value;
        let depth = 0;
        while (cursor !== undefined && cursor !== null && depth < 4) {
          for (const key of Object.getOwnPropertyNames(cursor)) names.add(key);
          cursor = Object.getPrototypeOf(cursor);
          depth += 1;
        }
        return [...names].sort();
      } catch {
        return [];
      }
    }

    return { apply, inject: ['inputTriggers', 'sessions'] };
  },
});
