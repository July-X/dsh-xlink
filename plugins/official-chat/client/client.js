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
 *    （774080 bytes）——`getTextContent()` `ui-conversation/client.js@0.2.1-rc.2:13752-13753`、
 *    `serializeReference` await `ui-conversation/client.js@0.2.1-rc.2:15511`、
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
            // **`detail` 要穿过 RPC 边界**：host 侧 `toRpcError` 把它放在
            // `details.detail`（`plugins/official-chat/host/rpc.js:121-135`）——
            // **界面文案要用的事实只能从这儿拿**（客户端拿不到 host 的内部变量）
            // ⇒ 这里丢掉的话，第 14 码只能出无数字的降级文案。
            error.detail = result?.error?.details?.detail ?? null;
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
     * **本 client 实例已插入的引用 id 列表**（P2 协议 §2.2：client 负责提供
     * 「当前草稿里已选中的引用 id 列表」，**host 不猜草稿**）。
     *
     * **口径说明（待 PM 确认）**：内核**没有**给插件「提交前草稿投影」的入口
     * （`agent/pre-step` 在 Node 进程侧且带的是已展开的 `UserMessage[]`，
     * 浏览器侧的 `codec.serialize` 是**逐 occurrence** 调用、看不到同行其它
     * 引用）⇒ client 只能用「**本实例插入过、且仍认为在草稿里的**」引用 id。
     * 用户在编辑器里删掉某个 chip 之后，这个 id 仍留在本列表里 ⇒
     * **预检偏保守**（多算一点），**不会少报**。
     *
     * **为什么放在内存而不是落盘**：这是**在途态**（草稿的临时视图），
     * 落盘态的事实源是快照文件（协议 S-3）。
     */
    const selectedRefs = new Set();

    /**
     * **可见提示的记录**（P2 协议 §3.2：提示必须「可复查」）。
     *
     * 这里存的是**结构化数据**（不含文案）：kind / 组 / 预算 / 触发时引用的
     * id。UX 方案定稿后，呈现层从这条记录取数据渲染；文案措辞不在本文件定。
     */
    const notices = [];

    /**
     * **核对轮次的串行链**（P2 协议 §3.1：每次新增引用都要核对一次）。
     *
     * **为什么必须串行**：内核 notice 是**单槽**（最新一条覆盖旧条），而提示的
     * 历史是**可复查**的 ⇒ 多轮核对若**并发**，提示的先后就会与「轮次」错位
     * （实测：第二轮的预算提示可能落到第三轮之后）⇒ 用户看到的可能是旧状态，
     * 判据也会读到错轮次的数字。⇒ 这里让每一轮**依次跑完**再跑下一轮。
     *
     * **不合并轮次**：协议 §3.1-1 要求「选中第二个及以后的同组引用时**立即**
     * 出现提示」⇒ 每一轮都得真的核一次（合并会让中间那轮的预算事实丢失）。
     */
    let duplicatesChain = Promise.resolve();

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
        section: 'Web Chat',
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
            return [unavailableRow(catalogUnavailableText('host_unreachable'))];
          }
          let listed;
          try {
            listed = await host.call('catalog', { query });
          } catch (error) {
            record('catalog-failed', {
              sessionId: session?.sessionId,
              query,
              code: String(error?.code ?? ''),
              message: String(error?.message ?? error),
            });
            // ⚠️ **真实路径就在这里**：host 的命令层失败会被 RPC 层翻成外层
            // `ok:false` ⇒ `host.call` 抛**带 `code` 的错** ⇒ ⇒ 码要在这儿取
            // （下面那条 `listed?.ok !== true` 是防御性分支，真实 host 不走它）。
            return [unavailableRow(catalogUnavailableText(String(error?.code ?? '')))];
          }
          if (listed?.ok !== true) {
            const code = String(listed?.code ?? '');
            record('catalog-failed', { sessionId: session?.sessionId, query, code });
            return [unavailableRow(catalogUnavailableText(code))];
          }
          const page = listed.page ?? {};
          const allocations = listed.allocations ?? {};
          const items = Array.isArray(page.items) ? page.items : [];
          // **来源标识**（host 给的 `page.source`）⇒ 界面显示「DeepSeek」还是
          // 「固定夹具」由**事实**决定，不再是一句写死的文案。
          //
          // ⚠️ **拿不到就退旧文案**（旧缓存页没有这个字段）⇒ ⇒ **不假装知道
          // 来源**，也不显示成「未知来源」吓人。
          const sourceLabel = String(page?.source?.label ?? '');
          const sourceKind = String(page?.source?.kind ?? '');
          // **真实源只显示来源名**（izzy 2026-10-21 口径：「DeepSeek 就显示
          // DeepSeek」）⇒ ⇒ **不加后缀**。
          //
          // **为什么不给真实源加「· 官网对话」**：那半句是**同义反复**（插件整个
          // 功能就是官网对话、分组名已经是「Web Chat」）⇒ ⇒ 描述列只留**带新信息**
          // 的那半句；夹具那档的「G1 验证用」带新信息（这是测试数据、别当真）⇒
          // 保留。
          //
          // ⚠️ **拿不到 `source` 就退旧文案**：只有**升级前**写的旧缓存页会这样
          // （目录页缓存 TTL **5 分钟**）⇒ ⇒ 那时的目录页**确实是固定夹具**
          // （真实源目录是 P4 才接上的）⇒ ⇒ 退旧文案是**事实正确**的，不是编的。
          const sourceDescription =
            sourceLabel === ''
              ? '固定夹具 · G1 验证用'
              : sourceKind === 'site'
                ? sourceLabel
                : `${sourceLabel} · G1 验证用`;
          const rows = items.map((item) => ({
            name: String(item?.title ?? ''),
            // **描述是产品面文案**（来源名由 host 的 `page.source.label` 给；
            // 目录页模型（协议 §1.2）里没有「描述」这一字段——加了就得改协议，
            // 所以这行文案留在客户端）。
            description: sourceDescription,
            icon: 'session',
            section: 'Web Chat',
            // **`snapshotId` 跟着一行走**：pick 那一刻是同步的，客户端没有
            // 第二条路去问 host 要 id（协议 v0.2 S-1）。
            value: JSON.stringify({
              kind: SOURCE_NAME,
              // 目录项的**会话 id 键名是 `id`**（协议 P4 §2.4 面二 / §4.3 ② 的
              // 6 个键）—— 旧形态那个 `sessionId` 键**已经换掉**了。
              id: String(item?.id ?? ''),
              title: String(item?.title ?? ''),
              snapshotId: String(allocations[String(item?.id ?? '')] ?? ''),
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
            // **原因分两层，不许互相套整句**（PM 2026-10-21 23:5x 裁定）：
            // · **菜单那行**（`unavailableRow` 的描述）＝ **为什么列不出** ⇒
            //   `value.error` 那串（catalog 的成品文案，如「还没在官网窗口登录
            //   …」）**已经在菜单里**、用户点之前就看得见 ⇒ ⇒ 外层**不重复**它
            //   （PM ②：「没登录」这类原因既不能被盖掉、也不能被复读一遍）。
            // · **外层**（`showUnavailable`）＝ **这次没形成引用句柄 ＋ 下一步**
            //   ⇒ ⇒ 只在**菜单那行没说过原因**时（正常目录行）补一个**短类别
            //   词**（「没取到引用编号」）⇒ ⇒ 不塞整句成品文案（PM ①③）。
            //
            // **原始原因仍逐字进埋点**（`record` 的 `why`）⇒ 排查看得到，界面
            // 看不到。
            const rawReason = value.error === undefined || value.error === null ? '' : String(value.error);
            const reason = rawReason === '' ? 'host 没给出可用的快照 id' : rawReason;
            record('onPick-rejected', { id: sourceSessionId, why: reason });
            if (rawReason === '') {
              // **这一档**：host 的目录项没给出可用的 id ⇒ ⇒ 用户侧唯一能改变
              // 结果的动作是**换一条会话**（另一条目录项可能分配到了 id）⇒ ⇒
              // 动作过得了 `showUnavailable` 上方那道判据 ⇒ ⇒ 给它。
              showUnavailable(ctx, sessions, sessionId, '没取到引用编号', '换一条会话再试');
            } else {
              // **菜单那行已经说了原因、也说了下一步** ⇒ ⇒ 外层**只说事实**、不
              // 给动作、**不复读**菜单那行的下一步（PM 2026-10-21 23:5x 裁定）⇒
              // ⇒ 原因原样进埋点（上面那行 `record`）就够了。
              showUnavailable(ctx, sessions, sessionId, '', '');
            }
            return undefined;
          }
          if (snapshotId.length > MAX_SNAPSHOT_ID_LEN) {
            count('onPick-rejected');
            record('onPick-rejected', { id: sourceSessionId, why: 'snapshot-id-too-long', length: snapshotId.length });
            // **这一档：用户侧没有可执行动作**（id 由 host 分配 ⇒ 真到这档说明
            // host 侧的 id 形态变了 ⇒ 重开菜单不会让它变短）⇒ ⇒ **只说事实、
            // 不给动作**（PM 2026-10-21 23:5x 裁定）⇒ ⇒ 类别词只到「引用编号
            // 超长」为止，后面不接任何动作。
            showUnavailable(ctx, sessions, sessionId, '引用编号超长', '');
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
            // **省略事实**（协议 P1 §1.1 的 `captureResult` ＝
            // `{status, omitted, unsupported}`；`omitted` ＝ `[{kind, count}]`）⇒ ⇒
            // 用来判「只带出了部分正文」那一档（档 D ①）。
            const captureResult = captured?.snapshot?.captureResult ?? {};
            const omitted = Array.isArray(captureResult?.omitted) ? captureResult.omitted : [];
            const omittedCount = omitted.reduce(
              (sum, item) => sum + (Number(item?.count ?? 0) || 0),
              0,
            );
            record('snapshot-saved', {
              ref: snapshotId,
              contentDigest: captured?.snapshot?.contentDigest,
              captureStatus: String(captureResult?.status ?? 'ok'),
            });
            reads.delete(snapshotId);
            // **P2 协议 §3.1-1**：选中一个引用后立刻核对「草稿里已选的引用」
            // 有没有同组（§1.1 五字段全等）⇒ 有则**立即出现可见提示**。
            //
            // **失败不阻塞**（协议 §2.3）：`duplicates` 失败只影响提示，
            // 正文照发（这里的 catch 已在 `checkDuplicates` 内部消化）。
            selectedRefs.add(snapshotId);
            void checkDuplicates(ctx, sessions, sourceSessionId, snapshotId);
            // **成功时不主动弹提示**（PM 2026-10-22 08:4x 裁定）：chip 出现本身
            // 就是信号 ⇒ ⇒ 成功那句「已取到」留给**点开引用详情**时报（`openReference`
            // 那条路）⇒ ⇒ **别挂在这条回调上**。
            //
            // **但「有内容没带进来」必须当场说**（P1 §3 `partial_content` 那格＝
            // 「明示省略事实」）⇒ ⇒ 只在 `status = 'partial'` 且 `omitted` 非空时
            // 出这一条（档 D ①）⇒ ⇒ **全取到时一句都不出**。
            if (captureResult?.status === 'partial' && omittedCount > 0) {
              presentNotice(ctx, sessions, sessionId, {
                kind: 'body-partial',
                level: 'info',
                trigger: snapshotId,
                omitted,
                text: bodyPartialNoticeText(omittedCount),
              });
            }
            return captured;
          });
          read.settled = settled;
          settled.catch((error) => {
            record('snapshot-failed', { ref: snapshotId, message: String(error?.message ?? error) });
            // **第 14 码 `body_budget_exceeded`：出专属文案**（P1 §3 第 14 码）。
            //
            // **为什么单独分流**：其它码落的是那条固定串（「正文读取失败」），
            // 它与 `body_too_large` **完全同文** ⇒ 用户分不出「这一条太长」与
            // 「整条超 256 KB」⇒ 第 14 码有自己的下一步（换更短的正文）。
            //
            // **两条不许同时出**：一句说「太长」、一句说「读取失败」，用户会读到
            // 互相矛盾的两句 ⇒ 这里**二选一**（专属文案优先）。
            //
            // **载体：`info` 档的输入区常驻条**（内核只渲染 `info` 档的常驻条，
            // `error` 档只弹瞬态 toast）⇒ 与 P2 提示同一承载位。
            if (String(error?.code ?? '') === 'body_budget_exceeded') {
              presentNotice(ctx, sessions, sessionId, {
                kind: 'body-budget-exceeded',
                level: 'info',
                trigger: snapshotId,
                code: 'body_budget_exceeded',
                detail: error?.detail ?? null,
                text: bodyBudgetNoticeText(error?.detail ?? null),
              });
              return;
            }
            // **`source_unsupported` ＋ host 那句「正文取得机制尚未坐实」** ⇒ 才
            // 是「正文这一格还没接通」这一档。
            //
            // ⚠️ **为什么不能只看码**：`source_unsupported` 在 host 侧有**七个**
            // 产出点（目录里没有这个会话／真实源取数源没接上／pick 序号用尽／
            // 取数桥没起／捕获源明说不支持该能力／未知端点与异常兜底…）⇒ ⇒ 只看
            // 码 ⇒ 这句「正文还没接通」会**误挂**到别的场景 ⇒ ⇒ 那是**假陈述**
            // （用户会以为正文通道坏了，其实不是）⇒ ⇒ 按 `detail` 的**标记**再断
            // 一道。
            //
            // **失配是安全的**：标记对不上 ⇒ 退回通用失败文案（现状），**不会说错**。
            if (
              String(error?.code ?? '') === 'source_unsupported' &&
              String(error?.detail ?? '').startsWith(BODY_SOURCE_PENDING_MARK)
            ) {
              presentNotice(ctx, sessions, sessionId, {
                kind: 'body-source-pending',
                level: 'info',
                trigger: snapshotId,
                code: 'source_unsupported',
                detail: error?.detail ?? null,
                text: bodySourcePendingText(),
              });
              return;
            }
            // **按码给成品文案**（PM 2026-10-22 08:3x／08:4x 裁定 · 档 B）⇒ ⇒
            // 不再是那条固定串（「正文读取失败，发送时会拒绝这次提交」）⇒ ⇒ 每一
            // 档说清「发生了什么 ＋ 你能做什么」⇒ ⇒ **动作都过判据**（见
            // `bodyFetchFailedText` 上方那段）。
            //
            // **载体：`error` 档的瞬态 toast**：这一档是**失败**（正文没取到 ⇒ 发送
            // 会被拒）⇒ ⇒ 必须显眼 ⇒ ⇒ 降级成 `info` 档会让人以为还能发
            // （端侧用例就钉着这条：失败提示必须是 error 级）。
            presentNotice(ctx, sessions, sessionId, {
              kind: 'body-fetch-failed',
              level: 'error',
              trigger: snapshotId,
              code: String(error?.code ?? ''),
              detail: error?.detail ?? null,
              text: bodyFetchFailedText(error?.code ?? ''),
            });
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
                record('serialize-failed', {
                  ref: id,
                  message: String(error?.message ?? error),
                  code: String(error?.code ?? ''),
                });
                // **裸码不许上屏**：host 侧 `toRpcError` 把**协议码**放进 `message`
                // （`plugins/official-chat/host/rpc.js:114-117` 造的
                // `code：userNext`）⇒ ⇒ 原样抛出的话，内核顶部横幅显示的就是
                // `source_unsupported：换一个来源` 这种裸码 ⇒ ⇒ 这里换成
                // **成品文案**（`serializeFailure`）。
                //
                // **原始 message 不丢**：它在上面的 `record` 里留着（排查看用），
                // 只是不再拿它上屏。
                throw serializeFailure(error);
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
              record('serialize-failed', {
                ref: id,
                message: String(error?.message ?? error),
                code: String(error?.code ?? ''),
              });
              // **同上**：host 的裸码不许进内核的横幅 ⇒ ⇒ 换成**成品文案**。
              throw serializeFailure(error);
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
    /**
     * **P2 提示的承载位接口**（协议 §3.2：可见 + 可复查 + 落在输入区/引用处）。
     *
     * **承载位 = 内核输入区的 notice**（`conversation.input.for(actx).notify`；
     * 内核 `dsh-client-ui-conversation/lib/types/client/input/facade.d.ts@0.2.1-rc.2:268`
     * 「Surface a notice from outside the machine」⇒ 落在该会话的 composer 上，
     * **不是浏览器 toast**；同包
     * `dsh-client-ui-conversation/lib/types/client/contract/input.d.ts@0.2.1-rc.2:188-190`
     * 明写它按会话路由）。
     *
     * **数据与文案分离**：`notice` 是**结构化数据**（kind / 组 / 预算 / 触发 id），
     * 本函数把它**记进 `notices`**（内存里可复查）并**原样记进埋点**（探针与
     * 判据能读到全量数据），措辞是**UX 定稿的成品文案**（协议 v0.6 §3.2 收录
     * 原文，实现侧照抄）。
     *
     * **承载机制的事实**（协议 v0.5 §3.2）：内核 notice 是**单槽**，而且内核
     * **随时可能发一条自己的 error notice 把我们这条顶掉** ⇒ ① 每条提示的文案
     * 必须**自包含全部事实**（组条数 ＋ 重复字节 ＋ 预算字节 ＋ 超限字节）；
     * ② 本层**不依赖**内核 toast / notice 的出现时机与顺序来表达我们的状态
     * （状态另有下面这份 `notices` 记录，可复查）；③ **不假设提示会一直在**
     * ——用户**再动一次引用**就会重新触发一轮核对、重新发出一条提示 ⇒ 提示
     * 是**可复现**的。
     *
     * @param {object} ctx - 客户端根上下文。
     * @param {object} sessions - sessions 服务。
     * @param {string} sessionId - 提示落在哪个会话的输入区。
     * @param {object} notice - 结构化提示数据（含 `text` 成品文案）。
     */
    function presentNotice(ctx, sessions, sessionId, notice) {
      notices.push(notice);
      record('notice', { ...notice });
      notifyUser(ctx, sessions, sessionId, notice.level ?? 'info', notice.text ?? '', notice.kind);
    }

    /**
     * P2 提示的**成品文案**（UX 定稿 2026-10-21 · 协议 v0.6 §3.2 收录原文）。
     *
     * **为什么只有一份文案**：协议 §3.1（v0.4 起）——内核 notice 是**单槽**、
     * 最新一条覆盖旧条 ⇒「两个触发点各自独立」＝**两条都要发**，且**后发那条
     * 必须自包含全部事实** ⇒ 两条说的是同一件事 ⇒ **逐字相同**（协议 v0.6 §3.2
     * 「两条逐字相同」）。
     *
     * **数字不加千分位**（协议 v0.6 §3.2）——判据断的是**原值**在不在。
     *
     * **多组时逐组列**（协议 §3.2「多组形态」· PM 01:3x）：「同组 N 条内容完全相同」
     * 必须按**组**读才为真（组内相同、组间不同）⇒ 协议 §3.2「数字来源」里的
     * `members.length` 与 `redundantBytes` 都是**组内**的量 ⇒ 混成一句（「同组 4 条」）
     * 是假陈述 ⇒ **一组一句**。且多组时**先说组数、再逐组编号**（「N 组重复：第 i 组
     * …」）——两个「同组 N 条」并排，读者会本能归到同一个组上 ⇒ 读成「同一组既是
     * 3 条又是 2 条」⇒ 表面矛盾（不是假陈述，但是真实的可读性缺陷）。
     *
     * @param {object[]} groups - 协议 §2.2 的 `groups`，**顺序由 host 定**
     *   （host 按 `groupKey` 升序产出；client **照单渲染**，不自己再排）。
     * @param {object} budget - 协议 §2.2 的 `budget`。
     * @returns {string} 成品文案。
     */
    function duplicatesNoticeText(groups, budget) {
      const multi = groups.length > 1;
      const groupParts = groups.map((group, index) => {
        const members = Array.isArray(group?.members) ? group.members.length : 0;
        const redundant = Number(group?.redundantBytes ?? 0);
        const same = `${members} 条内容完全相同，重复可省 ${redundant} 字节`;
        return multi ? `第 ${index + 1} 组 ${same}` : `同组 ${same}`;
      });
      const groupPart = multi
        ? `${groups.length} 组重复：${groupParts.join('；')}`
        : groupParts.join('');
      const projected = Number(budget?.projectedBytes ?? 0);
      const limit = Number(budget?.limitBytes ?? 0);
      const budgetPart =
        budget?.wouldExceed === true
          ? `引用正文 ${projected} 字节，超出单次消息预算 ${limit} 字节` +
            `（超 ${Number(budget?.exceedByBytes ?? 0)} 字节）`
          : `引用正文 ${projected} 字节，未超出单次消息预算 ${limit} 字节`;
      return `本次核对：${[groupPart, budgetPart].filter((part) => part !== '').join('；')}。`;
    }

    /**
     * 从 host 的 `detail` 里解析第 14 码的三个数。
     *
     * **契约形态**（PM 2026-10-21 裁，host 侧逐字产出：`host/protocol.js` 的
     * `resolveCaptureCase` 与 `host/index.js` 的 `writeSnapshot` 两处同构）：
     *
     *     单条正文超单条正文预算：正文 {bytes} 字节，超出单条正文预算 {budget} 字节（超 {over} 字节）
     *
     * **自洽性也断**：`over` 必须等于 `bytes - budget`（host 就是这么算的）⇒
     * 对不上说明这串 `detail` 不是按契约来的 ⇒ **宁可退化成无数字文案**，
     * 也不给用户看互相矛盾的数字。
     *
     * @param {string|null} detail - host 回的 `detail`。
     * @returns {{bytes: number, budget: number, over: number}|null} 三个数；
     *   取不到就是 null（调用方出降级文案）。
     */
    function parseBodyBudgetDetail(detail) {
      const matched = /正文 (\d+) 字节，超出单条正文预算 (\d+) 字节（超 (\d+) 字节）/.exec(
        String(detail ?? ''),
      );
      if (matched === null) return null;
      const bytes = Number(matched[1]);
      const budget = Number(matched[2]);
      const over = Number(matched[3]);
      if (!Number.isSafeInteger(bytes) || !Number.isSafeInteger(budget) || !Number.isSafeInteger(over)) {
        return null;
      }
      if (over !== bytes - budget) return null;
      return { bytes, budget, over };
    }

    /**
     * 第 14 码 `body_budget_exceeded` 的**成品文案**（UX 定稿 2026-10-21）。
     *
     * **为什么必须有专属文案**：其它码落的那条固定串（「引用「…」的正文读取
     * 失败，发送时会拒绝这次提交」）与 `body_too_large` **完全同文** ⇒ 用户
     * 分不出「这一条太长」与「整条超 256 KB」，而这两件事的**下一步不同**
     * （前者换更短的正文，后者换更小的会话）。
     *
     * **数字从 `detail` 解析**（三个数：正文字节 / 预算字节 / 超出量）——**取
     * 不到就退化成无数字文案**，**不编数字**。
     *
     * **措辞纪律**：不写「别的引用不受影响」——**已裁定：该语义在第 14 码下成立**
     * （各引用各自落盘、互不牵连），但**文案按 PM 裁定继续回避**：用户此刻只有
     * 一个下一步＝移除这条太长的引用、换一段更短的正文；写「别的引用不受影响」
     * 会让人以为「其余照发就行」，而正确的心智是「这条没进来，你得处理它」。
     * 也不写「重试／再试一次」（这个码**不可重试**）、不显示错误码（码只进
     * `detail`）。
     *
     * @param {string|null} detail - host 回的 `detail`。
     * @returns {string} 成品文案。
     */
    function bodyBudgetNoticeText(detail) {
      const parsed = parseBodyBudgetDetail(detail);
      if (parsed === null) {
        return '这条引用太长，请移除它，换一段更短的正文。';
      }
      return (
        `这条引用太长：正文 ${parsed.bytes} 字节，` +
        `超出单条正文预算 ${parsed.budget} 字节（超 ${parsed.over} 字节）。` +
        '请移除这条引用，换一段更短的正文。'
      );
    }

    /**
     * host 那句 detail 的**机器标记**（`host/protocol.js` 的
     * `BODY_SOURCE_PENDING_DETAIL` 的**句首**）⇒ 客户端靠它认出「这一格还没
     * 接通」那一档。
     *
     * **为什么不是中文抄本**：中文**会随文案改** ⇒ 抄本会漂 ⇒ ⇒ host 侧已在
     * 句首加了**稳定的 ASCII 标记**（实现线 `6caa8ee2` 落的：
     * `body-source-pending:正文取得机制尚未坐实（…）`）⇒ ⇒ 这里认**标记** ⇒
     * **不再依赖任何一句中文措辞**（那条中文说明照旧留给日志与排查看）。
     *
     * **失配仍然是安全的**：标记对不上 ⇒ 退回通用失败文案，**不会说错**。
     */
    const BODY_SOURCE_PENDING_MARK = 'body-source-pending:';

    /**
     * **正文这一格还没接通**时的专属文案（`source_unsupported` ＋ 上面那个标记）。
     *
     * ## 为什么单独一条
     *
     * 目录页已经能列**站点的真实会话**，但正文取得机制**还没做**（协议 §4.4
     * 「候选机制·未成立」）⇒ ⇒ 这不是「读取失败」那种**像 bug 的错**，而是
     * **这一格还没通** ⇒ ⇒ 说成「读取失败」会让用户以为坏了。
     *
     * ## 口径（UX 定稿 2026-10-21）
     *
     * · **不装成成功**：明说「带不出正文」。
     * · **不做时间承诺**：**不写「暂时」**——「暂时」暗示「等会儿会好」，而这一格
     *   在前置（G4-20）坐实前**不会自己好** ⇒ ⇒ 只陈述现状。
     * · **不用内部黑话**：不写「这一格」（那是内部说法，用户不知道指什么）⇒ ⇒
     *   说「正文读取还没接通」。
     * · **不拼 host 的 `detail`**：那串是**给日志与排查看的线索**（含协议节号与
     *   前置编号）⇒ ⇒ 界面文案自己写完整、自包含的事实 ⇒ ⇒ 也不与本句重复说
     *   同一件事。
     *
     * @returns {string} 成品文案。
     */
    function bodySourcePendingText() {
      return '这条引用带不出正文：目录页能列出会话，但正文读取还没接通。';
    }

    /**
     * **「正文取不到」按码给的成品文案**（PM 2026-10-22 08:3x／08:4x 裁定）。
     *
     * ## 码从哪来（客户端只看得到**码**，看不到成因）
     *
     * 实现侧把正文那条链的失败**归一到闭集 14 码**（`src-tauri/src/official_chat/
     * session_structure.rs` 的 `CaptureFailure::capture_code()`）：回程失败／超时
     * ⇒ `timeout`｜站内码 ⇒ `catalog_failed`｜形状漂移 ⇒ `schema_changed`｜站点
     * 返回空 ⇒ `catalog_empty`｜本机没这条通路（非 Windows）⇒ `source_unsupported`。
     * ⇒ ⇒ **客户端按码分流**（成因在 `detail` 里，那是给日志排查看的线索，**不
     * 进界面**）。
     *
     * ## 每一档的**动作**都过了一道判据
     *
     * **判据**：用户做完那个动作，**真能改变这一档的结果吗？** ⇒ 答不出「能」
     * ⇒ **不许给那个动作**，宁可**只说事实**。
     *
     * · `timeout` ⇒ **过**（再选一次**可能**让站点这次就返回了）⇒ 给「再选一次
     *   这条会话」。
     * · `schema_changed` ⇒ **过**（与码表 `schema_changed` 那格「升级插件」同
     *   方向 ⇒ P1 §3 第 ⑥ 条允许细化）⇒ 给「升级插件后再引用」。
     * · `catalog_failed`（站内码）⇒ **不过**（站内码的成因与出路都不可知）
     *   ⇒ **只说事实**。
     * · `catalog_empty` ⇒ **不过** ⇒ **只说事实**。
     * · `body_too_large` ⇒ **过**（换一条正文更短的会话 ⇒ 可能不再超限；与码表
     *   那格「换更小的会话」同方向）⇒ 给「换一条短一些的会话再试」。
     *
     * ## 为什么「传输层失败」与「timeout」**合成一句**
     *
     * 实现侧把「回程失败」与「等超时」**都落 `timeout`** ⇒ ⇒ 客户端**分不出**
     * ⇒ ⇒ 句子用**不含未经证实限定**的那句（**不写「在时间内」**——那只是
     * timeout 的一个子情形）⇒ ⇒ 动作相同（再选一次）⇒ ⇒ 用户可感一致 ⇒ ⇒ 这一
     * 档是**一档**，不是两档。
     *
     * ## 兜底
     *
     * **认不出的码 ⇒ 只说事实、不指原因、不给动作** ⇒ ⇒ 宁可信息少，也不要**说错
     * 原因**（把「不是这个原因」说成「是这个原因」比不说更坏）。
     *
     * @param {string} code - 协议码（认不出的走兜底）。
     * @returns {string} 成品文案。
     */
    function bodyFetchFailedText(code) {
      const known = String(code ?? '');
      // **形状漂移**：官网给的东西读不懂 ⇒ ⇒ 下一步是升级插件。
      if (known === 'schema_changed') {
        return '这条引用带不出正文：官网返回的内容读不懂，升级插件后才能再引用。';
      }
      // **站内码**：官网自己拒绝了这次读取 ⇒ ⇒ 成因与出路都不可知 ⇒ 只说事实。
      if (known === 'catalog_failed') {
        return '这条引用带不出正文：官网拒绝了这次读取。';
      }
      // **站点返回空正文**：接口正常，但这条会话没有消息 ⇒ ⇒ 只说事实。
      if (known === 'catalog_empty') {
        return '这条引用带不出正文：这条会话没有内容。';
      }
      // **正文超限**（落盘层那档）：⇒ ⇒ 换一条正文更短的会话。
      if (known === 'body_too_large') {
        return '这条引用带不出正文：这条会话的正文太长，换一条短一些的会话再试。';
      }
      // **内容被省略**（若它以错误码出现）：⇒ ⇒ 说清「只带出了部分」。
      if (known === 'partial_content') {
        return '这条引用只带出了部分正文，另有内容没能带进来。';
      }
      // **没取到**（回程失败／超时都落这一档）⇒ ⇒ 动作过判据 ⇒ 给「再选一次」。
      if (known === 'timeout') {
        return '这条引用带不出正文：官网没有返回这条会话的正文，可以再选一次这条会话。';
      }
      // **兜底**：只说事实 ⇒ 不指原因、不给动作。
      return '这条引用带不出正文。';
    }

    /**
     * **「只带出了部分正文」的成品文案**（PM 2026-10-22 08:3x 裁定 · 档 D ①）。
     *
     * ## 触发条件（**别提前说「分页没取全」**）
     *
     * **只在** `captureResult.status === 'partial'` **且** `omitted` **非空**时用
     * ⇒ ⇒ 这一档是**现成可判**的（协议 P1 §1.1 的 `captureResult` ＝
     * `{status, omitted, unsupported}`；`omitted` ＝ `[{kind, count}]`）⇒ ⇒
     * **不需要等任何判据**。
     *
     * ⚠️ **但「分页没取全」是另一回事**（一次深链发多条同 id 请求、只等一条）
     * ⇒ ⇒ 那个**判不出「还有没有更多」** ⇒ ⇒ **不许说「一部分」**（那是猜测）
     * ⇒ ⇒ 协议 §9 **G4-23** 把它登记成缺口。
     *
     * ## 口径
     *
     * · **明示省略事实**（P1 §3 `partial_content` 那格＝「明示省略事实」）⇒ ⇒
     *   **带上条数**（`omitted` 里的 `count` 求和）⇒ ⇒ 用户知道缺了多少。
     * · **先说「N 条内容」**，**不点明类型**（现在的 kind 是站点的内容类型，
     *   搬进界面＝把站点概念泄给用户）⇒ ⇒ 将来 kind 多了再按类型细分。
     * · **不许说「全部／都取到」**（同上 G4-23）⇒ ⇒ 这句也只说「能带的部分」。
     * · **不给动作**：用户在界面里**没有**能改变这一档的动作（重选一次大概率
     *   还是同样的截断）⇒ ⇒ 按 P1 §3 第 ⑦ 条，**只说事实**。
     *
     * @param {number} count - 省略了多少条（`omitted` 的 `count` 之和）。
     * @returns {string} 成品文案。
     */
    function bodyPartialNoticeText(count) {
      return `这条引用带出了这条会话能带的部分正文，另有 ${count} 条内容没能带进来。`;
    }

    /**
     * **serialize 失败时抛给内核的错误**（**裸码绝不上屏**）。
     *
     * ## 为什么要有这一步
     *
     * host 侧的错误信封把**协议码**直接放进 `message`
     * （`plugins/official-chat/host/rpc.js:114-117`：
     * `userNext === '' ? code : \`${code}：${userNext}\``）
     * ⇒ ⇒ 原样抛出时，**内核顶部横幅显示的就是裸码**（内核取 `error.message`，
     * 见 `ui-conversation/client.js@0.2.1-rc.2:15525`）⇒ ⇒ 用户读到的是
     * `source_unsupported：换一个来源` 这种**没法行动**的串。
     *
     * ## 为什么按**双条件**分流（跟 notice 那条**同一口径**，
     * 见 `plugins/official-chat/client/client.js:885-909`）
     *
     * **只看码会误挂**：`source_unsupported` 在 host 侧有**七个**产出点
     * （目录里没这个会话／真实源取数源没接上／pick 序号用尽／取数桥没起／
     * 捕获源明说不支持该能力／未知端点与异常兜底…）⇒ ⇒ 只看码 ⇒ 「正文这一格
     * 还没接通」会挂到别的场景 ⇒ ⇒ 那是**假陈述**（说错比说错更坏）
     * ⇒ ⇒ 按 `detail` 的**机器标记**（`body-source-pending:`）再断一道。
     *
     * **失配是安全的**：标记对不上 ⇒ 落**通用失败文案**（只说事实、不给动作）
     * ⇒ ⇒ **不会说错**，也不会落到那句专属文案上。
     *
     * ## 「没有协议码」那一档为什么原样
     *
     * host 造的 `message` **一定带协议码**（码就是 `message` 的前缀）⇒ ⇒ **带码**
     * 的才是 host 的裸码（那一律换成品文案）；**不带码**的是**客户端自己的话**
     * （读取消、测试钩子那些）⇒ ⇒ 原样保留（既有断言
     * `plugins/official-chat/test/client.test.mjs:798` 钉的就是这一档，
     * 按「不许动既有断言」不改它）。
     *
     * @param {any} error - 失败的那个错误（带 `code` / `detail`）。
     * @returns {Error} 抛给内核的错误（`message` 是成品文案，不是裸码）。
     */
    function serializeFailure(error) {
      const code = String(error?.code ?? '');
      const detail = String(error?.detail ?? '');
      // **命中**：正文这一格还没接通 ⇒ 专属文案（与 notice 那条同一句）。
      if (code === 'source_unsupported' && detail.startsWith(BODY_SOURCE_PENDING_MARK)) {
        return new Error(bodySourcePendingText());
      }
      // **没有协议码** ⇒ 客户端自己的话 ⇒ 原样（不是裸码那一档）。
      if (code === '') {
        return error instanceof Error ? error : new Error(String(error?.message ?? error));
      }
      // **失配** ⇒ 通用失败文案（不含错误码、不含内部词、只说事实）。
      return new Error(serializeGenericFailureText());
    }

    /**
     * **serialize 失败的通用文案**（失配那一档）。
     *
     * ## 口径
     *
     * · **不含错误码**：`snapshot_invalid` 这种码用户看不懂、也没法照着做。
     * · **不含内部词**：不写 host／协议节号／前置编号／ref 串。
     * · **不给固定尾巴**：「重新打开 @ 菜单再试」在这一档**不保证有用**
     *   （协议 P1 §3 第 ⑦ 条「假动作禁令」）⇒ ⇒ 判据是「用户做完那个动作，
     *   真能改变这一档的结果吗」——这一档的成因不定 ⇒ ⇒ 答不出「能」
     *   ⇒ ⇒ 只陈述现状。
     * · **不装成成功**：失败就是失败。
     *
     * ## 为什么带着「快照不存在」这半句
     *
     * 既有断言钉住了这个说法（`plugins/official-chat/test/client.test.mjs:881`、
     * `plugins/official-chat/test/client.test.mjs:1042`：快照没了时
     * reject 的文案要认得出「快照不存在」或 `snapshot_invalid`）⇒ ⇒ 通用文案
     * 带着这半句 ⇒ ⇒ 用户也读得出「引用的那份正文已经不在了」。
     *
     * @returns {string} 成品文案。
     */
    function serializeGenericFailureText() {
      return '这条引用没读出正文：快照不存在，或读取过程失败。';
    }

    /**
     * **catalog 失败时的成品文案**（按码给，**不显示错误码**）。
     *
     * ## 为什么要有这张表
     *
     * 原来那一句是 `host 的目录命令失败（${code}）` ⇒ ⇒ **两个问题**：
     * ① **内部词「host」露给用户**（那是插件与 host 半边之间的内部叫法）；
     * ② **错误码露给用户**（`source_unsupported` 这种英文码，用户看不懂、也没法
     *   照着做）⇒ ⇒ 用户读到的是一句**没法行动**的话。
     *
     * ## 口径
     *
     * · **按码给成品文案**（口径对齐协议 §3 的码表与 `userNext`）：每个码一句
     *   **完整、自包含**的话（说清「发生了什么 ＋ 你能做什么」）。
     * · **不显示错误码、不显示内部词** ⇒ 码与内部串只进埋点（`record`）。
     * · **未知码 ⇒ 通用兜底**（不说「未知码」，也不猜它是什么）。
     * · **不装成成功**：失败就是失败。
     *
     * @param {string} code - 协议码（认不出的走兜底）；`host_unreachable` 是
     *   **客户端自己判**的一档（页面里没有 fetch ⇒ 通道没注册上），**不是协议码**。
     * @returns {string} 成品文案。
     */
    function catalogUnavailableText(code) {
      const known = String(code ?? '');
      // **客户端自己判的一档**（不是协议码）：通道没注册上。
      if (known === 'host_unreachable') {
        return '还没连上官网对话，重新打开 @ 菜单再试。';
      }
      // **目录空**：不是错，是这一页没有东西可列。
      if (known === 'catalog_empty') {
        return '这个来源没有会话，换个关键词试试。';
      }
      // **这个来源列不出会话**（如真实取数源没接上）——**不甩内部说法**。
      //
      // ⚠️ **这句覆盖的是 P1 §3 `source_unsupported` 的「用户下一步」**（码表那
      // 格写的是「**换一个来源**」）⇒ ⇒ **不许说成别的方向**（不许说成「重试」
      // 「升级」）。
      //
      // **为什么落成「到官网窗口复制那段对话」**：协议那格是**通用语境**的简写
      // ⇒ ⇒ 具体路径上要给出**用户真能做的动作**；而**当前产品只有一个来源**、
      // 界面上**没有「换来源」的入口** ⇒ ⇒ 「换一个来源」的可执行形态就是**去
      // 官网窗口（真实来源的本体）自己拿那段对话**。
      //
      // **为什么不选「重开再试」**：这一档的成因是「取数源没接上／取数桥没起」
      // ⇒ ⇒ 重开**不保证**有用 ⇒ 那是个**假动作**。
      if (known === 'source_unsupported') {
        return '这个来源现在列不出会话：请到官网窗口复制那段对话。';
      }
      // **没登录**：用户能自己解决 ⇒ 说清去哪儿登录。
      if (known === 'not_logged_in') {
        return '还没在官网窗口登录，登录后才能列出会话。';
      }
      // **账号变了**：要重开官网窗口（会话与账号绑定）。
      if (known === 'account_changed') {
        return '登录的账号变了，重新打开官网窗口再列一次。';
      }
      // **站点改版了**：用户能做的只有升级插件。
      if (known === 'schema_changed') {
        return '官网的页面结构变了，升级插件后才能列出会话。';
      }
      // **超时**：**可重试** ⇒ 明确说「再试」。
      if (known === 'timeout') {
        return '会话列表没在时间内读完，重新打开 @ 菜单再试。';
      }
      // **`catalog_failed` 与一切认不出的码**：通用兜底（不猜、不显示码）。
      return '会话列表这次没取到，重新打开 @ 菜单再试。';
    }

    /**
     * 调 host 的 `duplicates` 命令，产出 P2 提示（协议 §2.2 / §3.1 / §4）。
     *
     * **触发时机**（协议 §3.1）：
     * · §3.1-1「重复发生时」：**每选中一条引用就核对一次**——选到第二个及
     *   以后的同组引用时，同组成员数 ≥ 2 ⇒ 立即出现提示（host 侧只返回成员数
     *   ≥ 2 的组，所以选第一条时不会误报）。
     * · §3.1-2 / §4「发送前的预算预检」：**同一趟**顺带算出（协议 §2.2 的
     *   `budget` 与 `groups` 同一次往返出）⇒ 每次新增引用都会重算一次预算。
     *   **口径说明**：内核**没有**「提交前草稿投影」的入口 ⇒ 预算按 client
     *   自己掌握的引用 id 列表算，**原生 `@session` 的占用不可观测**（按 0
     *   计）⇒ 口径**偏保守**（可能少报一点），且删掉 chip 后不会自动回收。
     *
     * **失败不阻塞发送**（协议 §2.3）：失败 ⇒ 提示「无法核对是否重复」＋ 可见
     * 提示，**不额外拦截发送**（本函数不抛错、也不挂到发送路径上）。
     *
     * **提示档位一律 `info`**（协议 v0.2 §3.2）：内核只渲染 `info` 档的**输入区
     * 常驻条**（`dsh-client-ui-conversation/lib/client.js@0.2.1-rc.2:18982-18986`）；
     * `error` 档**只弹瞬态 toast**、没有常驻条
     * （`dsh-client-ui-conversation/lib/client.js@0.2.1-rc.2:18808`）
     * ⇒ 本批**任何**提示（含失败与超预算两条）都必须用 `info` 档。
     *
     * @param {object} ctx - 客户端根上下文。
     * @param {object} sessions - sessions 服务。
     * @param {string} sourceSessionId - 官网会话 id（协议 §2.2 的入参）。
     * @param {string} [triggerRef] - 触发这次核对的引用 id（新选中的那条）。
     * @returns {Promise<object|undefined>} 核对结果（探针与测试用）。
     */
    function checkDuplicates(ctx, sessions, sourceSessionId, triggerRef) {
      const run = () => runDuplicatesCheck(ctx, sessions, sourceSessionId, triggerRef);
      // **无论上一轮成功还是失败，都继续跑下一轮**（一轮失败不该卡住队列）。
      const chained = duplicatesChain.then(run, run);
      duplicatesChain = chained.then(
        () => undefined,
        () => undefined,
      );
      return chained;
    }

    /**
     * 真正跑**一轮**核对（由 `checkDuplicates` 串行调用，见上面那条串行链）。
     *
     * @param {object} ctx - 客户端根上下文。
     * @param {object} sessions - sessions 服务。
     * @param {string} sourceSessionId - 官网会话 id（协议 §2.2 的入参）。
     * @param {string} [triggerRef] - 触发这轮核对的引用 id。
     * @returns {Promise<object|undefined>} 本轮核对结果。
     */
    async function runDuplicatesCheck(ctx, sessions, sourceSessionId, triggerRef) {
      const host = hostClient(ctx);
      if (host === undefined) {
        // **通道不可达也是一种「失败」**（协议 §2.3：失败 ⇒ 提示「无法核对是否
        // 重复」）⇒ 同样给一条 **info 档**可见提示（§3.2：`error` 档只弹瞬态
        // toast，没有常驻条）。**不额外拦截发送**（本函数不抛错）。
        record('duplicates-unavailable', { why: 'host 不可达' });
        presentNotice(ctx, sessions, sourceSessionId, {
          kind: 'duplicates-failed',
          level: 'info',
          trigger: triggerRef ?? null,
          text: '无法核对是否重复。',
        });
        return undefined;
      }
      const snapshotIds = [...selectedRefs];
      if (snapshotIds.length === 0) return undefined;
      let result;
      try {
        result = await host.call('duplicates', {
          sessionId: sourceSessionId,
          snapshotIds,
          requestId: triggerRef ?? snapshotIds[snapshotIds.length - 1],
        });
      } catch (error) {
        record('duplicates-failed', { message: String(error?.message ?? error) });
        presentNotice(ctx, sessions, sourceSessionId, {
          kind: 'duplicates-failed',
          level: 'info',
          trigger: triggerRef ?? null,
          text: '无法核对是否重复。',
        });
        return undefined;
      }
      if (result?.ok !== true) {
        record('duplicates-rejected', { code: result?.code ?? null });
        presentNotice(ctx, sessions, sourceSessionId, {
          kind: 'duplicates-failed',
          level: 'info',
          trigger: triggerRef ?? null,
          code: result?.code ?? null,
          text: '无法核对是否重复。',
        });
        return undefined;
      }
      // **组的顺序由 host 定**（协议 §3.2「多组形态」· PM 01:3x：host 是产出处 ⇒
      // 排序在产出处做 ⇒ 任何消费方都拿到稳定顺序；client **照单渲染**）。
      const groups = Array.isArray(result.groups) ? result.groups : [];
      const budget = result.budget ?? {};
      // **成品文案只有一份**（UX 定稿 2026-10-21 · 协议 v0.6 §3.2「两条逐字相同」）：
      // §3.1 要求「后发那条自包含全部事实」⇒ 两条说的是同一件事 ⇒ 同一句文案
      // （内核 notice 是**单槽**，用户最终看到的就是后发那条 ⇒ 它必须全）。
      const text = duplicatesNoticeText(groups, budget);
      if (groups.length > 0) {
        presentNotice(ctx, sessions, sourceSessionId, {
          kind: 'duplicate-groups',
          level: 'info',
          trigger: triggerRef ?? null,
          groups,
          budget,
          text,
        });
        // **不 return**：协议 §3.1 的**两个触发点各自独立**——「重复发生时」与
        // 「超预算时」可以同时成立 ⇒ **两条提示都要出**（早退会把预算提示吃掉）。
      }
      if (budget.wouldExceed === true) {
        presentNotice(ctx, sessions, sourceSessionId, {
          kind: 'budget-exceeded',
          level: 'info',
          trigger: triggerRef ?? null,
          groups,
          budget,
          text,
        });
      }
      if (groups.length === 0 && budget.wouldExceed !== true) {
        record('duplicates-checked', {
          groups: 0,
          projectedBytes: budget.projectedBytes ?? null,
        });
      }
      return result;
    }

    /**
     * 引用句柄的**状态提示**（用户可见）。
     *
     * **为什么去掉了「G1：」前缀**：`G1` 是**项目内的里程碑代号**（「G1 可行性
     * 验证」）⇒ ⇒ 用户看到的是内部代号 ⇒ ⇒ 去掉（izzy 2026-10-21 当面点名）。
     * ⇒ 去掉之后句子本身**自包含**（主语是「引用「…」」）⇒ 读着仍是人话。
     *
     * **为什么留着 `ref`**：这串 id **用户在编辑器里看得见**（chip 的可见文本
     * 就是它——内核 `dsh-client-ui-conversation/lib/client.js@0.2.1-rc.2:13752-13753`
     * 那个 `getTextContent()`（**方法头** + `return this.__clipboardText;` 两行）
     * 返回 `clipboardText`，而 `clipboardText` 就是 `ref`）
     * ⇒ ⇒ 带上它，用户能对上是**哪一条**（多条同名引用时尤其需要）⇒ ⇒ **它不
     * 是只有开发看得见的内部串**，去掉反而说不清是哪一条。
     *
     * ## 成功档（`ready`）的措辞纪律（PM 2026-10-22 08:3x／08:4x 裁定）
     *
     * · **只承诺「已取到」**：**不许出现「全部」「都取到」**这类完整性说法 ⇒ ⇒
     *   现实现只按 query 里的 `chat_session_id` 匹配**等那一条**响应 ⇒ ⇒ 「取
     *   到的是不是该会话的全部」**目前没有依据**（协议 §9 **G4-23** 登记为缺
     *   口）⇒ ⇒ 说「全部」是**没依据的承诺**。
     * · **不许说「已就绪」**（文件头那条纪律：pending ≠ ready）⇒ ⇒ 这一档说的
     *   是「正文已取到」，不是「这条引用就绪了」。
     * · **落点**：这句**只在用户点开引用详情时**报（`openReference` 那条路）⇒ ⇒
     *   **pick 成功时不主动弹**（chip 出现本身就是信号；PM 08:4x 提醒：别挂在
     *   pick 的回调上）。
     */
    function showState(ctx, sessions, sessionId, ref, state) {
      const text =
        state === 'ready'
          ? `引用「${ref}」的正文已取到，发送时会一起带上。`
          : state === 'failed'
            ? `引用「${ref}」的正文读取失败，发送时会拒绝这次提交`
            : state === 'pending'
              ? `引用「${ref}」的正文还在读取中，尚未就绪`
              : `引用「${ref}」没有对应的读取记录`;
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
     * ## 两层提示各管一件事（PM 2026-10-21 23:5x 裁定 · 这道锁别省）
     *
     * · **菜单那行**（`unavailableRow` 的 `description`）＝ **为什么列不出**
     *   （catalog 失败的成品文案，如「还没在官网窗口登录，登录后才能列出会
     *   话。」）⇒ 用户**点之前**就看得见。
     * · **本条**（＝ 外层提示）＝ **这次没形成引用句柄 ＋ 下一步**。
     *
     * ⇒ ⇒ **两层不许互相套整句**（PM ①）：成品文案**不**塞进这里的括号 ⇒ ⇒
     * 括号里只放 **≤ 6–8 字的类别词**（如「没取到引用编号」「引用编号超
     * 长」），**不套整句**（PM ③）。
     * ⇒ ⇒ **菜单那行说过的原因，这里不复读**（PM ②：「没登录」这类原因既不
     * 能被盖掉、也不能被说两遍）⇒ ⇒ 那时**整句不带括号**。
     *
     * **为什么这不是「两套模板混着来」**：规则只有**一条** —— 括号里放的是
     * **这一档独有的类别词**；**这一档没有独有的原因**（菜单那行已经说过）⇒
     * ⇒ **整句不带括号** ⇒ ⇒ **绝不会渲染出空括号**（`（）` 那种空位）⇒ ⇒
     * 也绝不会把整句成品文案塞进来。
     * ⇒ ⇒ 反过来：**这一档独有的原因不能丢**（如「引用编号超长」——菜单那行
     *   从来没说过它）⇒ ⇒ 它就靠这个类别词进界面。
     *
     * ⚠️ **为什么这道锁要写在注释里**：上一版把 catalog 的**成品文案整句**
     * 塞进了括号 ⇒ 拼出「官网引用这次没有生成（会话列表这次没取到，重新打开
     * @ 菜单再试。），重新打开 @ 菜单再试」⇒ **同一句说两遍**、括号里套整句
     * 也不像话 ⇒ ⇒ 口径是「**一层说为什么、一层说接下来怎么办**」。
     *
     * ## 下一步按档给（PM 2026-10-21 23:5x 裁定）
     *
     * **判据（一句话）**：给一个动作之前先过这一道 —— **用户做完那个动作，真
     * 能改变这一档的结果吗？** ⇒ 答不出「能」，**就不许给那个动作**，宁可
     * **只说事实、不给动作**。
     *
     * **为什么不能一刀切**：原来那条固定尾巴「重新打开 @ 菜单再试」**对每一
     * 档都套** ⇒ ⇒ ① **两档配上它都站不住**（见下面三档的事实）⇒ ⇒ ② 而且
     * **菜单那行每档都自带下一步**（重开菜单／换关键词／到官网窗口复制／登
     * 录／重开官网窗口／升级插件…）⇒ ⇒ 外层再补一条固定尾巴＝**复读**，还
     * 可能跟菜单那行的下一步打架。
     *
     * **三档**（括号有无、类别词、动作各是什么）：
     *
     * · **菜单那行已说原因**（它也说了下一步）⇒ 外层**只说事实、不给动作**、
     *   **不复读** ⇒ 渲染成「官网引用这次没有生成」⇒ 动作**不适用**（不给）。
     * · **没取到引用编号** ⇒ 成因是 host 的目录项没给出可用的 id ⇒ 用户侧
     *   唯一能改变结果的动作是**换一条会话**（另一条目录项可能分配到了 id）
     *   ⇒ ⇒ 判据答「能」⇒ ⇒ 给它 ⇒ 渲染成「官网引用这次没有生成（没取到引用
     *   编号），换一条会话再试」。
     * · **引用编号超长** ⇒ id 由 host 分配（形态上界 22 字符，小于客户端判的
     *   24；见 `plugins/official-chat/host/protocol.js:377-383`）⇒ 真到这档说
     *   明 host 侧的 id 形态变了 ⇒ **用户侧没有可执行动作**⇒ ⇒ 判据答不出
     *   「能」⇒ ⇒ **只说事实、不给动作** ⇒ 渲染成「官网引用这次没有生成（引用
     *   编号超长）」。
     *
     * **本函数只负责拼**：事实 ＋ 类别词 ＋ 动作三件套 ⇒ 动作那一件**必须由
     * 调用点按档给**，且**每个非空动作都得过上面那道判据**。
     *
     * @param ctx - 客户端根上下文。
     * @param sessions - sessions 服务。
     * @param sessionId - 会话 id。
     * @param fact - **短类别词**（≤ 6–8 字）或**空串**（＝ 菜单那行已经说过
     *   原因，这里不重复）。⚠️ **不是一句成品文案** ⇒ 内部串（host 的原始
     *   `detail`／异常 message）更不许进来 ⇒ ⇒ 那些只进埋点。
     * @param action - **用户做完真能改变这一档结果的动作**（≤ 8 字）或**空串**
     *   （＝ 这一档没有可执行动作 ⇒ 只说事实）。⚠️ **不许一刀切** ⇒ 每个非空
     *   动作都得过上面那道判据。
     */
    function showUnavailable(ctx, sessions, sessionId, fact, action) {
      // **同样去掉「G1：」前缀**（项目内里程碑代号，用户不认——见上面
      // `showState` 的注释）⇒ 去掉后句子自包含、读着像人话。
      const brief = String(fact ?? '').trim();
      const step = String(action ?? '').trim();
      const text =
        `官网引用这次没有生成${brief === '' ? '' : `（${brief}）`}${step === '' ? '' : `，${step}`}`;
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
