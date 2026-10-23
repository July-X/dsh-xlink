/**
 * dsh-xlink 内置本地记忆插件（Host 侧）。
 *
 * ## 它做什么
 *
 * 1. 每个 agent 创建时，在该 agent 的作用域里注册一条**运行上下文贡献**
 *    （`xlink.memory`）：文本是本会话的检索缓存，`interpolate: false`——记忆正文
 *    里的 `{{…}}` 不该被当成提示变量。
 * 2. 每个用户回合的第一步，向壳的回环桥要一次**有界检索**，更新缓存。
 * 3. 注册三个工具：`memory_search`（按查询看有哪些记忆）、`memory_read`（按 id 读
 *    完整正文）与 `memory_propose`（提交候选，**只能进待确认**，批准只存在于面板里）。
 *
 * ## 三条不许破的纪律
 *
 * · **`agent/created` 里不许抛**：内核的那个事件是串行的，监听器抛出去会让 agent
 *   创建失败——记忆坏了不该让普通对话起不来。这里每一处都自己吞掉并留一行日志。
 * · **失败用空缓存**：桥不可用、超时、越权都退回「本回合没有记忆」，绝不沿用上一
 *   回合的正文（用户停用/删除后仍看到旧内容，是最伤信任的一种错）。
 * · **检索不等于放行**：`pre-step` 里先 `await next()` 拿到决定，再做检索，最后
 *   原样返回那个决定——排序、拒绝都归别的插件，这里只更新缓存。
 *
 * ## 上下文为什么走 `agent.ctx`
 *
 * `refreshContext()` 只重解析**装配时已注册**的贡献，装配之后才注册的名字不会被
 * 补上（除非声明 `required`）。所以在 `agent/created` 里用 `agent.ctx.systemPrompt.context`
 * 注册，而不是在 `pre-step` 里懒注册——后者会静默不注入，而每一处都不报错。
 *
 * @module xlink-memory
 */
import { defineTool } from '@deepseek-ai/dsh-tools';

import { BlockAssembler, createUserMessage } from '@deepseek-ai/dsh-llm';
import { MAX_PATH_BYTES, callMemory, claimKeyOf, encodedPathBytes, readCredentials } from './bridge.js';
import {
  EXTRACTION_SYSTEM_PROMPT,
  EXTRACTOR_VERSION as EXTRACT_VERSION,
  buildSlice,
  contentFingerprint,
  extractionUserText,
  parseExtractionOutput,
} from './extract.js';
import { readSessionRange } from './sessionlog.js';
import { runExtraction } from './run.js';
import {
  afterCompaction,
  applyStep,
  contextDelta,
  createWakeScheduler,
  isCompactionDone,
  scopeForWorkspace,
  memoryScope,
  beginStep,
  emptyExtractState,
  EXTRACT_DEBOUNCE_WAKE_MS,
  EXTRACTOR_VERSION,
  noteSessionEvent,
  shouldScheduleExtraction,
  conflictQuery,
  resolveWorkspaceId,
  shouldProbeLibrary,
  userText,
} from './policy.js';

/**
 * 注入块 / 工具输出 / 面板**共用同一套中文标签**。
 *
 * 三处各写一份是不得已：面板在 `ui/src/memory/memory.js`（Vue 侧）、注入块在壳的
 * `src-tauri/src/memory/query.rs`（Rust 侧）、工具输出在这里（插件侧）——跨语言跨进程，
 * 没有共享模块可放。所以**词表钉在判据里**（`test/assembly.test.mjs` 读三处源码比对），
 * 不靠自觉：三处说三个词时，用户拿面板上的话解释不了模型看到的东西。
 *
 * 键是壳下发的**原始枚举**（`scope` / `verification` 字段），认不出的原样显示——
 * 后端将来加一档时至少看得见，不会被静默说成「未知」。
 */
export const SCOPE_LABELS = { general: '通用偏好', project: '项目记忆' };
export const VERIFICATION_LABELS = { verified: '已验证', unverified: '未验证', stale: '已过期' };
const scopeLabel = (scope) => SCOPE_LABELS[scope] ?? String(scope ?? '');
const verificationLabel = (value) => VERIFICATION_LABELS[value] ?? String(value ?? '');

/** 与 `plugins/memory/package.json` 的 `name`、接线行 id 三者同值。 */
const PACKAGE_ID = 'xlink-memory';
const name = PACKAGE_ID;
/** 需要的服务：上下文注册面、工具注册面、有效工作目录。 */
const inject = ['systemPrompt', 'tools', 'workingDirectory'];
/** 运行上下文贡献名。 */
const CONTEXT_NAME = 'xlink.memory';
/**
 * 排序值 300：内核 `CONTEXT_ORDERS` 目前只有四个槽位
 * （WORKING_DIRECTORY 100 / SANDBOX_POLICY 110 / APPROVAL_POLICY 115 /
 * SUBAGENT_DELEGATION 120），没有给记忆留位置。取 300 排在它们之后——记忆是
 * 背景材料，放在操作事实后面。**不是调 `getContextOrder("MEMORY")`**：那个名字
 * 不存在，传进去只会拿到 `undefined` 并在注册时抛错。
 */
const CONTEXT_ORDER = 300;

/**
 * 装配。
 *
 * @param {object} ctx - 插件上下文。
 */
function apply(ctx) {
  const credentials = readCredentials(process.env);
  if (!credentials) {
    ctx.logger?.warn?.(`${name}: 壳没有为这个内核起回环桥（缺 ${'DSH_XLINK_BUILTIN_BRIDGE_URL'}），插件加载但不会注入任何记忆`);
  }

  // 工作区注册表**按需注入**（`ctx.inject` 的回调只在服务就绪时跑），不进
  // `inject` 数组：它是「顺带把路径换成内核认的 id」这一项增强，而不是本插件的
  // 生存条件。写进 `inject` 的话，任何不带 workspace 服务的 profile 都会让整个
  // 插件停在「等待服务」上——**记忆一条都注入不了，而日志里什么也没有**。
  let workspaceRegistry = null;
  // 每个会话一份注入状态。**为什么按会话存而不是留在 pre-step 的闭包里**：压缩事件是
  // 插件级收到的，要按会话路由回对应那份状态（见下面的订阅）。
  const agentStates = new Map();
  const MAX_TRACKED_SESSIONS = 64;
  /**
   * 「静默期到点再叫醒一次提取」的定时器账本（按会话一份）。
   *
   * **为什么不能只在回合边界上领**：壳那边任务进表后要过 30 秒静默期才算到期
   * （`memory/jobs.rs` 的 `DEBOUNCE_MS`），而排任务那一次紧跟着就去领必然领不到——
   * 于是每个会话最后留下的那条任务要等用户**下一次说话**才被顺带领走。2026-10-16 的
   * 真模型探针实测：聊完 5 回合再等 150 秒，任务表里那条 `pending: 1` 一动没动。
   * 理由与三条纪律见 `policy.js::createWakeScheduler`。
   */
  const wakeups = createWakeScheduler();

  // 压缩之后预算清零（见 `afterCompaction`）。
  //
  // **这条订阅必须挂在插件自己的 `ctx` 上，不能挂 `agent.ctx`**：`session/event` 是在**会话
  // scope** 上派发的（`dsh-session` 的 `commit()` 用会话自己的 `emitCtx`），而 `dsh-scope` 的
  // 约定是「事件沿 scope 向上流、绝不向下」——`scopeTarget` 的注释原话是 `a listener owned by
  // an enclosing scope receives every descendant scope's events`，反过来（挂在更靠里的 agent
  // scope 上）**一条都收不到**，而且没有任何症状：压缩之后只是再也不注入记忆。
  ctx.on('session/event', (session, event) => {
    const box = agentStates.get(String(session?.id ?? ''));
    if (!box) return;
    // **只有成功的压缩才重置**：失败的那次（`end` 带 `error`）没有丢掉任何快照，
    // 重置只会白取一次检索，甚至把同一段内容再塞一遍（见 `isCompactionDone`）。
    if (isCompactionDone(event)) {
      box.state = afterCompaction(box.state);
      box.handoff = afterCompaction(box.handoff);
      box.contextText = '';
      return;
    }
    // P2：这一段里有真的人类消息就记下它的序号（纯注入的回合不算，见 `HUMAN_SOURCE`），
    // 回合正常结束时排一个提取任务。**排任务失败不抛也不阻断**——会话照常走，下一次
    // 正常结束的回合游标更靠后，范围里还包含这些内容。只有成功才推进游标：
    // 否则这一次的内容就再也没人看它了。
    box.extract = noteSessionEvent(box.extract, event);
    const decision = shouldScheduleExtraction(box.extract, event);
    if (!credentials) return;
    // **先排、再领**：排任务只往表里放一行（壳那边有静默期，刚放进去的不会立刻到期），
    // 而这次真正要跑的往往是**前面某个回合**留下的那条——相邻回合因此合并成一次提取。
    // 领不到就什么也不做：没有到期任务是常态，不是失败。
    const runExtractionNow = () => {
      // **整段不抛**：提取跑在用户的会话里，它坏了不该影响对话，**更不该把插件本身弄崩**
      // ——2026-10-15 实测过一次：`runDueExtraction` 里一个 TDZ 异常让内核报
      // `fatal load failure`，整个插件当场失效，而 55 条插件测试与 993 条 Rust 测试全绿
      // （装配层没有内核，跑不到这条线）。所以这里**必须**兜住，不能只靠「函数内部不抛」的自觉。
      return runDueExtraction(
        credentials,
        liveExtractionServices(),
        box.state.workspace,
        session?.id
      ).catch((error) => {
        ctx.logger?.warn?.(
          `xlink-memory: 领取/提取这条线抛了异常（不影响对话）：${String(error?.message ?? error)}`
        );
      });
    };
    const attempt = () => {
      // **只在回合边界上领**：`session/event` 每个事件都会到这里（一个回合几十条），
      // 每来一条就去问一次「有到期任务吗」是白打几十个请求（第一版就是这样，13 次 claim）。
      // 记人类消息那条路仍然看**每个**事件——它要的正是事件流本身。
      if (event?.type !== 'turn/end') return;
      void runExtractionNow();
    };
    if (!decision.schedule) {
      attempt();
      return;
    }
    box.extract = { ...box.extract, lastCursor: decision.cursor };
    void enqueueExtraction(credentials, session, decision.cursor).then(
      () => {
        attempt();
        // **排完还要在静默期之后再叫醒一次**：这一次的 `attempt()` 必然领不到（任务刚进表，
        // 壳那边要再过 30 秒），而下一次 `turn/end` 可能永远不来——用户说完最后一句就
        // 去干别的、或者关掉工作台，那条任务就一直搁着（2026-10-16 真模型探针实测）。
        // 排成功了才排这一次：排失败时下一次正常结束的回合会带着更靠后的游标重来。
        wakeups.schedule(
          String(session?.id ?? ''),
          EXTRACT_DEBOUNCE_WAKE_MS,
          runExtractionNow,
          (error) => {
            ctx.logger?.warn?.(
              `xlink-memory: 静默期唤醒抛了异常（不影响对话）：${String(error?.message ?? error)}`
            );
          }
        );
      },
      attempt
    );
  });

  /**
   * 排一个自动提取任务（P2）。**只有壳点了头才算数**：开关关着时桥那边答 `disabled`，
   * 这里只记一行 warn——「获许可的会话」是设计 §8-1 的第三条判据，不放在插件里判，
   * 免得两个地方各有一份「开关是什么状态」。
   */
  /** 提取调用的输出上限：候选是小东西，给太多只会让它啰嗦。 */
  const EXTRACT_MAX_TOKENS = 2048;
  /** 一次提取的总时限（H8 的最小一半）：超时就取消，任务按失败退回去重试。 */
  const EXTRACT_TIMEOUT_MS = 120_000;
  /**
   * 续租间隔。**壳那边的租约是 120 秒**（`jobs::LEASE_MS`），而这里按 45 秒续一次：
   * 独立评审第 9 条指出，原先两边都是 120 秒——而一个从「收到回执」起算、一个从「壳发令牌」起算，
   * 于是慢模型跑到 119 秒再提交时租约已经过期：另一个会话一 claim 就把它接管，
   * 旧令牌的 `/jobs/submit` 与 `/jobs/fail` **全部无效**，结果与失败原因一起消失。
   * 45 秒 = 两次续租都还有余量（网络抖一次也来得及）。
   */
  const RENEW_EVERY_MS = 45_000;

  /**
   * 调一次提取模型——**一次性调用，不进用户会话**。
   *
   * 模板抄内核自己的 `session-title-llm`（它正是「不产生用户可见对话的一次性调用」）：
   * `prepareCall(route, signal, configure)` → `call.stream({...call.config, messages, system, signal})`。
   * 两处刻意的取舍：
   * · **`purpose` 不填**：设计不许冒充 `session-title`，也不许自造一个内核没登记过的用途。
   * · **`sessionId` 不传**：传了这次请求会被记进那个会话的日志；提取是插件自己的事，
   *   不该在用户的会话里留下一条谁也看不懂的记录。
   */
  async function callExtractionModel({ llm, route, text, existing, pending, signal }) {
    try {
      const call = await llm.prepareCall(
        { provider: route.provider, model: route.model, maxTokens: EXTRACT_MAX_TOKENS },
        signal
      );
      const messages = [
        createUserMessage({
          // **切片、既有记忆与待确认候选在同一条消息里**（C5/C6）：模型要判断的是「新提的
          // 这条与库里那条是不是同一件事」「用户是不是在改先前那条」，三者分开就变成三次独立
          // 输入，而 `conflicts_with` / `supersedes` 会一直为空。
          content: [{ type: 'text', text: extractionUserText(text, existing, pending) }],
          source: { kind: 'xlink-memory-extract' },
        }),
      ];
      const assembler = new BlockAssembler();
      for await (const chunk of call.stream({ ...call.config, messages, system: EXTRACTION_SYSTEM_PROMPT, signal })) {
        assembler.push(chunk);
      }
      const answer = assembler
        .blocks()
        .filter((block) => block?.type === 'text' && typeof block.text === 'string')
        .map((block) => block.text)
        .join('\n');
      return { ok: true, text: answer };
    } catch (error) {
      return { ok: false, reason: String(error?.message ?? error) };
    }
  }

  /**
   * 领一个到期的任务并跑完它。
   *
   * 顺序是**先领旧的、再排新的**：任务进表之后要过一段静默期（壳那边的 `DEBOUNCE_MS`）才算到期，
   * 所以这里领到的通常是**前面某个回合**留下的那条——把相邻的几个回合合并成一次提取。
   * 领不到就什么也不做，这不算失败。整段不抛：提取跑在用户的会话里，它坏了不该影响对话。
   */
  async function runDueExtraction(creds, services, workspace, currentSessionId) {
    // 桥的返回是 `{ok, value}`（见 bridge.js），而 `/claim` 的形状是
    // `{token, key:{session,cursor,extractor,from_cursor}, attempts, expires_at_ms}`
    // （见 jobs.rs 的 `Claimed`）。**`from_cursor` 也在里面**：它是「这段会话上次提取到哪儿」，
    // 而读取那一段的起点就是它——漏掉它症状是静默的（每次从 0 重读整份），理由见 `claimKeyOf`。
    // **带上会话**：不带的话壳会从全表挑最早那条，而提交用的坐标取自**发起领取的会话**——
    // 两个会话同跑时，A 的对话内容会带 B 的项目坐标落库（评审第 3 条）。领自己那条才对得上。
    const claimed = await callMemory({
      credentials: creds,
      route: 'jobs/claim',
      // **这是「当前会话」的 id**（用来只领自己那条），与下面从任务键里读出来的 `sessionId`
      // 不是同一个东西——第一版把两者写成一个名字，于是这里用在了 `const sessionId` 之前，
      // 真内核里抛 TDZ 异常、整个插件 `fatal load failure`（2026-10-15 探针逮到）。
      params: { session: String(currentSessionId ?? '') },
    });
    const token = claimed.ok ? claimed.value?.token : null;
    if (typeof token !== 'string' || token.length === 0) return;

    const claimedKey = claimed.value?.key ?? {};
    const sessionId = String(claimedKey.session ?? '');
    const endSeq = Number(claimedKey.cursor ?? 0);
    // **要拿到真的会话对象，不能自己拼一个 `{id}`**。内核的 `SessionStore.flush` 用
    // `WeakMap` 按**对象身份**取 live entry（`dsh-session`），一个自拼的字面量永远不是它认识的那个
    // ——主路（落盘屏障 → 读句柄）因此每次都抛 `session "…" is not live in this store`，
    // 静默退到 live 兜底（独立评审第 2 条，Lead 复核过内核源码）。
    // 按**任务键里的会话 id** 去 store 里取：领到的任务未必属于当前这个会话（见第 3 条），
    // 所以不能拿手上这个 `session` 对象顶替。取不到就让它空着，读的那一步会如实说「没有会话对象」。
    const liveSession = services.sessions?.get?.(sessionId) ?? null;
    // **任务键的重建交给 `claimKeyOf`**（它在 `bridge.js`，纯函数、可直接测）：这一处原先
    // 是手挑 `session / cursor / extractor` 三个字段，`from_cursor` 落在外面，于是读取恒从 0
    // 开始——那条注释声称要避开的「每次重读整份日志」正是这么来的，而更糟的是 `content_hash`
    // 因此覆盖整段会话，H7 的指纹判据每次都成立。原因写在 `claimKeyOf` 的注释里。
    // 第三个参数是 `extractor` 的兜底：那个常量只有本文件有（`extract.js` 的 `EXTRACTOR_VERSION`，
    // 与下面 `jobs/submit` 用的是同一个），`bridge.js` 是纯函数、不该知道它——早先搬过去时被写成空串。
    const key = claimKeyOf(claimedKey, liveSession, EXTRACT_VERSION);
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), EXTRACT_TIMEOUT_MS);
    // **续租**：不续的话，跑得久一点就会被别的会话接管，而这一轮的结果与失败原因都会作废
    // （评审第 9 条）。续不到就**提前收工**——继续跑只是白烧一次模型调用。
    const renewTimer = setInterval(() => {
      void callMemory({ credentials: creds, route: 'jobs/renew', params: { token } }).then(
        (answer) => {
          if (answer.ok && answer.value?.renewed === false) {
            ctx.logger?.warn?.('xlink-memory: 租约已被接管，这次提取提前收工');
            controller.abort();
          }
        }
      );
    }, RENEW_EVERY_MS);
    let result;
    try {
      const selection = services.agentDefaultModel?.currentSelection?.();
      // **工作区坐标这一次解析一次，检索与提交共用**：提交那份坐标若与检索那份不是同一个
      // 工作区，模型标出的冲突就指向「别的项目的记忆」，而面板并排摆出来时用户看不出来。
      const workspaceId = await resolveWorkspaceId(services.workspaceRegistry, workspace);
      result = await runExtraction({
        // 键里两处要分清：读日志那条路要**会话对象**（用 `session.id`），给桥那条要**字符串 id**。
        // 第一版这里写的是 `{ session: { id: key.session }, ...key }`——展开顺序让内层立即被覆盖，
        // 是个死表达式（评审第 2 条的附带项）。
        key,
        // **从上一游标开始读**（独立评审第 7 条）：壳在排任务时把「这段会话上次提取到哪儿」
        // 冻进了任务键（`from_cursor`），这里照着读——原先每次都从 0 读整份日志，
        // 长会话下是白白重读一遍。旧壳没有这个字段时它是 0，即旧行为。
        from: Number(key.from_cursor ?? 0),
        to: endSeq,
        signal: controller.signal,
        deps: {
          system: EXTRACTION_SYSTEM_PROMPT,
          readRange: ({ session, from, to }) =>
            readSessionRange({
              sessions: services.sessions,
              sessionPersistence: services.sessionPersistence,
              sessionQuery: services.sessionQuery,
              session,
              from,
              to,
            }),
          buildSlice,
          retrieveForExtract: ({ text }) =>
            retrieveForExtract({ creds, workspaceId, workspacePath: String(workspace ?? ''), text }),
          callModel: ({ text, existing, pending, signal }) =>
            selection
              ? callExtractionModel({ llm: services.llm, route: selection, text, existing, pending, signal })
              : { ok: false, reason: '拿不到当前模型路由（agentDefaultModel）' },
          fingerprint: contentFingerprint,
          parse: parseExtractionOutput,
          submit: async ({ candidates, contentHash }) => {
            let submitted = 0;
            const refusals = [];
            // 工作区 id 在**这一次提取开始时**现解析一次，给检索与提交共用（G3）：预算用满的
            // 回合会在解析之前就早退（`shouldProbeLibrary` 那条），缓存下来的可能是**上一个**
            // 工作区——那正是「候选记到旧项目」。工作目录本身在 `beginStep` 里已刷新。
            for (const candidate of candidates) {
              // **一条候选一次调用**：壳那边的 `/jobs/submit` 就是这么收的（正文经查询串过去）。
              const params = {
                token,
                session: sessionId,
                executed_extractor: EXTRACT_VERSION,
                // **来源引用的那一条**（独立评审第 17 条）：原先写死 0，于是库里每条候选的
                // `source` 范围都长成 `0..cursor`，精确到句的引用全丢了。这里给的是解析阶段
                // 校验过的 `evidence_seq`——它必须指向**切片里**一条真的人类消息（见 `extract.js`）。
                start_seq: candidate.evidence_seq,
                end_seq: endSeq,
                workspace_id: workspaceId,
                // 依据是壳的规则：没登记工作区时 `project` 会被拒（见 `scopeForWorkspace`）。
                scope: scopeForWorkspace(workspaceId),
                workspace: String(workspace ?? ''),
                title: candidate.title,
                body: candidate.body,
                keywords: candidate.keywords.join(','),
                // **冲突标记（C5）**：解析阶段已逐项校验过 id 确实来自这次给模型看的既有记忆，
                // 壳这一侧再收一次形状与限额（`memory/marks.rs`）。走查询串是既有形状
                // （正文那条 8 KiB 通道上限摆在那儿），而 id 是短串，几条也占不了多少。
                conflicts: (candidate.conflicts_with ?? []).join(','),
                // **取代标记（C6）**：同一种形状。壳**不会**据此拒收任何候选——它只存下这个
                // 标记，让面板把两条摆给用户按一次（纪律见 `extract.js` 的规则 9）。
                supersedes: (candidate.supersedes ?? []).join(','),
                // **来源内容指纹（H7）**：不是每个候选带一份，而是**整批一份**——它们全都来自
                // 同一段被冻结的切片。壳拿它判「先前那条待确认候选是不是同一段的重提」。
                content_hash: contentHash ?? '',
              };
              // **提交前按整条路径真算一次**（评审第 6 条）：正文的上限按原文定、通道的上限按编码后
              // 定，中文一乘就是 9 倍——逐项都合法的候选合起来能远超通道，而那种失败是**整批**
              // 被打回（`callMemory` 的预检在发出去之前就拦了，但那时已经来不及只丢这一条）。
              // 这里按条判、按条丢，理由里带上真实数字，其余候选照常提交。
              const pathBytes = encodedPathBytes('jobs/submit', params);
              if (pathBytes > MAX_PATH_BYTES) {
                refusals.push(
                  `「${candidate.title}」编码后 ${pathBytes} 字节，超过通道上限 ${MAX_PATH_BYTES} 字节（中文经 URL 编码约 9 倍）`
                );
                continue;
              }
              const answer = await callMemory({
                credentials: creds,
                route: 'jobs/submit',
                params,
              });
              if (answer.ok && answer.value?.submitted === true) {
                submitted += 1;
              } else if (answer.ok && answer.value?.submitted === false) {
                // **壳的业务闸拒的**（去重、开关关了、来源对不上、序号超范围）：这是**结果**，
                // 不是故障（独立评审第 16 条）。原先一律按 ok:false 处理 → 任务失败 → 重试三次，
                // 白烧两次模型调用，而用户看到的是「这条内容已经在记忆里了」这种不像故障的话。
                refusals.push(answer.value.reason ?? '提交被拒');
              } else {
                // **桥这一层不通**（连不上、非 2xx、超时）：这才是故障，交给任务的重试。
                return { ok: false, reason: answer.reason ?? '提交时桥不可用' };
              }
            }
            // 全被业务闸拒也算**处理完了**：判据是确定性的，重试一次还会得到同样的答案。
            // 理由经 `refused` / `refusedReason` 回到日志与结果里（第 5 条），不会静默消失。
            // **部分被拒也要报出来**（独立评审第 5 条）：原先只有「一条都没成」才 ok:false，
            // `refusals` 直接丢——10 条里落 3 条、7 条被「已经在待确认里了」拒时，日志会写
            // 「提交 3 条，被判据拒 0 条」（那个数是解析阶段的），7 条不知去向，而任务照常 finish、
            // 这段内容再也不会被重看。
            return {
              ok: true,
              submitted,
              refused: refusals.length,
              ...(refusals.length > 0 ? { refusedReason: refusals[0] } : {}),
            };
          },
        },
      });
    } finally {
      clearTimeout(timer);
      clearInterval(renewTimer);
    }

    if (result.ok) {
      await callMemory({ credentials: creds, route: 'jobs/finish', params: { token } });
      // 三个数分开报：**解析阶段**被拒（模型的输出不合 schema）、**提交阶段**被拒（壳的业务闸）、
      // 以及真的落库的条数。合起来报的话，「提交 3 条，被判据拒 0 条」会把 7 条的去向藏起来。
      ctx.logger?.info?.(
        `xlink-memory: 提取完成（落库 ${result.submitted} 条；解析阶段拒 ${result.rejected.length} 条；` +
          `提交阶段拒 ${result.refused ?? 0} 条；对照既有记忆 ${result.compared ?? 0} 条` +
          `、待确认候选 ${result.againstPending ?? 0} 条` +
          `${result.refusedReason ? `：${result.refusedReason}` : ''}）`
      );
    } else {
      // **stage 与原因一起报**：壳那边把这条写进 `jobs.json` 的 error，而插件的 logger 在这个
      // 内核里不进 stdout——排障的人只能看这一条，缺了 stage 就只剩半句话（§8 的「出事了查什么」）。
      await callMemory({
        credentials: creds,
        route: 'jobs/fail',
        params: { token, error: `${result.stage}：${result.reason ?? ''}` },
      });
      ctx.logger?.warn?.(`xlink-memory: 提取失败于「${result.stage}」：${result.reason}`);
    }
  }

  /**
   * 提取**前**的对照检索：既有记忆（C5）+ 待确认候选（C6）。
   *
   * 一次调用、两个集合，原因在 `?pending=1`：那条路本来就是按**同一个会话的工作区**检索的，
   * 也就是模型平时会拿到的那批记忆；另开一条只会得到两个口径，而「冲突/取代」判在两套口径
   * 之间只会更说不清。待确认候选**不进 `text`**，所以运行时上下文一个字都不变。
   *
   * **为什么不新加路由**：`/context` 本来就是这条路——它按**同一个会话的工作区**检索，
   * 也就是模型平时会拿到的那批记忆；另开一条只会得到两个口径，而「冲突」判在两套口径之间
   * 只会更说不清。它的 `hits` 里带 `id`/`title`/`body`，所以 id 也拿得到。
   *
   * **查询取切片里用户说过的话**（而不是整段原文）：整段含助手的话与工具痕迹，词更杂、
   * 命中更少；用户刚说的那句才是「可能与库里某条冲突」的入口。上限按壳那边的
   * `MAX_QUERY_CHARS` 收，超了它在路由里会被拒。
   *
   * @returns {Promise<{ok: boolean, items: object[], reason?: string}>}
   */
  async function retrieveForExtract({ creds, workspaceId, workspacePath, text }) {
    const query = conflictQuery(text);
    if (!query) return { ok: true, items: [], pending: [] };
    const answer = await callMemory({
      credentials: creds,
      route: 'context',
      // **`pending=1` 只为这一次要**：壳把待确认候选一并带回来，模型才认得出「用户是不是
      // 在改口」。运行时注入不带这个参数，所以那一次的响应一个字都没变——未确认的东西
      // 不许影响模型平时看到什么。
      params: { q: query, workspace: workspacePath, workspace_id: workspaceId, pending: '1' },
    });
    if (!answer.ok) {
      return { ok: false, items: [], pending: [], reason: answer.reason ?? '检索既有记忆失败' };
    }
    const rows = (list) =>
      (Array.isArray(list) ? list : [])
        .filter((row) => row && typeof row.id === 'string' && row.id)
        .map((row) => ({ id: row.id, title: row.title ?? '', body: row.body ?? '' }));
    return {
      ok: true,
      items: rows(answer.value?.hits),
      pending: rows(answer.value?.pending),
    };
  }

  async function enqueueExtraction(creds, session, cursor) {
    const result = await callMemory({
      credentials: creds,
      route: 'jobs/enqueue',
      params: {
        session: String(session?.id ?? ''),
        cursor,
        extractor: EXTRACTOR_VERSION,
      },
    });
    if (!result.ok) {
      ctx.logger?.warn?.(`xlink-memory: 排提取任务失败：${result.reason}`);
      return;
    }
    if (result.value?.outcome === 'disabled') {
      ctx.logger?.info?.('xlink-memory: 记忆开关关着，这次不排提取任务');
    }
  }

  // 提取要用的内核服务。**分开注入**：`workspaceRegistry` 那条路是注入用的，缺了只是没有记忆；
  // 这一组缺了只是没有自动提取——两件事的降级方向不同，别绑在一起。
  // **用时现取，不做快照**。这里踩过两次：
  // ① 第一版把 `inject` 回调拿到的**子上下文**存下来，事后读服务抛「inactive context」；
  // ② 第二版改成回调里 `ctx.get(...)` 急取快照——**还是不行**：`ctx.get` 返回的是**绑定当时
  //    那个 fiber 的追踪代理**，而服务 fiber 事后会重载（`_checkImpl` 把失效的从 store 里删掉），
  //    于是快照下来的是一个已经失活的对象，用的时候照样抛同一句话。
  //    两次都是「两侧端到端跑」才露头的：单测里根本没有内核，快照与现取看不出区别。
  // 现取时 `ctx.get(name)` 默认会**跳过失活的服务**（拿不到就回 undefined），调用方因此能
  // 如实说「内核没给这个服务」，而不是拿一个死对象去用。
  ctx.inject(['sessions', 'sessionPersistence', 'sessionQuery', 'llm', 'agentDefaultModel'], () => {});
  const liveExtractionServices = () => ({
    sessions: ctx.get('sessions'),
    sessionPersistence: ctx.get('sessionPersistence'),
    sessionQuery: ctx.get('sessionQuery'),
    llm: ctx.get('llm'),
    agentDefaultModel: ctx.get('agentDefaultModel'),
    workspaceRegistry: ctx.get('workspaceRegistry'),
  });

  ctx.inject(['workspaceRegistry'], (scoped) => {
    workspaceRegistry = scoped.workspaceRegistry;
  });

  ctx.on('agent/created', ({ agent }) => {
    // **整个注册过程不许抛**（见模块注释）：这里任何异常都只降级成「本会话没有记忆」。
    try {
      if (typeof agent?.ctx?.systemPrompt?.context !== 'function') {
        ctx.logger?.warn?.(`${name}: 这个内核没有 agent 作用域的上下文注册面，本次不注入记忆`);
        return;
      }
      const box = {
        contextText: '',
        state: { turn: -1, text: '', injections: 0, bytes: 0, workspace: '', revision: null },
        handoff: { turn: -1, text: '', injections: 0, bytes: 0, workspace: '', revision: null },
        // P2 自动提取的会话状态：最近一条**真的人类消息**的序号、以及最后一次排任务用掉的游标。
        extract: emptyExtractState(),
      };
      const sessionId = String(agent.session?.id ?? '');
      if (sessionId) {
        // 上限只是防长期不销毁的壳里堆小对象：根 scope 上拿不到可靠的「agent 已销毁」钩子
        // （监听器自己会随 fiber 释放，但这张表不是监听器）。挤掉的会话退回「压缩后不重置」
        // 的旧行为——宁可少一次重置，也不要无上限地留。
        if (agentStates.size >= MAX_TRACKED_SESSIONS) {
          const evicted = agentStates.keys().next().value;
          agentStates.delete(evicted);
          // **它的静默期唤醒也一起清掉**：留着就是一个没有会话状态的定时器，到点了
          // 拿着旧的工作区去领一次（`session/event` 那条路这时已经 `if (!box) return` 了）。
          wakeups.clear(evicted);
        }
        agentStates.set(sessionId, box);
      }

      agent.ctx.systemPrompt.context({
        name: CONTEXT_NAME,
        order: CONTEXT_ORDER,
        interpolate: false,
        text: () => box.contextText,
      });

      agent.ctx.on('agent/pre-step', async ({ agent: subject, messages, turn }, next) => {
        // 先拿到别的插件的决定，再检索，最后原样返回——排序与拒绝不归这里管。
        const decision = await next();
        try {
          if (!credentials) return decision;
          // **子智能体的读范围按工作区判等**（设计 §5，2026-10-15 拍板）：算出来的是「目录 +
          // 工作区 id」，两者一起进缓存与检索。**提取提交的坐标也取自这份缓存**
          // （`box.state.workspace`，见 `runDueExtraction` 的调用点），所以
          // 「候选的范围不会超出这一步注入的范围」是同一条规则的两个出口。
          const scope = await memoryScope(ctx, workspaceRegistry, subject ?? agent);
          const previousMemory = box.state.text;
          const previousHandoff = box.handoff.text;
          const firstRead = box.state.revision === null;
          if (box.state.workspace !== scope.workspace) box.contextText = '';
          // **换目录立刻生效，哪怕预算已经用完**（`beginStep` 里写回了状态）。
          // 只在「能重新检索」的分支里写回时，「预算用满 + 换目录」会让上一个项目的
          // 正文一路留在这个会话里——那正是这条要挡的事，而它只在接线处看得见。
          const step = beginStep(box.state, scope.workspace, turn);
          box.state = step.state;
          const handoffStep = beginStep(box.handoff, scope.workspace, turn, 1);
          box.handoff = handoffStep.state;
          const handoffDue = handoffStep.refresh || shouldProbeLibrary(box.handoff, turn, undefined, 1);
          // 两条路都取一次检索，区别只在**结果怎么用**：
          // · 回合预算内（`step.refresh`）：按这一步的消息检索，取到什么就用什么；
          // · 预算用完之后（`shouldProbeLibrary`）：只看**数据代次**——用户在会话里批准
          //   一条候选、删掉一条、关掉开关，代次才动，注入块才跟着换。查询每回合都不同、
          //   正文必然不同，拿「文本变了」当判据会把回合预算整个绕过去。
          const probe = !step.refresh;
          const memoryDue = !probe || shouldProbeLibrary(box.state, turn);
          if (!memoryDue && !handoffDue) return decision;
          const result = await callMemory({
            credentials,
            route: 'context',
            params: {
              // `userText` 已按 MAX_QUERY_CHARS 截尾：查询串超 8 KB 时桥会直接
              // 掐断连接且不回一个字，那一整个回合就会静默地没有记忆。
              q: userText(messages),
              workspace_id: scope.workspaceId,
              workspace: scope.workspace,
            },
          });
          // 「取回来的结果怎么用」整条判断在 `applyStep` 里（纯函数、有测试）：
          // 回合内取到什么用什么；探测那条路只在数据代次变了时才换正文。
          const applied = memoryDue ? applyStep(
            box.state,
            turn,
            probe ? 'probe' : 'turn',
            result.ok
              ? {
                  ok: true,
                  enabled: !!result.value?.enabled,
                  text: [result.value?.text, result.value?.experience_text].filter(Boolean).join(''),
                  generation: result.value?.generation,
                }
              : { ok: false },
          ) : { state: box.state, applied: false };
          box.state = applied.state;
          if (handoffDue) {
            box.handoff = applyStep(box.handoff, turn, handoffStep.refresh ? 'turn' : 'probe',
              result.ok ? { ok: true, enabled: !!result.value?.enabled, text: result.value?.handoff_text, generation: result.value?.generation } : { ok: false }).state;
          }
          const delta = contextDelta(previousMemory, previousHandoff, box.state.text, box.handoff.text);
          if (delta !== null) box.contextText = delta;
          if (!result.ok) {
            ctx.logger?.warn?.(`${name}: 本次检索失败，本回合不注入记忆：${result.reason}`);
          }
          // 「这一次的结果**真的进了上下文**」要回来说一声，壳那边记一条（面板的
          // 「最近注入」卡片靠它）。**只报 id 与开关状态，不报正文**——正文是用户私有
          // 内容，标题由壳按 id 从库里取。探测那条路没被采用时 `applied` 为 false，
          // 所以这里记下的正好是「模型实际看到了什么」，不含每回合的探测噪声。
          // 失败只 warn：这是一条可见性信号，不能因为它把这一回合的记忆搞没。
          if (result.ok && (delta !== null || firstRead)) {
            const noted = await callMemory({
              credentials,
              route: 'note',
              params: {
                session: String((subject ?? agent)?.session?.id ?? ''),
                workspace: scope.workspace,
                ids: [
                  ...(box.state.text !== previousMemory ? [...(result.value?.hits ?? []), ...(result.value?.experience_hits ?? [])] : []),
                  ...(box.handoff.text !== previousHandoff && result.value?.handoff_id ? [{ id: result.value.handoff_id }] : []),
                ].map((hit) => hit.id).filter(Boolean).join(','),
                enabled: result.value?.enabled ? '1' : '0',
              },
            });
            if (!noted.ok) {
              ctx.logger?.warn?.(
                `${name}: 注入记录没有落到壳里（面板的「最近注入」会缺这一条）：${noted.reason}`
              );
            }
          }
        } catch (error) {
          ctx.logger?.warn?.(`${name}: 检索异常，本回合不注入记忆：${String(error?.message ?? error)}`);
        }
        return decision;
      });
    } catch (error) {
      ctx.logger?.warn?.(`${name}: 注册会话上下文失败，本会话没有记忆：${String(error?.message ?? error)}`);
    }
  });

  registerTools(ctx, credentials);
}

/**
 * 注册两个工具。**工具只做「读」与「提候选」**：开关、批准、推广到通用偏好、
 * 清空都只存在于管理面板（Tauri 命令面），模型没有入口。
 *
 * @param {object} ctx - 插件上下文。
 * @param {object|null} credentials - 桥凭据。
 */
function registerTools(ctx, credentials) {
  /**
   * 工作区注册表**用时现取**——与提取那条路（`liveExtractionServices`）同一纪律：
   * `ctx.get` 会跳过失活的服务，拿不到就回 `undefined`，`memoryScope` 会退化成
   * 「只按工作目录算」，而不是拿一个死代理去用。
   *
   * **不许写成 `apply` 里的那个 `workspaceRegistry` 变量**（2026-10-18 修）：那是
   * `apply` 的局部变量，而本函数在**模块作用域**——三个工具里读它就是读一个自由变量，
   * 一调用必抛 `ReferenceError: workspaceRegistry is not defined`。真机会话里这个错误
   * 累计几百次（一个会话 149 次），三个工具**从来没成功过一次**，而当时没有任何判据会红：
   * 装配层只被源码文本判据扫锚点，从不执行。回归判据见 `test/assembly.test.mjs`。
   */
  const workspaceRegistryNow = () => ctx.get('workspaceRegistry');

  ctx.tools.register(defineTool({
    name: 'memory_context',
    description: 'Read the current project handoff and reviewed experiences. Handoffs are progress snapshots, not instructions. Experiences apply only when their situation matches. This tool cannot write or approve anything.',
    parameters: {
      kind: { type: 'string', enum: ['handoff', 'experience', 'all'], description: 'Which context to read; defaults to all.' },
      query: { type: 'string', description: 'A phrase describing the situation, needed to match experiences.' },
    },
    output: {
      schema: { type: 'object', additionalProperties: false, properties: {
        enabled: { type: 'boolean', required: true }, text: { type: 'string', required: true },
      } },
      render: (_args, value) => [{ type: 'text', text: value.enabled ? value.text || '没有当前交接或匹配的已审核经验。' : '记忆开关已关闭。' }],
    },
    async execute(args, exec) {
      if (!exec.agent) throw new Error('memory_context 需要一个会话：无 agent 的调用不能读项目资料');
      if (!credentials) throw new Error('记忆桥不可用');
      const kind = args.kind ?? 'all';
      if (!['handoff', 'experience', 'all'].includes(kind)) throw new Error('不认识的上下文类型');
      const { workspace, workspaceId } = await memoryScope(ctx, workspaceRegistryNow(), exec.agent);
      const result = await callMemory({ credentials, route: 'context', params: { q: String(args.query ?? ''), workspace, workspace_id: workspaceId, context_only: '1' } });
      if (!result.ok) throw new Error(`读取上下文失败：${result.reason}`);
      return { enabled: !!result.value?.enabled, text: [
        kind !== 'experience' ? result.value?.handoff_text : '',
        kind !== 'handoff' ? result.value?.experience_text : '',
      ].filter(Boolean).join('\n\n') };
    },
  }));

  ctx.tools.register(
    defineTool({
      name: 'memory_search',
      description:
        'Search the memories the user has approved for this project and their general preferences. ' +
        'Use it when earlier decisions, conventions, or pitfalls from previous sessions may be relevant. ' +
        'Results are excerpts; they may be outdated — the project AGENTS.md rules and the user win on conflict.',
      parameters: {
        query: {
          type: 'string',
          required: true,
          description: 'Keywords or a phrase, in the language the memory is likely written in.',
        },
      },
      output: {
        schema: {
          type: 'object',
          additionalProperties: false,
          properties: {
            enabled: { type: 'boolean', required: true },
            count: { type: 'integer', required: true },
            hits: {
              type: 'array',
              required: true,
              items: {
                type: 'object',
                additionalProperties: false,
                properties: {
                  id: { type: 'string', required: true },
                  title: { type: 'string', required: true },
                  scope: { type: 'string', required: true },
                  body: { type: 'string', required: true },
                  verification: { type: 'string', required: true },
                  updated_at: { type: 'string', required: true },
                  truncated: { type: 'boolean', required: true },
                },
              },
            },
          },
        },
        render: (_args, value) => [
          {
            type: 'text',
            text: value.enabled
              ? value.count === 0
                ? '记忆里没有与这次查询相关的内容。'
                : value.hits
                    .map(
                      (hit) =>
                        `[${hit.id}] ${hit.title}（${scopeLabel(hit.scope)} · ${verificationLabel(hit.verification)}，更新于 ${hit.updated_at}）\n${hit.body}` +
                        (hit.truncated ? '\n…（本条正文被截断，需要全文用 memory_read 读）' : ''),
                    )
                    .join('\n\n')
              : '这个实例的「使用已批准的记忆」开关是关着的：没有可检索的内容。',
          },
        ],
      },
      async execute(args, exec) {
        if (!exec.agent) throw new Error('memory_search 需要一个会话：无 agent 的调用不能读项目记忆');
        if (!credentials) throw new Error('记忆桥不可用：本机壳没有为这个内核起回环桥');
        const { workspace, workspaceId } = await memoryScope(ctx, workspaceRegistryNow(), exec.agent);
        const result = await callMemory({
          credentials,
          route: 'context',
          params: {
            q: String(args.query ?? ''),
            workspace_id: workspaceId,
            workspace,
          },
        });
        if (!result.ok) throw new Error(`检索记忆失败：${result.reason}`);
        const hits = Array.isArray(result.value?.hits) ? result.value.hits : [];
        return {
          enabled: !!result.value?.enabled,
          count: hits.length,
          hits: hits.map((hit) => ({
            id: String(hit.id ?? ''),
            title: String(hit.title ?? ''),
            scope: String(hit.scope ?? ''),
            body: String(hit.body ?? ''),
            // 壳那侧一直带着这三项（`MemoryHit`），是这一层把它们丢了——检索结果因此
            // 看不出「正文被截断」与「这条什么时候更新的」（第二次评审第 5 条）。
            verification: String(hit.verification ?? ''),
            updated_at: String(hit.updated_at ?? ''),
            truncated: !!hit.truncated,
          })),
        };
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: 'memory_read',
      description:
        'Read one memory in full by its id (ids come from memory_search). ' +
        'Use it when an excerpt looks relevant but is cut off, or when you need the exact wording of a convention. ' +
        'Reading counts as explicit use, so prefer it over re-searching.',
      parameters: {
        id: {
          type: 'string',
          required: true,
          description: 'The id shown as [id] in memory_search results.',
        },
      },
      output: {
        schema: {
          type: 'object',
          additionalProperties: false,
          properties: {
            id: { type: 'string', required: true },
            title: { type: 'string', required: true },
            scope: { type: 'string', required: true },
            body: { type: 'string', required: true },
            state: { type: 'string', required: true },
            verification: { type: 'string', required: true },
            updated_at: { type: 'string', required: true },
            use_count: { type: 'integer', required: true },
          },
        },
        render: (_args, value) => [
          {
            type: 'text',
            text: `${value.title}（${scopeLabel(value.scope)} · ${verificationLabel(value.verification)} · 更新于 ${value.updated_at}）\n\n${value.body}`,
          },
        ],
      },
      async execute(args, exec) {
        if (!exec.agent) throw new Error('memory_read 需要一个会话：无 agent 的调用不能读项目记忆');
        if (!credentials) throw new Error('记忆桥不可用：本机壳没有为这个内核起回环桥');
        const { workspace, workspaceId } = await memoryScope(ctx, workspaceRegistryNow(), exec.agent);
        const result = await callMemory({
          credentials,
          route: 'read',
          params: {
            id: String(args.id ?? ''),
            // 桥按这两个坐标校验归属与状态：**按 id 读也要过项目与「是否停用」两道
            // 门**，否则在 B 项目里拿着 A 项目的 id 就能读出来，而用户在面板上按的
            // 「停用」也会被绕过（注入表头里有 id，会话历史里一直留着）。
            workspace_id: workspaceId,
            workspace,
          },
        });
        if (!result.ok) throw new Error(`读取记忆失败：${result.reason}`);
        return {
          id: String(result.value?.id ?? ''),
          title: String(result.value?.title ?? ''),
          scope: String(result.value?.scope ?? ''),
          body: String(result.value?.body ?? ''),
          state: String(result.value?.state ?? ''),
          verification: String(result.value?.verification ?? ''),
          updated_at: String(result.value?.updated_at ?? ''),
          use_count: Number(result.value?.use_count ?? 0),
        };
      },
    })
  );

  ctx.tools.register(
    defineTool({
      name: 'memory_propose',
      description:
        'Propose one durable memory or a structured experience for the user to review. An experience needs situation, action, observed outcome and reusable lesson; never claim an unverified attempt succeeded. ' +
        'The candidate is NOT saved until the user confirms it in the Memory panel, so never claim it is remembered. ' +
        'Propose when the user asks to remember something, when they state a lasting preference or correction, ' +
        'when a project convention or a pitfall you hit is clearly worth carrying into future sessions, ' +
        'or when the user has just corrected you about how they want things done. ' +
        'One convention per call — do not propose one-off task details, status updates, or anything already covered ' +
        'by the repository itself.',
      parameters: {
        kind: { type: 'string', enum: ['memory', 'experience'], description: 'memory (default) or experience; never handoff.' },
        situation: { type: 'string', description: 'For experience: when the lesson applies. Keep each structured field concise; the whole request shares the bridge size limit.' },
        action: { type: 'string', description: 'For experience: what was actually done.' },
        outcome: { type: 'string', description: 'For experience: observed result, including failures.' },
        lesson: { type: 'string', description: 'For experience: reusable lesson supported by that result.' },
        title: {
          type: 'string',
          required: true,
          description: 'One short line naming the memory.',
        },
        body: {
          type: 'string',
          // 这里**没有 `maxLength` 可用**：内核的 `StringValueSchemaSpec` 只有
          // `type` / `enum` / `const`（0.2.1-alpha.2 实读），长度只能写进描述。
          // 真正的上限来自通道而不是 store：桥的请求行 7000 字节，中文经百分号编码
          // 一个字约 9 字节，所以大约 700 字。写长了会拿到「这条记忆太长了」再重试
          // 一次——一句描述就能省掉那一轮。
          description:
            'Required for memory; omit for experience, which uses the four structured fields. Include the conditions under which it applies. Do not include secrets. ' +
            'Keep it under about 600 characters — one convention per memory, not a document.',
        },
        scope: {
          type: 'string',
          enum: ['project', 'general'],
          description: 'project for this repository only; general for all projects. Defaults to project for memory, general for experience.',
        },
      },
      output: {
        schema: {
          type: 'object',
          additionalProperties: false,
          properties: {
            id: { type: 'string', required: true },
            state: { type: 'string', required: true },
          },
        },
        render: (_args, value) => [
          {
            type: 'text',
            text: `已提交候选 ${value.id}（待确认）。请在管理面板的「记忆 → 待确认」里点保存，它才会生效。`,
          },
        ],
      },
      async execute(args, exec) {
        if (!exec.agent) throw new Error('memory_propose 需要一个会话：无 agent 的调用不能写记忆');
        if (!credentials) throw new Error('记忆桥不可用：本机壳没有为这个内核起回环桥');
        const { workspace, workspaceId } = await memoryScope(ctx, workspaceRegistryNow(), exec.agent);
        const result = await callMemory({
          credentials,
          route: 'propose',
          params: {
            kind: String(args.kind ?? 'memory'),
            situation: String(args.situation ?? ''),
            action: String(args.action ?? ''),
            outcome: String(args.outcome ?? ''),
            lesson: String(args.lesson ?? ''),
            title: String(args.title ?? ''),
            body: String(args.body ?? ''),
            scope: String(args.scope ?? ''),
            workspace,
            workspace_id: workspaceId,
            session: String(exec.agent.session?.id ?? ''),
          },
        });
        if (!result.ok) throw new Error(`提交候选失败：${result.reason}`);
        return {
          id: String(result.value?.id ?? ''),
          state: String(result.value?.state ?? 'pending'),
        };
      },
    })
  );
}

export { apply, inject, name, PACKAGE_ID };
