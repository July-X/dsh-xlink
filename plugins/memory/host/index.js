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
import { callMemory, readCredentials } from './bridge.js';
import { EXTRACTION_SYSTEM_PROMPT, EXTRACTOR_VERSION as EXTRACT_VERSION, buildSlice, parseExtractionOutput } from './extract.js';
import { readSessionRange } from './sessionlog.js';
import { runExtraction } from './run.js';
import {
  afterCompaction,
  applyStep,
  isCompactionDone,
  beginStep,
  emptyExtractState,
  EXTRACTOR_VERSION,
  noteSessionEvent,
  shouldScheduleExtraction,
  resolveWorkspaceId,
  shouldProbeLibrary,
  userText,
} from './policy.js';

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
 * 这条 agent 会话的有效工作目录。
 *
 * 用**有效**目录而不是冻结的 `session.header.cwd`：用户可以在会话中途换目录
 * （`working-directory/change`），按冻结值检索会把 A 项目的记忆带进 B 项目——
 * 而那正是这套东西要防的事。服务拿不到时退到冻结值，绝不让它抛。
 *
 * @param {object} ctx - 插件上下文（声明了 `workingDirectory`）。
 * @param {object} agent - 会话所属 agent。
 * @returns {string}
 */
function workingDirectory(ctx, agent) {
  try {
    const value = ctx.workingDirectory?.get?.(agent.session);
    if (typeof value === 'string' && value) return value;
  } catch {
    // 老内核没有这个服务（或会话还没登记目录）：退到冻结的项目目录。
  }
  try {
    return agent.session?.header?.cwd || '';
  } catch {
    return '';
  }
}

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
    // **只在回合边界上领**：`session/event` 每个事件都会到这里（一个回合几十条），
    // 每来一条就去问一次「有到期任务吗」是白打几十个请求（第一版就是这样，13 次 claim）。
    // 记人类消息那条路仍然看**每个**事件——它要的正是事件流本身。
    const attempt = () => {
      if (event?.type !== 'turn/end') return;
      void runDueExtraction(
        credentials,
        { ...extractionServices, workspaceId: box.workspaceId },
        box.state.workspace
      );
    };
    if (!decision.schedule) {
      attempt();
      return;
    }
    box.extract = { ...box.extract, lastCursor: decision.cursor };
    void enqueueExtraction(credentials, session, decision.cursor).then(attempt, attempt);
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
   * 调一次提取模型——**一次性调用，不进用户会话**。
   *
   * 模板抄内核自己的 `session-title-llm`（它正是「不产生用户可见对话的一次性调用」）：
   * `prepareCall(route, signal, configure)` → `call.stream({...call.config, messages, system, signal})`。
   * 两处刻意的取舍：
   * · **`purpose` 不填**：设计不许冒充 `session-title`，也不许自造一个内核没登记过的用途。
   * · **`sessionId` 不传**：传了这次请求会被记进那个会话的日志；提取是插件自己的事，
   *   不该在用户的会话里留下一条谁也看不懂的记录。
   */
  async function callExtractionModel({ llm, route, text, signal }) {
    try {
      const call = await llm.prepareCall(
        { provider: route.provider, model: route.model, maxTokens: EXTRACT_MAX_TOKENS },
        signal
      );
      const messages = [
        createUserMessage({
          content: [{ type: 'text', text }],
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
  async function runDueExtraction(creds, services, workspace) {
    // 桥的返回是 `{ok, value}`（见 bridge.js），而 `/claim` 的形状是
    // `{token, key:{session,cursor,extractor}, attempts, expires_at_ms}`（见 jobs.rs 的 `Claimed`）。
    const claimed = await callMemory({ credentials: creds, route: 'jobs/claim', params: {} });
    const token = claimed.ok ? claimed.value?.token : null;
    if (typeof token !== 'string' || token.length === 0) return;

    const claimedKey = claimed.value?.key ?? {};
    const sessionId = String(claimedKey.session ?? '');
    const endSeq = Number(claimedKey.cursor ?? 0);
    const extractor = String(claimedKey.extractor ?? EXTRACT_VERSION);
    // 键里两处要分清：`session` 给读日志的那条路**要的是会话对象**（它读 `session.id`），
    // 而给桥的那条路要的是**字符串 id**。第一版把同一个字段既当对象又当字符串，
    // 展开顺序还把对象覆盖掉了——于是读到的是「这个会话没有 id」。
    const key = { session: { id: sessionId }, cursor: endSeq, extractor };
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), EXTRACT_TIMEOUT_MS);
    let result;
    try {
      const selection = services.agentDefaultModel?.currentSelection?.();
      result = await runExtraction({
        key: { session: { id: key.session }, ...key },
        from: 0,
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
          callModel: ({ text, signal }) =>
            selection
              ? callExtractionModel({ llm: services.llm, route: selection, text, signal })
              : { ok: false, reason: '拿不到当前模型路由（agentDefaultModel）' },
          parse: parseExtractionOutput,
          submit: async ({ candidates }) => {
            let submitted = 0;
            const refusals = [];
            for (const candidate of candidates) {
              // **一条候选一次调用**：壳那边的 `/jobs/submit` 就是这么收的（正文经查询串过去）。
              const answer = await callMemory({
                credentials: creds,
                route: 'jobs/submit',
                params: {
                  token,
                  session: sessionId,
                  start_seq: 0,
                  end_seq: endSeq,
                  scope: 'project',
                  workspace_id: services.workspaceId ?? '',
                  workspace: String(workspace ?? ''),
                  title: candidate.title,
                  body: candidate.body,
                  keywords: candidate.keywords.join(','),
                },
              });
              if (answer.ok && answer.value?.submitted === true) submitted += 1;
              else refusals.push(answer.value?.reason ?? answer.reason ?? '提交被拒');
            }
            if (refusals.length > 0 && submitted === 0) {
              return { ok: false, reason: refusals[0] };
            }
            return { ok: true, submitted };
          },
        },
      });
    } finally {
      clearTimeout(timer);
    }

    if (result.ok) {
      await callMemory({ credentials: creds, route: 'jobs/finish', params: { token } });
      ctx.logger?.info?.(`xlink-memory: 提取完成（提交 ${result.submitted} 条，被判据拒 ${result.rejected.length} 条）`);
    } else {
      await callMemory({ credentials: creds, route: 'jobs/fail', params: { token, error: result.reason ?? '' } });
      ctx.logger?.warn?.(`xlink-memory: 提取失败于「${result.stage}」：${result.reason}`);
    }
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
  const extractionServices = {};
  ctx.inject(['sessions', 'sessionPersistence', 'sessionQuery', 'llm', 'agentDefaultModel'], (scoped) => {
    Object.assign(extractionServices, {
      sessions: scoped.sessions,
      sessionPersistence: scoped.sessionPersistence,
      sessionQuery: scoped.sessionQuery,
      llm: scoped.llm,
      agentDefaultModel: scoped.agentDefaultModel,
    });
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
        state: { turn: -1, text: '', injections: 0, bytes: 0, workspace: '', revision: null },
        // P2 自动提取的会话状态：最近一条**真的人类消息**的序号、以及最后一次排任务用掉的游标。
        extract: emptyExtractState(),
      };
      const sessionId = String(agent.session?.id ?? '');
      if (sessionId) {
        // 上限只是防长期不销毁的壳里堆小对象：根 scope 上拿不到可靠的「agent 已销毁」钩子
        // （监听器自己会随 fiber 释放，但这张表不是监听器）。挤掉的会话退回「压缩后不重置」
        // 的旧行为——宁可少一次重置，也不要无上限地留。
        if (agentStates.size >= MAX_TRACKED_SESSIONS) {
          agentStates.delete(agentStates.keys().next().value);
        }
        agentStates.set(sessionId, box);
      }

      agent.ctx.systemPrompt.context({
        name: CONTEXT_NAME,
        order: CONTEXT_ORDER,
        interpolate: false,
        text: () => box.state.text,
      });

      agent.ctx.on('agent/pre-step', async ({ agent: subject, messages, turn }, next) => {
        // 先拿到别的插件的决定，再检索，最后原样返回——排序与拒绝不归这里管。
        const decision = await next();
        try {
          if (!credentials) return decision;
          const workspace = workingDirectory(ctx, subject ?? agent);
          // **换目录立刻生效，哪怕预算已经用完**（`beginStep` 里写回了状态）。
          // 只在「能重新检索」的分支里写回时，「预算用满 + 换目录」会让上一个项目的
          // 正文一路留在这个会话里——那正是这条要挡的事，而它只在接线处看得见。
          const step = beginStep(box.state, workspace, turn);
          box.state = step.state;
          // 两条路都取一次检索，区别只在**结果怎么用**：
          // · 回合预算内（`step.refresh`）：按这一步的消息检索，取到什么就用什么；
          // · 预算用完之后（`shouldProbeLibrary`）：只看**数据代次**——用户在会话里批准
          //   一条候选、删掉一条、关掉开关，代次才动，注入块才跟着换。查询每回合都不同、
          //   正文必然不同，拿「文本变了」当判据会把回合预算整个绕过去。
          const probe = !step.refresh;
          if (probe && !shouldProbeLibrary(box.state, turn)) return decision;
          const workspaceId = await resolveWorkspaceId(workspaceRegistry, workspace);
          // 提取提交候选时要用同一个 workspace_id（会话与工作区的对应关系是会话级的事实）。
          box.workspaceId = workspaceId;
          const result = await callMemory({
            credentials,
            route: 'context',
            params: {
              // `userText` 已按 MAX_QUERY_CHARS 截尾：查询串超 8 KB 时桥会直接
              // 掐断连接且不回一个字，那一整个回合就会静默地没有记忆。
              q: userText(messages),
              workspace_id: workspaceId,
              workspace,
            },
          });
          // 「取回来的结果怎么用」整条判断在 `applyStep` 里（纯函数、有测试）：
          // 回合内取到什么用什么；探测那条路只在数据代次变了时才换正文。
          const applied = applyStep(
            box.state,
            turn,
            probe ? 'probe' : 'turn',
            result.ok
              ? {
                  ok: true,
                  enabled: !!result.value?.enabled,
                  text: result.value?.text,
                  generation: result.value?.generation,
                }
              : { ok: false },
          );
          box.state = applied.state;
          if (!result.ok) {
            ctx.logger?.warn?.(`${name}: 本次检索失败，本回合不注入记忆：${result.reason}`);
          }
          // 「这一次的结果**真的进了上下文**」要回来说一声，壳那边记一条（面板的
          // 「最近注入」卡片靠它）。**只报 id 与开关状态，不报正文**——正文是用户私有
          // 内容，标题由壳按 id 从库里取。探测那条路没被采用时 `applied` 为 false，
          // 所以这里记下的正好是「模型实际看到了什么」，不含每回合的探测噪声。
          // 失败只 warn：这是一条可见性信号，不能因为它把这一回合的记忆搞没。
          if (applied.applied) {
            const noted = await callMemory({
              credentials,
              route: 'note',
              params: {
                session: String((subject ?? agent)?.session?.id ?? ''),
                workspace,
                ids: (result.value?.hits ?? []).map((hit) => hit.id).filter(Boolean).join(','),
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
  ctx.tools.register(
    defineTool({
      name: 'memory_search',
      description:
        'Search the memories the user has approved for this project and their general preferences. ' +
        'Use it when earlier decisions, conventions, or pitfalls from previous sessions may be relevant. ' +
        'Results are excerpts; they may be outdated — the current repository and the user win on conflict.',
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
                : value.hits.map((hit) => `[${hit.id}] ${hit.title}（${hit.scope}）\n${hit.body}`).join('\n\n')
              : '这个实例的「使用已批准的记忆」开关是关着的：没有可检索的内容。',
          },
        ],
      },
      async execute(args, exec) {
        if (!exec.agent) throw new Error('memory_search 需要一个会话：无 agent 的调用不能读项目记忆');
        if (!credentials) throw new Error('记忆桥不可用：本机壳没有为这个内核起回环桥');
        const workspace = workingDirectory(ctx, exec.agent);
        const result = await callMemory({
          credentials,
          route: 'context',
          params: {
            q: String(args.query ?? ''),
            workspace_id: await resolveWorkspaceId(workspaceRegistry, workspace),
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
            text: `${value.title}（${value.scope} · ${value.verification}）\n\n${value.body}`,
          },
        ],
      },
      async execute(args, exec) {
        if (!exec.agent) throw new Error('memory_read 需要一个会话：无 agent 的调用不能读项目记忆');
        if (!credentials) throw new Error('记忆桥不可用：本机壳没有为这个内核起回环桥');
        const workspace = workingDirectory(ctx, exec.agent);
        const result = await callMemory({
          credentials,
          route: 'read',
          params: {
            id: String(args.id ?? ''),
            // 桥按这两个坐标校验归属与状态：**按 id 读也要过项目与「是否停用」两道
            // 门**，否则在 B 项目里拿着 A 项目的 id 就能读出来，而用户在面板上按的
            // 「停用」也会被绕过（注入表头里有 id，会话历史里一直留着）。
            workspace_id: await resolveWorkspaceId(workspaceRegistry, workspace),
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
        'Propose one durable memory (a stable preference, a project convention, or a pitfall) for the user to review. ' +
        'The candidate is NOT saved until the user confirms it in the Memory panel, so never claim it is remembered. ' +
        'Propose when the user asks to remember something, when they state a lasting preference or correction, ' +
        'when a project convention or a pitfall you hit is clearly worth carrying into future sessions, ' +
        'or when the user has just corrected you about how they want things done. ' +
        'One convention per call — do not propose one-off task details, status updates, or anything already covered ' +
        'by the repository itself.',
      parameters: {
        title: {
          type: 'string',
          required: true,
          description: 'One short line naming the memory.',
        },
        body: {
          type: 'string',
          required: true,
          // 这里**没有 `maxLength` 可用**：内核的 `StringValueSchemaSpec` 只有
          // `type` / `enum` / `const`（0.2.1-alpha.2 实读），长度只能写进描述。
          // 真正的上限来自通道而不是 store：桥的请求行 7000 字节，中文经百分号编码
          // 一个字约 9 字节，所以大约 700 字。写长了会拿到「这条记忆太长了」再重试
          // 一次——一句描述就能省掉那一轮。
          description:
            'The content itself, including the conditions under which it applies. Do not include secrets. ' +
            'Keep it under about 600 characters — one convention per memory, not a document.',
        },
        scope: {
          type: 'string',
          enum: ['project', 'general'],
          description: 'project (default) for this repository only; general for a preference that applies everywhere.',
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
        const workspace = workingDirectory(ctx, exec.agent);
        const result = await callMemory({
          credentials,
          route: 'propose',
          params: {
            title: String(args.title ?? ''),
            body: String(args.body ?? ''),
            scope: String(args.scope ?? ''),
            workspace,
            workspace_id: await resolveWorkspaceId(workspaceRegistry, workspace),
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
