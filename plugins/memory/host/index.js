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

import { callMemory, readCredentials } from './bridge.js';
import {
  MAX_INJECTIONS_PER_SESSION,
  applyFailure,
  applyResult,
  resolveWorkspaceId,
  shouldRefresh,
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
      let state = { turn: -1, text: '', injections: 0 };

      agent.ctx.systemPrompt.context({
        name: CONTEXT_NAME,
        order: CONTEXT_ORDER,
        interpolate: false,
        text: () => state.text,
      });

      agent.ctx.on('agent/pre-step', async ({ agent: subject, messages, turn }, next) => {
        // 先拿到别的插件的决定，再检索，最后原样返回——排序与拒绝不归这里管。
        const decision = await next();
        try {
          if (!credentials) return decision;
          if (!shouldRefresh(state, turn, MAX_INJECTIONS_PER_SESSION)) return decision;
          const workspace = workingDirectory(ctx, subject ?? agent);
          const workspaceId = await resolveWorkspaceId(workspaceRegistry, workspace);
          const result = await callMemory({
            credentials,
            route: 'context',
            params: {
              q: userText(messages),
              id: workspaceId,
              workspace,
            },
          });
          state = result.ok
            ? applyResult(state, turn, result.value?.enabled ? String(result.value.text ?? '') : '')
            : applyFailure(state, turn);
          if (!result.ok) {
            ctx.logger?.warn?.(`${name}: 本次检索失败，本回合不注入记忆：${result.reason}`);
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
            id: await resolveWorkspaceId(workspaceRegistry, workspace),
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
            // 桥按这两个坐标校验归属：**按 id 读也要过项目这道门**，否则在 B 项目里
            // 拿着 A 项目的 id 就能读出来。
            id_workspace: await resolveWorkspaceId(workspaceRegistry, workspace),
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
        'Use it when the user asks to remember something, or when a decision is clearly worth carrying into future sessions.',
      parameters: {
        title: {
          type: 'string',
          required: true,
          description: 'One short line naming the memory.',
        },
        body: {
          type: 'string',
          required: true,
          description: 'The content itself, including the conditions under which it applies. Do not include secrets.',
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
