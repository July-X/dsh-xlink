/**
 * Ponytail 内置插件的 Host 适配器：把随包交付的技能正文注册进内核技能表。
 *
 * ## 为什么是「注册技能」而不是别的
 *
 * Ponytail 没有运行时代码：它的全部内容就是六个 `SKILL.md`。所以这里能做的、
 * 也只该做一件事——把随包交付的技能正文交给内核的技能表（`ctx.skills`），
 * 让它们在工作台里按需被模型或用户调用。
 *
 * 注册用 `registerProvider` 而不是 `register`：`register` 收的是**内存里的一段
 * 正文**，而我们的正文是磁盘上的文件（应用升级时壳会重写物化目录，provider
 * 每次读取都能拿到当前那份）。形状与内核自己的 `@deepseek-ai/dsh-skill-office`
 * 一致（`lib/index.js`：读 SKILL.md → 剥 frontmatter → 注册 bundled 技能），
 * 抄的是那一份的形状，不是自己另发明一套。
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
 * 上游的档位切换（`/ponytail lite|full|ultra`）与会话级常驻依赖 Claude Code /
 * Cursor 的生命周期钩子与宿主状态，dsh 侧没有对应面。这里给出的是规则集 +
 * 六个按需调用的技能，不含那套状态机。
 *
 * @module xlink-ponytail
 */
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { BUNDLED_SKILL_RANK } from "@deepseek-ai/dsh-skill";

import { createProvider } from "./skills.js";

/** 与 `plugins/ponytail/package.json` 的 `name`、接线行 id 三者同值。 */
const PACKAGE_ID = "xlink-ponytail";
const name = PACKAGE_ID;
/** 需要 `ctx.skills` 才能注册；不声明会在别的插件先跑时拿到 undefined。 */
const inject = ["skills"];
/** 技能目录（相对本文件）：包内 `skills/<name>/SKILL.md`。 */
const SKILLS_DIR = join(dirname(fileURLToPath(import.meta.url)), "..", "skills");

/**
 * 注册技能 provider。
 *
 * @param {object} ctx - 携带技能注册表（`ctx.skills`）的上下文。
 */
function apply(ctx) {
  const provider = createProvider(SKILLS_DIR, BUNDLED_SKILL_RANK);
  if (provider.candidates.length === 0) {
    ctx.logger?.warn?.(`${name}: skills/ 下没有可用的 SKILL.md，ponytail 不注册任何技能`);
    return;
  }
  ctx.skills.registerProvider(() => provider);
  ctx.logger?.info?.(
    `${name}: 已注册 ${provider.candidates.length} 个技能（${provider.candidates.map((c) => c.name).join(", ")}）`,
  );
}

export { apply, inject, name, PACKAGE_ID };