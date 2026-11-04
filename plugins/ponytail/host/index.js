/**
 * Ponytail 内置插件的 Host 适配器：把上游的技能正文注册进内核技能表，并在有
 * 上游内容时把三个生命周期钩子挂到**内核原生扩展点**上。
 *
 * ## 这里发的是什么、不发什么
 *
 * **不发**上游内容。技能正文（六个 `SKILL.md`）与 hook 脚本都由**桌面壳**在启用
 * 时从上游拉取、物化进实例目录（`builtin::upstream`），本适配层只负责把它们
 * 接进内核。这样上游改一次不必重发一版 dsh-xlink，仓库里也不会躺着第二份与
 * 用户手上那份不同步的内容。
 *
 * 发的是：
 * · 随包的一份**适配层**（本文件 + `host/`、`adapter/`：更新脚本与
 *   `ponytail-update` 技能）。
 * · 从物化目录读到的上游内容（读到多少接多少——**读不到不是错误**：
 *   插件刚装、还没同步过时就是空的，此时只注册我们自己的 `ponytail-update`）。
 *
 * ## 为什么是「注册 provider」而不是「注册」
 *
 * `ctx.skills.register` 收的是**内存里的一段正文**，而我们的正文是磁盘上的文件
 * （壳启动内核前可能重写物化目录）。provider 每次读取都拿到当前那份，形状与内核
 * 自己的 `@deepseek-ai/dsh-skill-office` 一致。
 *
 * 本文件刻意只留「装配」：候选的字段、解析规则、provider 的两个方法都在
 * `skills.js` 里——那是**没有外部依赖**的一半，所以 `npm run check` 里的
 * 冒烟测试能 import 它并真的把 provider 跑一遍。2026-10-12 第一版把这些都写在
 * 这里，测试只好去读源码文本，于是「`list()` 少填了 source / rank / provider」
 * 这类错一路绿灯直到真机启动才炸（详见 skills.js 的模块注释）。
 *
 * ## rank 600（`BUNDLED_SKILL_RANK`）意味着什么
 *
 * 同名时 rank 小的赢：项目级 100/200、壳管理的活动视图 300、内核默认根 400/500、
 * bundled 600。所以**用户自己装的同名 `ponytail` 技能永远压过我们这份**——
 * 「随应用交付」不等于「强制生效」，用户改主意不用先卸载应用。
 *
 * ## hook 走原生扩展点，不走 `dsh-hooks-claude-code`
 *
 * 上游把规则注入/切档/子代理注入实现成三份 Claude Code 形态的 node 脚本。本适配
 * 层**直接监听内核的原生扩展点**，再按上游固定的 stdin/stdout 契约执行那三份脚本
 * （`host/hooks.js`）：
 *
 * · `agent/created`      → `SessionStart`（注入当前档位规则集）
 * · `agent/pre-step`     → `UserPromptSubmit`（识别切档命令，切换后注入新档规则集）
 * · `subagent/start`     → `SubagentStart`（把同一份规则集注入子代理）
 *
 * 内核从 0.2.1-alpha.2 起不再随带 `@deepseek-ai/dsh-hooks-claude-code`（上游把它
 * 降级成了 devDependency），我们也不再需要那个兼容桥：三个事件、字段与输出信封
 * 都由本适配层自己对接。**上游的 hook 机制按稳定契约对待**（2026-11-03 用户
 * 确认）：脚本路径、stdin 字段与 `hookSpecificOutput.additionalContext` 输出形状
 * 若上游改掉，这里只会**输出空、记一行 warn**（fail open），不会把插件或会话
 * 弄坏；改上游契约时必须同步 `host/hooks.js` 与 `test/hooks.test.mjs`。
 *
 * 三处环境（hook 进程继承内核进程环境；`host/hooks.js::hookChildEnv` 清掉
 * `DSH_*` 与 `NODE_OPTIONS`，只补三个显式变量）：
 *
 * · `CLAUDE_PLUGIN_ROOT` = 物化目录（上游脚本定位自己的 `hooks/` 用）。
 * · `PLUGIN_DATA` = 壳给的状态目录（`CLAUDE_CONFIG_DIR`）。上游按它切到 Codex
 *   方言：输出 `hookSpecificOutput` JSON，我们的解析器才接得住；档位状态也落在
 *   插件自己的 `state/` 下，而不是用户家目录的 `.claude/`。
 * · `CLAUDE_PROJECT_DIR` = 每个 hook 进程的工作区（`ctx.workingDirectory.ensure`），
 *   上游按它做「按仓库隔离」的档位文件。
 *
 * 「内核没给 `agents` / `workingDirectory` 服务」时不是错误：插件只注册技能，
 * hook 两项能力缺席（与上游内容尚未同步时同形）。本适配层不把这种缺席抛给 cordis。
 *
 * @module xlink-ponytail
 */
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { BUNDLED_SKILL_RANK } from "@deepseek-ai/dsh-skill";
import { createUserMessage } from "@deepseek-ai/dsh-llm";

import { createProvider } from "./skills.js";
import { applyLocale } from "./i18n.js";
import { removeBridgeCredential, writeBridgeCredential } from "./bridge-credential.js";
import { HOOK_SCRIPTS, hookPayload, runHookScript } from "./hooks.js";

/** 与 `plugins/ponytail/package.json` 的 `name`、接线行 id 三者同值。 */
const PACKAGE_ID = "xlink-ponytail";
const name = PACKAGE_ID;
/** 技能注册必需；hook 的 `agents` / `workingDirectory` 由 `ctx.inject` 按需拿。 */
const inject = ["skills"];
/** 插件根（物化目录），`${CLAUDE_PLUGIN_ROOT}` 替换成它。 */
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");

/** 把消息里的文本块拼成上游 `prompt` 字段；内核的 content 块可能不止文本。 */
function promptText(messages) {
  const texts = [];
  for (const message of messages || []) {
    for (const block of message?.content || []) {
      if (block?.type === "text" && typeof block.text === "string") {
        texts.push(block.text);
      }
    }
  }
  return texts.join("");
}

/** 注入一条上下文消息；source kind 与用户输入区分开（内核按它识别注入来源）。 */
function contextMessage(text) {
  return createUserMessage({
    content: [{ type: "text", text }],
    source: { kind: "xlink-ponytail" },
  });
}

/**
 * 把三个上游 hook 接到内核原生扩展点上。
 *
 * @param {object} ctx - 已注入 `agents` / `workingDirectory` 的 scoped 上下文。
 */
function attachHooks(ctx) {
  const hooksDir = join(ROOT, "hooks");
  if (!existsSync(hooksDir)) {
    ctx.logger?.info?.(`${name}: 还没有上游内容（尚未同步），本次不挂 hook`);
    return;
  }
  // 上游按宿主方言与状态目录判断行为（见模块注释）；三条命令都靠环境变量，
  // 必须在任何 hook 进程启动之前就位。
  process.env.CLAUDE_PLUGIN_ROOT = ROOT;
  if (process.env.CLAUDE_CONFIG_DIR !== undefined) {
    process.env.PLUGIN_DATA ??= process.env.CLAUDE_CONFIG_DIR;
  }

  // 插件卸载/内核退出时杀掉还在跑的 hook 子进程；不阻塞卸载。
  const disposed = new AbortController();
  ctx.effect?.(() => () => disposed.abort(), `${name}: 停止 hook 进程`);

  // 同一个事件的失败只报一次：UserPromptSubmit 每轮都会跑，逐轮刷 warn 会把
  // 真正有用的日志淹掉（与桥的「失败非阻塞」契约一致）。
  const warned = new Set();
  const warnOnce = (event, error) => {
    if (warned.has(event)) return;
    warned.add(event);
    ctx.logger?.warn?.(`${name}: ${event} hook 失败，本次会话继续（能力降级）：${String(error)}`);
  };

  /** 跑一个上游 hook；返回要注入的文本，任何失败都返回空串。 */
  const run = async (event, agent, extra, signal) => {
    if (!agent) return "";
    const script = join(hooksDir, HOOK_SCRIPTS[event]);
    if (!existsSync(script)) {
      warnOnce(event, `缺少 ${script}（上游尚未同步或改过了文件名）`);
      return "";
    }
    let cwd;
    try {
      cwd = await ctx.workingDirectory.ensure(agent, signal || disposed.signal);
    } catch {
      cwd = process.cwd();
    }
    const result = await runHookScript({
      event,
      script,
      payload: hookPayload(event, { agent, ...extra }),
      cwd,
      env: { CLAUDE_PROJECT_DIR: cwd, CLAUDE_PLUGIN_ROOT: ROOT },
      signal: signal || disposed.signal,
    });
    if (result.error) warnOnce(event, result.error);
    return result.context;
  };

  const injectContext = (agent, text) => {
    if (text) agent.inject(contextMessage(text));
  };

  // SessionStart：会话（含 resume / clear / compact）开始即注入当前档位规则集。
  ctx.on("agent/created", async ({ agent, source, signal }) => {
    try {
      injectContext(agent, await run("SessionStart", agent, { source }, signal));
    } catch (error) {
      warnOnce("SessionStart", error);
    }
  });

  // UserPromptSubmit：只在识别到切档命令时注入（上游脚本自己判断，普通消息
  // 输出 `{}`）。必须等别的插件做完决定再追加——排序与拒绝不归这里管。
  ctx.on("agent/pre-step", async ({ agent, messages, signal }, next) => {
    let text = "";
    const prompt = promptText(messages);
    if (prompt) {
      try {
        text = await run("UserPromptSubmit", agent, { prompt }, signal);
      } catch (error) {
        warnOnce("UserPromptSubmit", error);
      }
    }
    const downstream = await next();
    if (!text || downstream.kind !== "enter") return downstream;
    return { ...downstream, messages: [...downstream.messages, contextMessage(text)] };
  });

  // SubagentStart：父线程的 SessionStart 上下文不会传给子代理（上游 issue
  // #252），所以每个子代理启动时各注入一份。detached 执行，不拖慢子代理启动。
  ctx.on("subagent/start", (info) => {
    const child = ctx.get("agents")?.get(info.id);
    if (!child) return;
    void (async () => {
      try {
        injectContext(child, await run("SubagentStart", child, { subagent: { id: info.id } }));
      } catch (error) {
        warnOnce("SubagentStart", error);
      }
    })();
  });

  ctx.logger?.info?.(`${name}: hook 已挂到原生扩展点（${hooksDir}）`);
}

/**
 * 注册技能 provider。
 *
 * **先覆盖中文层再建 provider**：provider 的候选是在 `createProvider` 里从磁盘读
 * frontmatter 解析出来的，晚一步覆盖就已经进了 description，而技能列表显示的正是它。
 * 上游那条注入规则集的 hook 是会话开始时才跑的，它自己读盘，所以它读到的一定是
 * 覆盖之后的那一份——两处因此拿到同一个文件，不需要再各自处理。
 *
 * 覆盖失败不影响技能可用：applyLocale 内部逐文件兜住，真失败也只是保持上游原文。
 *
 * 顺带把**桥凭据**落一份盘（`bridge-credential.js`）：`/ponytail-update` 是模型在
 * 工作台里跑的，看不到内核的环境，盘上那份文件是它唯一的路。
 *
 * @param {object} ctx - 携带技能注册表（`ctx.skills`）的上下文。
 */
async function apply(ctx) {
  // 桥凭据先落盘，且**不参与**后面的成败判定：写不下去只记一行，技能照常注册。
  const credential = writeBridgeCredential(ROOT, process.env);
  if (credential.written) ctx.logger?.info?.(`${name}: ${credential.reason}`);
  else if (credential.failed) ctx.logger?.warn?.(`${name}: ${credential.reason}`);
  // 内核正常停止时收走这一代凭据；盘上那枚令牌不是我们的（换过壳、或内核刚重启）就不动。
  ctx.effect?.(() => () => removeBridgeCredential(ROOT, process.env), `${name}: 收走桥凭据`);

  const locale = applyLocale(ROOT, { logger: ctx.logger, dshHome: process.env.DSH_HOME });
  const provider = createProvider(ROOT, BUNDLED_SKILL_RANK);
  if (provider.candidates.length === 0) {
    ctx.logger?.warn?.(`${name}: 既没有上游内容也没有自带技能，ponytail 这次不注册任何技能`);
    return;
  }
  ctx.skills.registerProvider(() => provider);
  ctx.logger?.info?.(
    `${name}: 已注册 ${provider.candidates.length} 个技能（${provider.candidates.map((c) => c.name).join(", ")}）` +
      (locale.applied.length > 0 ? `；${locale.reason}` : ""),
  );
  // 服务齐全才挂 hook；缺服务只影响 hook 三项能力，技能照常（不抛给 cordis）。
  ctx.inject(["agents", "workingDirectory"], (scoped) => {
    try {
      attachHooks(scoped);
    } catch (error) {
      ctx.logger?.warn?.(`${name}: hook 挂载失败，规则集与档位切换不可用（技能不受影响）：${String(error)}`);
    }
  });
}

export { apply, inject, name, PACKAGE_ID };
