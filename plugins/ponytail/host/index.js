/**
 * Ponytail 内置插件的 Host 适配器：把上游的技能正文注册进内核技能表，并在有
 * 上游 hook 脚本时把它们挂到内核的拦截面上。
 *
 * ## 这里发的是什么、不发什么
 *
 * **不发**上游内容。技能正文（六个 `SKILL.md`）与 hook 脚本都由**桌面壳**在启用
 * 时从上游拉取、物化进实例目录（`builtin::upstream`），本适配层只负责把它们
 * 接进内核。这样上游改一次不必重发一版 dsh-xlink，仓库里也不会躺着第二份与
 * 用户手上那份不同步的内容。
 *
 * 发的是：
 * · 随包的一份**适配层**（本文件 + `adapter/`：更新脚本与 `ponytail-update` 技能）。
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
 * ## 做不到的部分（面板说明里已如实写明）
 *
 * 上游另有两个形态（statusline 徽标、`ponytail-gain` 的基准对照依赖别家 agent 的
 * 状态栏）在本宿主没有对应面；规则集与档位切换则**可用**，靠的就是下面的 hook 桥。
 *
 * @module xlink-ponytail
 */
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { BUNDLED_SKILL_RANK } from "@deepseek-ai/dsh-skill";
// **整个命名空间挂上去，不取其中一项**：桥自己声明 `inject: ["shell",
// "sessionProjections"]`，而第一版写的是
// `ctx.plugin({ name, apply: applyClaudeCodeHooks }, …)` —— 那个对象字面量里
// 没有 inject，cordis 于是拒绝注入，真机表现为每次开新对话都报
// `cannot get property "shell" without inject`（2026-10-12 用户实测）。
// 手工包一层 = 手工抄一份属性，而抄漏的那份不报「你少写了 inject」，它报的是
// 一个看起来毫不相关的运行时错误。传命名空间，属性天然齐全。
import * as claudeCodeHooks from "@deepseek-ai/dsh-hooks-claude-code";

import { createProvider } from "./skills.js";
import { applyLocale } from "./i18n.js";
import { removeBridgeCredential, writeBridgeCredential } from "./bridge-credential.js";

/** 与 `plugins/ponytail/package.json` 的 `name`、接线行 id 三者同值。 */
const PACKAGE_ID = "xlink-ponytail";
const name = PACKAGE_ID;
/** 需要 `ctx.skills` 才能注册；hook 桥自己按需注入 shell / sessionProjections。 */
const inject = ["skills"];
/** 插件根（物化目录），`${CLAUDE_PLUGIN_ROOT}` 替换成它。 */
const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
/** 上游的 Claude Code 形态钩子配置（由壳在同步上游时物化进来）。 */
const HOOKS_CONFIG = join(ROOT, "hooks", "claude-codex-hooks.json");

/**
 * 把 ponytail 的 hook 挂到内核的拦截面上。
 *
 * 上游那三个脚本是**为 Claude Code 写的**（`SessionStart` 注入规则集、
 * `UserPromptSubmit` 切档位、`SubagentStart` 传给子 agent）。dsh 内核自带
 * `dsh-hooks-claude-code` 这个桥，要做的就是「跑 unmodified 的 Claude Code
 * hooks.json」，所以上游脚本一个字都不用改。核对过的三处（0.2.1-alpha.1 实读）：
 *
 * 1. **事件与 matcher**：`SessionStartSource = 'startup'|'resume'|'clear'|'compact'`，
 *    与上游 matcher `startup|resume|clear|compact` 的四个词逐字相同；桥按
 *    claude-code 方言把 `a|b|c` 当**字面** alternation 比对，因此能选中。
 *    `UserPromptSubmit` 的 matcher 被桥丢弃（该事件按 prompt 触发，与工具名无关）。
 * 2. **注入通道**：脚本吐 `hookSpecificOutput.additionalContext`，协议包解析该键，
 *    桥把它 `agent.inject(...)` 塞进这一会话的上下文——规则因此**只注入一次**，
 *    不是每轮重发（每轮重发在短 prompt 上可能比它省下的代码还贵，上游自己算过）。
 * 3. **`${CLAUDE_PLUGIN_ROOT}`**：桥的 `pluginRoot` 配置替换成物化目录。
 *
 * **桥不扫插件目录**：`configPath` 必填，没有「自动发现 `<plugin>/hooks.json`」
 * 这回事（那一条在内核里还是 TODO）。所以必须由这里显式挂上去。
 *
 * 挂的是**内核那个插件对象本身**（模块命名空间），不是手写的 `{ name, apply }`：
 * 桥声明了 `inject: ["shell", "sessionProjections"]`，手抄一份十有八九漏掉
 * `inject`，而漏掉的症状是 `cannot get property "shell" without inject` ——
 * 一条与「你少写了一个字段」看不出关系的运行时错误（2026-10-12 真机踩过）。
 *
 * `hooks/` 来自上游同步，**尚未同步时不存在**——那时不挂也不报错：技能仍可用，
 * 只是没有「会话启动即注入」与档位切换。
 *
 * @param {object} ctx - 插件上下文。
 */
function attachHooks(ctx) {
  if (!existsSync(HOOKS_CONFIG)) {
    ctx.logger?.info?.(`${name}: 还没有上游内容（尚未同步），本次不挂 hook`);
    return;
  }
  ctx.plugin(claudeCodeHooks, {
    configPath: HOOKS_CONFIG,
    pluginRoot: ROOT,
  });
  ctx.logger?.info?.(`${name}: hook 已挂载（${HOOKS_CONFIG}）`);
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
function apply(ctx) {
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
  try {
    attachHooks(ctx);
  } catch (error) {
    ctx.logger?.warn?.(`${name}: hook 挂载失败，规则集与档位切换不可用（技能不受影响）：${String(error)}`);
  }
}

export { apply, inject, name, PACKAGE_ID };
