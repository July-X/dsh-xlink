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
 * ## P1 起：句柄与快照的关系（协议 S-1 / S-3 / S-4）
 *
 * P1 协议冻结稿：`docs/features/official-chat/official-chat-p1-protocol.md`
 * （PM 定，v0.1）。这三条是它的**硬约束**，本束按它实现：
 *
 *  · **HC-1 句柄 → 快照只能靠 `ref`**：内核发送路径只把 occurrence 的 `ref`
 *    交给源侧（`owner.codec.serialize(ref, signal)`，见上面的
 *    `input-trigger/client.js@0.2.1-rc.2:1250-1252`），`insert` 里加新字段
 *    **到不了发送路径**。⇒ 只有一串 id：`ref === snapshotId`。
 *  · **HC-2 `ref` 就是 chip 的可见文本**：内核自带 source 的
 *    `clipboardText: (ref) => ref` ⇒ ref 变长 = chip 变宽 = 用户直接看见。
 *    ⇒ 整串 ≤ 24 可见字符（协议 S-1）。
 *  · **S-3 单一事实源 = 快照文件**：落盘成功后内存里那条读记录**删掉**，
 *    句柄状态由文件派生；内存里只剩尚未落盘的在途态。
 *  · **S-4 状态口径门控**：进入模型的正文**取自落盘文件**——「任何一次请求
 *    发生时，该 occurrence 指向的快照必须已落盘，且落盘内容与模型输入里的正文
 *    逐字节相同」。
 *
 * **这三条不改变 G1 的甲形态**：pick 仍**同步**返回 pending 句柄（G1-1 未闭合），
 * `serialize` 的**等 / 拒**语义不动，label 仍中性，pending 仍不得称就绪
 * （协议 §7「本阶段不动的部分」）。变的是句柄**指向谁**、状态**从哪来**、
 * 正文**从哪取**。
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
 *    （18455 bytes / 465 行）——官方 `@` 来源模板：
 *    `codec` `ui-reference/client.js@0.2.1-rc.2:362-364`、
 *    `registerSource` `ui-reference/client.js@0.2.1-rc.2:368`。
 *    **`ui-reference/client.js@0.2.1-rc.2:363` 是 HC-2 的内核铁证**：
 *    `clipboardText: (ref) => ref`——内核自带
 *    source 的剪贴板文本**就是 ref 本身**，配上
 *    `ui-conversation/client.js@0.2.1-rc.2:4356` 的
 *    `reference.length !== reference.clipboardText.length` ⇒
 *    **ref 的形态与长度 = 用户编辑器里那枚 chip 的文本与宽度**。
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

    /**
     * **客户端命令的通道路径**（协议 §5「命名空间与桥」，v0.4 裁定）。
     *
     * **必须与 host 半边注册的 exact route 全等**——内核是**全等匹配**
     * （`fetchRoutes.get(url.pathname)`，
     * `dsh-client-connection/lib/index.js@0.2.1-rc.2:828`），错一个字符就 404/405。
     *
     * **为什么在 `/api` 命名空间下**：内核 `assertFetchRoute`
     * （同文件 `dsh-client-connection/lib/index.js@0.2.1-rc.2:973-977`）用
     * `endpointFromPath("/api", route.path)` 校验——不在 `/api` 下直接抛。
     *
     * **为什么不再是原来的 `/xlink-official-chat` RPC 通道**：那条路走的是
     * `connection.rpc.handle`，**插件侧走不通**（内核
     * `dsh-client-connection/lib/index.js@0.2.1-rc.2:871` 用属性访问
     * `owner.webServer.register(route)`，属性查找落在内核 connection 插件自己的
     * fiber scope ⇒ 必抛 `cannot get property "webServer" without inject`）。
     * 而浏览器侧内核自带的 `rpc.call` 打的 URL 是
     * `` `${channel}/${endpoint}`.slice(1) ``（相对路径、**不带 `/api`**，
     * `dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1221`）
     * ⇒ 它**也打不到** `/api` 下的 exact route ⇒ 客户端改为**自己发
     * same-origin fetch**（owner 准入仍由内核做）。
     */
    const FETCH_ROUTE_PATH = '/api/xlink-official-chat';

    /**
     * 造一个关联 id（信封的 `rpcId`）。
     *
     * **形态照抄内核**：浏览器侧内核用 `randomUuid()`
     * （`dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1214`）。
     * 这里**优先**用 `crypto.randomUUID()`，没有就退到一个带随机段的字符串
     * ——**只要同一轮里唯一**即可（它只用来把响应关联回请求）。
     *
     * @returns {string}
     */
    function newRpcId() {
      const c = globalThis.crypto;
      if (c !== undefined && c !== null && typeof c.randomUUID === 'function') {
        return c.randomUUID();
      }
      return `r-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
    }

    /**
     * 固定夹具：**不在客户端这一份**。
     *
     * **P1 v0.2 起，菜单内容由 host 半边的 `catalog` 命令给**（协议 §5：
     * 「客户端命令职责：`capabilities` / `catalog` / …」+「客户端传入的会话
     * 标识用于一致性校验，不构成授权」）。
     *
     * **为什么不再留一份**：客户端留一份 = 两份目录真相，两份会漂；而且
     * **id 分配权归 host**（协议 v0.2 S-1）——id 只有 host 分配得出来，客户端
     * 手里没有分配器，留一份夹具也只能渲染标题、点下去必然失败。
     *
     * 夹具本身仍在 host 半边（`host/index.js` 的 `G1_FIXTURES` + `host/commands.js`
     * 的 `createFixtureSource`），**内容逐字相同**——只是**只有一份**。
     */

    // -----------------------------------------------------------------------
    // 协议口径的去处：**不在这里再写一份**
    //
    // 协议 v0.1 时本文件内联过一份「快照 id / 规范化 / 摘要」的实现，后来
    // 发现**它与 host 半边真正在用的口径不同**（短码算法、规范化范围都差
    // 一截），于是成了**死代码**——两份算法同时躺在一个文件里，谁也不知道
    // 哪份是活的。
    //
    // 现在统一成：**协议口径只有一份，在 host 半边**
    // （`host/protocol.js`：快照格式 / 规范化 / 摘要 / 状态机；`host/seq-store.js`：
    // **id 的分配**（协议 v0.2 S-1：分配权归 host，client 不得自造 seq）；
    // `host/index.js` 另有 `SNAPSHOT_ID_RE` 与 `PROVIDER_RE` 两道**路径安全**闸）。
    //
    // **本文件一份口径都不用留**：id 由 host 在 `catalog` 那一步分配好、随
    // 候选行一起回来；正文由 host 在 `capture` 那一步取好并落盘；模型输入的
    // 正文由 host 在 `preview` 那一步**从盘上读回**。客户端只做三件事：
    // 同步返回句柄、等/拒、以及把状态报给用户。
    //
    // **一致性由夹具机检保证**（协议 §6）：`test/protocol.test.mjs` 让 JS 与
    // Rust 各跑一遍**同一组夹具**，断言「同一夹具 → 同一解析结果」——
    // 判的是**两端的数据**，不是代码长得像不像。
    // -----------------------------------------------------------------------

    /**
     * P1 协议（`docs/features/official-chat/official-chat-p1-protocol.md`）的
     * **客户端侧三片**。
     *
     * · **S-1 单一 id 制度**：`ref === snapshotId`。内核只透传 `ref`（HC-1），
     *   所以「句柄 → 快照」的指向只有这一条通道；而 `ref` 的形态与长度就是
     *   用户编辑器里那枚 chip 的文本与宽度（HC-2）→ 整串 ≤ 24 可见字符。
     *   **v0.2 追加：id 的分配权归 host**——本文件**不再自己数序号**
     *   （见下面「id 从哪来」）。
     * · **S-3 单一事实源 = 快照文件**：落盘成功后这条读记录**从内存里删掉**，
     *   句柄的 pending/ready/failed 一律由快照文件派生；内存里只允许留
     *   **尚未落盘**的在途态。
     * · **S-4 状态口径门控**：进入模型的正文**取自落盘文件**——「任何一次请求
     *   发生时，该 occurrence 指向的快照必须已落盘，且落盘内容与模型输入里的
     *   正文逐字节相同」。
     *
     * 快照由 **host 半边**落盘（`host/index.js` 的 `createSnapshotStore`），
     * 根目录是 `<DSH_HOME>/official-chat/`——`DSH_HOME` 由壳给每个实例注入
     * （`src-tauri/src/kernel/kernel_adapter.rs:21-23` 与
     * `src-tauri/src/kernel/kernel_adapter.rs:472`），所以路径
     * 天然按实例隔离，而代码里**不出现任何用户目录**。
     *
     * **协议口径不在本文件**：快照格式、规范化、摘要、状态机、**id 的生成**
     * 都在 `host/protocol.js` 与 `host/seq-store.js`（单一真相）。本文件只留
     * 兜底用的正文拼接口径。
     */

    /**
     * `ref` 整串长度上限（协议 S-1：≤ 24 可见字符 = chip 宽度）。
     *
     * **只用来提醒，不用来判合法**：id 的形态由 host 说了算（host 落盘前会
     * 自己核对形态、provider 段、会话段、以及「这个序号是不是 host 认领
     * 过的」）。客户端这一份只是**超长就当场拒**——chip 宽度是用户看得见的
     * 东西，超了要立刻说，不能等落盘后才发现。
     */
    const MAX_SNAPSHOT_ID_LEN = 24;

    /**
     * ## id 从哪来：**host 分配，客户端只取用**（协议 v0.2 S-1）
     *
     * v0.1 时本文件**自己数序号**（`seqBySession` + `sessionShort` + 拼 id）。
     * v0.2 把它删了，理由是协议原文：
     *
     * > 若 `seq` 由 client 的进程内计数生成，**dev 壳与 release 壳同开、或任一
     * > 进程重启，都可能生成同一个 `snapshotId` → 撞盘**（host 按「同 id 不重写」
     * > 拒 → 用户看到「这次选择没生效」）。⇒ **`seq` 由 host 侧统一分配**
     * > （host 持有并持久化已用序号，**client 不得自造**）；「本实例内单调递增」
     * > 必须跨进程成立。
     *
     * **那 `onPick` 同步怎么办**（内核的 `settle()` 同步调用它，见文件头）：
     * **分配挪到更早的异步一步**——`candidates()` 调 host 的 `catalog`，host
     * 在回目录页时**顺带把 id 分配好**（`host/seq-store.js` 持久化认领，跨进程
     * 排他），客户端把它放进候选行；`onPick` 那一刻只是把那串 id 拿出来当
     * `ref`。**中间没有 await** ⇒ 同步返回的约束不受影响。
     *
     * **顺带消掉一份真相**：客户端那份 `sessionShort`（FNV 变体 + base36）与
     * `host/protocol.js` 的 `makeSessionShort` 是**同一算法的两份拷贝**——本仓
     * 栽过「两份算法同时躺在一个文件里，谁也不知道哪份是活的」。id 归 host
     * 之后，客户端**一份都不用留**。
     */

    /**
     * 取**客户端命令**的调用面（协议 §5 的五个命令）。
     *
     * **通道是内核 `connection` 服务的 exact fetch route**（协议 v0.4 §5）：
     * 客户端**自己发 same-origin fetch** 到 `/api/xlink-official-chat`
     * （浏览器侧 `dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1497`
     * 提供的 `connection` 服务**只**有 `rpc.call`，它的 URL 不在 `/api` 下 ⇒
     * 打不到 exact route，所以**不再用**它）。
     *
     * **为什么不是「host 挂个服务、client 去 get」**：client 半边是**浏览器页面里
     * 的脚本**，host 半边是**内核 Node 进程里的插件**——两棵不同的 cordis 树，
     * 页面树里 `ctx.get('xlink-official-chat')` 永远解析不到（G1 实跑里快照
     * 一路落盘失败，走的就是这条不存在的路）。而 cordis 的
     * `ctx.set(name, value)` 在该属性没被 `provide` 过时**直接抛错**
     * （`cordis/lib/index.js@0.2.1-rc.2:782-789`），所以那条路两头都不通。
     *
     * **取不到就如实说取不到**：取不到时 pick 会**失败**（发送时拒），**不会**
     * 静默退化成「把内存里那份正文发给模型」——那会破坏 S-4 的口径。
     *
     * @param {object} ctx - 客户端根上下文。
     * @returns {object|undefined} 调用面；取不到即 undefined。
     */
    function hostClient(ctx) {
      // **通道 = 内核 `connection` 服务的 exact fetch route**（协议 v0.4 §5）：
      // 客户端**自己发 same-origin fetch**，打到 `/api/xlink-official-chat`
      // （与 host 半边注册的 exact route **全等**）。
      //
      // **不再用浏览器侧的 `connection.rpc.call`**：它的 URL 是
      // `` `${channel}/${endpoint}`.slice(1) ``（相对路径、**不带 `/api`**），
      // 打不到 `/api` 命名空间下的 exact route。
      //
      // **鉴权没有搬到客户端**：owner 准入仍由内核的 shared fetch handler 做
      // （`createSharedFetchHandler`，
      // `dsh-client-connection/lib/index.js@0.2.1-rc.2:820-838`）；浏览器同源
      // 自带 cookie，**客户端不碰鉴权**（协议 §5 显式否决自造安全面）。
      const doFetch =
        typeof globalThis.fetch === 'function' ? globalThis.fetch.bind(globalThis) : undefined;
      if (doFetch === undefined) {
        record('host-service-unavailable', { why: '页面里没有 fetch' });
        return undefined;
      }
      try {
        return {
          /**
           * 调一个客户端命令。
           *
           * @param {string} endpoint - 端点名（协议 §5 的五个）。
           * @param {object} payload - 请求负载。
           * @param {AbortSignal} [signal] - 取消信号。
           * @returns {Promise<object>} 命令的返回值。
           */
          async call(endpoint, payload, signal) {
            const rpcId = newRpcId();
            /** @type {Response} */
            let response;
            try {
              response = await doFetch(FETCH_ROUTE_PATH, {
                method: 'POST',
                headers: { 'content-type': 'application/json' },
                body: JSON.stringify({
                  type: 'client-request',
                  rpcId,
                  method: endpoint,
                  payload,
                }),
                ...signal === void 0 ? {} : { signal },
              });
            } catch (error) {
              // **送不到**（网络/页面被卸载/内核没起）：如实报「不可达」。
              const failure = new Error(
                `host 命令 ${endpoint} 送不到：${String(error?.message ?? error)}`,
              );
              failure.code = 'host_unreachable';
              throw failure;
            }
            if (!response.ok) {
              // **404/405 就是「exact route 没注册上」**——那正是 G1-7 那条上游缺口
              // 在浏览器侧的样子（内核自己的 API Gateway 路由不存在时也是 405）。
              const failure = new Error(
                `transport failure for ${FETCH_ROUTE_PATH}: HTTP ${response.status}`,
              );
              failure.code = 'host_unreachable';
              throw failure;
            }
            /** @type {any} */
            let full;
            try {
              full = await response.json();
            } catch {
              const failure = new Error(`host 命令 ${endpoint} 的响应不是 JSON`);
              failure.code = 'host_error';
              throw failure;
            }
            if (full?.type !== 'server-response' || full?.rpcId !== rpcId) {
              const failure = new Error(`host 命令 ${endpoint} 的信封不对（rpcId 不匹配）`);
              failure.code = 'host_error';
              throw failure;
            }
            const result = full.result;
            if (result?.ok === true) {
              const value = result.value;
              // **命令自己说失败了 ≠ 成功值**（协议 §3 的错误负载）。
              //
              // 内核 RPC 的信封 `ok:true` 只表示「这条命令跑完了」；命令**自己**
              // 的业务结果在 `value.ok` 上。**不把 `value.ok === false` 抛出去
              // 就会漏发**：客户端会拿到一个「失败但没有内容」的返回值，把空正文
              // 当成正文交给模型（协议 S-4/S-5：失效必须**拒**，请求数 0）。
              if (value !== null && typeof value === 'object' && value.ok === false) {
                const failure = new Error(String(value.code ?? 'host_error'));
                failure.code = String(value.code ?? 'host_error');
                failure.detail = value.detail ?? null;
                failure.userNext = String(value.userNext ?? '');
                failure.retryable = value.retryable === true;
                throw failure;
              }
              return value;
            }
            const error = new Error(String(result?.error?.message ?? `host 命令 ${endpoint} 失败`));
            error.code = String(result?.error?.code ?? 'host_error');
            throw error;
          },
        };
      } catch (error) {
        record('host-service-unavailable', { why: String(error?.message ?? error) });
        return undefined;
      }
    }

    /**
     * 快照正文的拼接口径（协议 §1.3：消息之间一个空行）。
     *
     * **为什么这里还有一份**：客户端束**必须自包含**（多一个同级文件就是
     * 404），所以它没法 import `host/protocol.js`。这份只用于「host 没直接回
     * 正文」的兜底；host 的 `preview` 会直接回正文（权威那份在 host 半边）。
     */
    const MESSAGE_SEPARATOR = '\n\n';

    /**
     * 把快照里的消息拼成正文（兜底用）。
     *
     * @param {Array<{text?: string}>} messages - 快照里的消息。
     * @returns {string} 正文。
     */
    function joinMessages(messages) {
      return (Array.isArray(messages) ? messages : [])
        .map((message) => String(message?.text ?? ''))
        .join(MESSAGE_SEPARATOR);
    }

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
     * **在途**的读取：快照 id → `{promise, settled, state, cancel}`。
     *
     * **主键是快照 id，不是对话 id**（协议 S-1）：唯一性粒度 = 一次 pick，
     * 同一条对话选两次 → 两个 id → **第二条不再覆盖第一条**（G1 现状以对话 id
     * 为主键，选两次会互相覆盖，那是 P1 必须修掉的）。
     *
     * **这里只留尚未落盘的条目**（协议 S-3）：落盘成功即 `delete`，此后句柄
     * 状态由快照文件派生。内存里不留「已落盘」的第二份真相。
     *
     * **一个快照 id 只有一次读取**：onPick 起读取，serialize 等的是同一条
     * 链（不是重新读）——「发送时重读」会让「固定版本」这条纪律失效。
     */
    const reads = new Map();

    /**
     * 起一次读取。返回 {promise, cancel}。
     *
     * **状态机**：读取成功**不等于**就绪——P1 协议 S-3 的 `ready` 是「**已落盘**」。
     * 所以这里只把失败落定成 `failed`；成功的那条路留给调用方（onPick）接上
     * 「落盘」这一步，落盘成功才把状态推到 `ready`。
     *
     * **这里不产出正文**：正文**只从落盘文件取**（协议 S-4），模型的输入正文
     * 与盘上的内容必须逐字节相同。这条读取链在 P1 里是**时序闸**（内核的
     * `serialize` 会等它：读取未完成 → 等；读取失败 → 拒），不是正文来源。
     *
     * @param {string} ref - 快照 id（同时就是 `ref`）。
     * @param {Function} [onSettled] - 读取落定（失败）时的回调。
     *   用来**让用户看到状态变化**（PM 2026-10-20 拍板第 3 条）：内核没有
     *   「更新既有 chip 状态」的接口（chip 是内核自己的草稿节点，插件拿不到
     *   编辑器），所以这条变化只能由内核的 `notify` 承载——不新增强制预览面板。
     */
    function startRead(ref, onSettled) {
      const options = cfg();
      let cancel = () => {};
      const promise = new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          if (options.failRead) {
            reject(new Error('G1: 固定读取失败（failRead=1）'));
            return;
          }
          resolve('');
        }, options.readDelayMs);
        cancel = () => {
          clearTimeout(timer);
          reject(new Error('G1: 读取被取消'));
        };
      });
      const entry = { promise, settled: promise, cancel, state: 'pending' };
      reads.set(ref, entry);
      promise.then(
        () => {
          // **不**在这里置 `ready`：就绪 = 已落盘（协议 S-3），落盘由 onPick 接上。
          record('read-ready', { ref, delayMs: options.readDelayMs });
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
     * 「这次用不了」的那一行菜单项。
     *
     * **为什么要有它**：host 不可达时菜单若**空着**，用户看到的是「@ 没有任何
     * 对话」——那是**静默失败**。给一行说明，用户至少知道「不是没有对话，是
     * 引用功能不可用」；点它会弹出可见提示（见 `showUnavailable`）。
     *
     * @param {string} reason - 原因（**已分类**的事实）。
     * @returns 候选行。
     */
    function unavailableRow(reason) {
      return {
        name: '官网引用不可用',
        description: reason,
        icon: 'session',
        section: '官方对话',
        value: JSON.stringify({ kind: SOURCE_NAME, error: reason }),
      };
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
         * 候选：**由 host 半边的 `catalog` 命令给**（协议 §5 的客户端命令之一）。
         *
         * **为什么菜单不再由本地夹具渲染**：
         *   · 协议 §5 说「服务端从已绑定实例、壳模式、实际站点账户推导命名空间；
         *     客户端传入的会话标识用于一致性校验，不构成授权」——目录是**服务端
         *     的东西**；
         *   · v0.2 S-1 把 **id 分配权给了 host**——id 只有 host 分配得出来，
         *     客户端手里没有分配器，留一份本地目录也只能渲染标题、点下去必然
         *     失败（那是「不静默丢内容」的反面教材）。
         *
         * **顺带拿到 id**：`catalog` 的回包里带 `allocations`
         * （`<官网会话 id> → <快照 id>`），host 已经在这一步**持久化认领**了
         * 序号（跨进程排他）。`onPick` 是同步的、问不到 host ⇒ 分配必须发生
         * 在**更早的异步一步**——就是这一步。
         *
         * @param session - 当前会话投影（`{sessionId}`）。
         * @param req - `{query, quoted, drilled, signal}`。
         * @returns 候选行数组。
         */
        async candidates(session, req) {
          const query = String(req?.query ?? '').trim();
          const host = hostClient(ctx);
          if (host === undefined) {
            // **不静默**：菜单里给一行说明（点它会报出原因），而不是空菜单。
            record('catalog-unavailable', { sessionId: session?.sessionId, why: 'host-service-unavailable' });
            return [unavailableRow('host 不可达（页面里没有 fetch，通道没注册上）')];
          }
          let listed;
          try {
            listed = await host.call('catalog', { query });
          } catch (error) {
            record('catalog-failed', {
              sessionId: session?.sessionId,
              query,
              message: String(error?.message ?? error),
            });
            return [unavailableRow('host 的目录命令失败')];
          }
          if (listed?.ok !== true) {
            const code = String(listed?.code ?? '未知码');
            record('catalog-failed', { sessionId: session?.sessionId, query, code });
            return [unavailableRow(`host 的目录命令失败（${code}）`)];
          }
          const page = listed.page ?? {};
          const allocations = listed.allocations ?? {};
          const items = Array.isArray(page.items) ? page.items : [];
          const rows = items.map((item) => ({
            name: String(item?.title ?? ''),
            // **描述是产品面文案**（当前来源是 P1 的固定夹具；P4 接真实站点后
            // 由 host 那边换文案）。目录页模型（协议 §1.2）里没有「描述」这一
            // 字段——加了就得改协议，所以这行文案留在客户端。
            description: '固定夹具 · G1 验证用',
            icon: 'session',
            section: '官方对话',
            // **`snapshotId` 跟着一行走**：pick 那一刻是同步的，客户端没有
            // 第二条路去问 host 要 id（协议 v0.2 S-1）。
            value: JSON.stringify({
              kind: SOURCE_NAME,
              id: String(item?.sessionId ?? ''),
              title: String(item?.title ?? ''),
              snapshotId: String(allocations[String(item?.sessionId ?? '')] ?? ''),
            }),
          }));
          record('candidates', {
            sessionId: session?.sessionId,
            query,
            count: rows.length,
            allocated: rows.filter((row) => JSON.parse(row.value).snapshotId !== '').length,
          });
          return rows;
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
          // **句柄里的会话段是「官网会话 id」，不是 dsh 的会话 id**：官网会话 id
          // 才是服务端取正文时认的东西（协议 §5：客户端传的会话标识只用于
          // 一致性校验，不构成授权）。
          const sourceSessionId = String(value.id ?? '');
          // **id 由 host 分配**（协议 v0.2 S-1），客户端只取用。
          const snapshotId = String(value.snapshotId ?? '');
          if (sourceSessionId === '' || snapshotId === '') {
            // **没有 host 分配的 id 就不形成句柄**：如实失败 + 可见提示。
            // 返回 `undefined` 是安全的——内核的 `execute()` 第一句就是
            // `if (outcome === void 0 || outcome === "handled") return false;`
            // （`input-trigger/client.js@0.2.1-rc.2:1366`），**不会**插出
            // `undefined`，也不会留下半个 chip。
            count('onPick-rejected');
            const reason = String(value.error ?? 'host 没给出可用的快照 id');
            record('onPick-rejected', { id: sourceSessionId, why: reason });
            showUnavailable(ctx, sessions, sessionId, reason);
            return undefined;
          }
          if (snapshotId.length > MAX_SNAPSHOT_ID_LEN) {
            count('onPick-rejected');
            record('onPick-rejected', { id: sourceSessionId, why: 'snapshot-id-too-long', length: snapshotId.length });
            showUnavailable(ctx, sessions, sessionId, 'host 给出的快照 id 超长');
            return undefined;
          }

          count('onPick');
          record('onPick', {
            id: sourceSessionId,
            sessionId,
            span: pick?.span ?? null,
            snapshotId,
          });

          const read = startRead(snapshotId);

          // 读取完成 → **落盘**（协议 S-3：落盘后单一事实源 = 快照文件）。
          // 落盘成功后这条读记录**从内存里删掉**：句柄状态此后由文件派生，
          // 内存里只剩「尚未落盘」的在途态。
          //
          // **正文由服务端取**（协议 §5）：客户端只传会话标识与**host 分配的
          // id**，正文与落盘都在 host 半边——客户端递过去的正文不能成为第二份
          // 真相。
          const settled = read.promise.then(async () => {
            const host = hostClient(ctx);
            if (host === undefined) {
              throw new Error('P1：host 不可达（页面里没有 fetch），这条引用无法落盘（发送时会拒绝）');
            }
            const captured = await host.call('capture', {
              sessionId: sourceSessionId,
              snapshotId,
              requestId: snapshotId,
            });
            record('snapshot-saved', {
              ref: snapshotId,
              contentDigest: captured?.snapshot?.contentDigest,
            });
            reads.delete(snapshotId);
            showState(ctx, sessions, sessionId, snapshotId, 'ready');
            return captured;
          });
          read.settled = settled;
          settled.catch((error) => {
            record('snapshot-failed', { ref: snapshotId, message: String(error?.message ?? error) });
            showState(ctx, sessions, sessionId, snapshotId, 'failed');
          });

          // **同步**返回一个引用句柄（pending 态）。内核的 `execute()` 只认
          // `claim` / `text` / `insert` 三个键；返回 Promise 会掉进
          // `slash/input-insert-reference` 分支插 `undefined`。
          //
          // `ref` 就是快照 id（S-1 单一 id 制度）：内核只透传 `ref`，句柄里
          // 另加字段到不了发送路径。
          //
          // `label` 保持**中性**：它就是那条对话的标题。pending 句柄**不是**
          // 就绪引用——措辞纪律见文件头。
          return {
            insert: {
              source: SOURCE_NAME,
              ref: snapshotId,
              label: String(value.title ?? ''),
              appearance: 'session',
              clipboardText: snapshotId,
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
          const entry = reads.get(ref);
          if (entry !== undefined) {
            // **在途**：内存里只允许有在途态，所以这里同步报的就是在途态。
            const state = entry.state;
            record('open-reference', { ref, state, from: 'inflight' });
            showState(ctx, sessions, session?.sessionId, ref, state);
            return true;
          }
          // **已落盘**：状态由**快照文件**派生（协议 S-3）。读文件是异步的，而
          // 内核的 `openReference` 是**同步**的（`types.d.ts@0.2.1-rc.2:213`
          // 返回 boolean）——所以这里先受理（返回 true），读到之后再报。
          record('open-reference', { ref, state: 'unknown', from: 'snapshot-file' });
          const host = hostClient(ctx);
          if (host === undefined) {
            showState(ctx, sessions, session?.sessionId, ref, 'failed');
            return true;
          }
          // 读文件是异步的，而内核的 `openReference` 是**同步**的
          // （`types.d.ts@0.2.1-rc.2:213` 返回 boolean）——所以这里先受理
          // （返回 true），读到之后再报。
          host.call('preview', { snapshotId: ref }).then(
            (loaded) => {
              const raw = loaded?.snapshot?.captureResult;
              const capture = String(raw?.status ?? raw ?? 'ok');
              showState(
                ctx,
                sessions,
                session?.sessionId,
                ref,
                capture === 'failed' ? 'failed' : 'ready',
              );
            },
            (error) => {
              record('open-reference-failed', { ref, message: String(error?.message ?? error) });
              showState(ctx, sessions, session?.sessionId, ref, 'failed');
            },
          );
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
           * **P1 起多一步（S-4）**：进入模型的正文**取自落盘文件**，不是内存里
           * 那份——「任何一次请求发生时，该 occurrence 指向的快照必须已落盘，
           * 且落盘内容与模型输入里的正文逐字节相同」。顺带把「内存里那份」这个
           * 第二份真相消灭掉（S-3）。
           *
           * @param ref - chip 带的快照 id（= `ref` = `snapshotId`，S-1）。
           * @param signal - 本次发送的取消信号。
           * @returns 交给模型的文本。
           */
          async serialize(ref, signal) {
            count('serialize');
            const id = String(ref);
            const entry = reads.get(id);
            record('serialize-begin', { ref: id, known: entry !== undefined, state: entry?.state });
            if (cfg().blockSerialize) {
              // 人为挂住：模拟「读取一直不回来」。
              await new Promise((_resolve, reject) => {
                signal?.addEventListener?.('abort', () => reject(new Error('G1：发送被取消')));
              });
            }
            // 在途：等它落定（**等 / 拒语义是协议 §7「不动」的部分**）。
            if (entry !== undefined) {
              try {
                await entry.settled;
              } catch (error) {
                record('serialize-failed', { ref: id, message: String(error?.message ?? error) });
                throw error;
              }
            }
            // S-4：正文**只从落盘文件**取。快照缺失 / 摘要不匹配 / 版本不支持
            // 都在 host 半边拒（`snapshot_invalid`），这里原样抛出 → 内核拒发。
            const host = hostClient(ctx);
            if (host === undefined) {
              const error = new Error('P1：host 不可达，这条引用无法进入模型输入（拒）');
              record('serialize-failed', { ref: id, message: String(error.message) });
              throw error;
            }
            let loaded;
            try {
              // `preview` = **按固定 id 读快照**（协议 §5）：host 从盘上读回，
              // 回来的就是那份文件的内容——请求发生时的快照状态由此判定。
              loaded = await host.call('preview', { snapshotId: id });
            } catch (error) {
              record('serialize-failed', { ref: id, message: String(error?.message ?? error) });
              throw error;
            }
            record('serialize-ok', {
              ref: id,
              contentDigest: loaded?.snapshot?.contentDigest,
              fromFile: true,
            });
            // 正文优先用 host 读回来的那份（它就是盘上的内容）；host 没回正文
            // 时按同一条拼接口径兜底（协议 §1.3：消息之间一个空行）。
            return String(loaded?.body ?? joinMessages(loaded?.snapshot?.messages));
          },
        },
      };
    }

    /**
     * 一个 ref 的**在途**状态：`pending` / `failed` / `unknown`。
     *
     * **落盘后的状态不在这里**（协议 S-3）：那条路由快照文件派生，见
     * `openReference` 里读文件那段。内存里只留**尚未落盘**的条目。
     *
     * @param ref - 快照 id。
     * @returns 状态字符串；没有在途记录就是 `unknown`。
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
          ? `G1：引用「${ref}」的正文已取到并落盘（只影响模型输入，不回填编辑器）`
          : state === 'failed'
            ? `G1：引用「${ref}」的正文读取失败，发送时会拒绝这次提交`
            : state === 'pending'
              ? `G1：引用「${ref}」的正文还在读取中，尚未就绪`
              : `G1：引用「${ref}」没有对应的读取记录`;
      const level = state === 'failed' ? 'error' : 'info';
      notifyUser(ctx, sessions, sessionId, level, text, state);
    }

    /**
     * 「这次没能形成引用句柄」的可见提示。
     *
     * **为什么单列一条**：这一支**没有 chip**（返回 `undefined`，内核的
     * `execute()` 直接返回 false，**不会**插出半个 chip）——所以用户不会从
     * 编辑器里看到任何东西。若不提示，这一次选择就是**静默消失**。
     *
     * @param ctx - 客户端根上下文。
     * @param sessions - sessions 服务。
     * @param sessionId - 会话 id。
     * @param reason - 原因（**已分类**的事实，不是异常串）。
     */
    function showUnavailable(ctx, sessions, sessionId, reason) {
      const text = `G1：官网引用这次没有生成（${reason}），重新打开 @ 菜单再试`;
      notifyUser(ctx, sessions, sessionId, 'error', text, 'unavailable');
    }

    /**
     * 把一条提示报给用户（内核的 `notify`）。
     *
     * @param ctx - 客户端根上下文。
     * @param sessions - sessions 服务（拿会话作用域）。
     * @param sessionId - 会话 id。
     * @param level - `info` / `error`。
     * @param text - 提示正文。
     * @param state - 状态标记（探针用）。
     */
    function notifyUser(ctx, sessions, sessionId, level, text, state) {
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
        // P1：host 侧的命令通道在不在（不在 → 引用只能停在 pending，发送时拒）。
        hostReachable: hostClient(ctx) !== undefined,
      };

      if (inputTriggers === undefined || sessions === undefined) {
        record('apply-aborted', { reason: 'missing-service' });
        return;
      }

      // P1：向 host 要一次「能力」快照（协议 §5 的 `capabilities` 命令）。
      //
      // **不再拿它播种本地序号**——协议 v0.2 S-1 把 id 分配权给了 host，客户端
      // **不得自造 seq**。这次调用只做两件事：确认 RPC 通道真的通（探针
      // `hostReachable`），以及把 host 侧的降级标记记下来。
      const host = hostClient(ctx);
      if (host !== undefined) {
        host.call('capabilities', {}).then(
          (caps) => {
            record('capabilities', {
              schemaVersion: caps?.schemaVersion ?? null,
              providerId: caps?.providerId ?? null,
              degraded: caps?.degraded ?? null,
            });
          },
          (error) => {
            record('capabilities-failed', { why: String(error?.message ?? error) });
          },
        );
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
