/**
 * 技能正文发现与 frontmatter 解析（纯文件操作，无第三方依赖）。
 *
 * 单独成文件有两个理由，都是为了让**真被测试执行的那份代码**就是运行时那份：
 * 1. `host/index.js` 的唯一外部 import 是 `@deepseek-ai/dsh-skill`（物化目录里
 *    由壳 peer 链接过来，仓库里并不存在）。解析器一旦也住在那个文件里，测试
 *    就得先 import 整个 index.js —— 而那在 `npm run check` 的环境里必然失败。
 *    住在这里，测试可以直接 import 它，且**测的就是运行时用的那份实现**，
 *    不是测试里抄一遍的规则。
 * 2. 「解析不出来就跳过」这条纪律需要能被反向验（喂一份坏 frontmatter 看它
 *    真的被跳过），而那要在不触碰文件系统的情况下先有 parse 这一层。
 *
 * 依赖形状：`parseSkill` 纯字符串 → 字符串/对象；`readCandidates` 才碰磁盘。
 */
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join } from "node:path";

/** 内核对技能名的要求（`dsh-skill` 的 `isSkillName` 同一条规则）。 */
export const SKILL_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;

/** 技能目录 → SKILL.md 的固定形状（与内核 `dsh-skill-filesystem` 一致）。 */
export const SKILL_FILE = "SKILL.md";

/**
 * 剥 YAML frontmatter，**只取 `name` 与 `description` 两项**。
 *
 * 刻意不引 `yaml`：物化目录的 node_modules 里只有壳链过来的
 * `@deepseek-ai/dsh-skill` 一个 peer（见 `BuiltinSpec::peers` 的理由），从
 * 插件文件出发解析不到别的包。这两个键在 SKILL.md 里是固定形状——顶层标量，
 * 或 `>` / `|` 加折叠块——够用且没有依赖。
 *
 * **解析不出来返回 `null` 而不是猜**（少认不可乱认，与壳里
 * `skills::skill_frontmatter` 同一纪律）：注册一个没有名字或没有说明的技能，
 * 只会让工作台里出现一行用户读不懂的东西，而失败原因无处可查。
 *
 * @param {string} raw - SKILL.md 全文。
 * @returns {{name: string, description: string, body: string} | null}
 */
export function parseSkill(raw) {
  const match = /^---[ \t]*\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/u.exec(raw);
  if (match === null) return null;
  const fields = new Map();
  let pending = null;
  for (const line of match[1].split(/\r?\n/u)) {
    // 缩进行属于上一个键的块正文，不当作新的键——否则正文里出现的
    // `name:` / `description:` 会被读走（description 的折行里提到字段名很常见）。
    if (line.startsWith(" ") || line.startsWith("\t")) {
      if (pending !== null) pending.lines.push(line.trim());
      continue;
    }
    if (pending !== null) {
      fields.set(pending.key, pending.lines.join(" ").trim());
      pending = null;
    }
    const colon = line.indexOf(":");
    if (colon < 0) continue;
    const key = line.slice(0, colon).trim();
    const value = line.slice(colon + 1).trim();
    if (value === ">" || value === "|" || value === ">-" || value === "|-") {
      pending = { key, lines: [] };
    } else if (value !== "") {
      fields.set(key, value.replace(/^["']|["']$/gu, ""));
    }
  }
  if (pending !== null) fields.set(pending.key, pending.lines.join(" ").trim());

  const skillName = fields.get("name");
  const description = fields.get("description");
  if (typeof skillName !== "string" || !SKILL_NAME_RE.test(skillName)) return null;
  if (typeof description !== "string" || description.length === 0) return null;
  return { name: skillName, description, body: raw.slice(match[0].length).trim() };
}

/**
 * 扫描技能目录，返回可注册的候选。
 *
 * 符号链接的目录**跳过**：内核那边把符号链接排除在发现之外（避免同一份内容
 * 被两条路径各认一次），这里跟它对齐，避免装出来的技能在内核那边少一个或多一个。
 *
 * @param {string} skillsDir - 含 `<name>/SKILL.md` 的目录。
 * @returns {Array<{name: string, description: string, directory: string, path: string}>}
 */
export function readCandidates(skillsDir) {
  if (!existsSync(skillsDir) || !statSync(skillsDir).isDirectory()) return [];
  const candidates = [];
  for (const entry of readdirSync(skillsDir, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
    const directory = join(skillsDir, entry.name);
    const path = join(directory, SKILL_FILE);
    if (!existsSync(path)) continue;
    const parsed = parseSkill(readFileSync(path, "utf8"));
    if (parsed === null) continue;
    candidates.push({ directory, path, name: parsed.name, description: parsed.description });
  }
  return candidates.sort((a, b) => a.name.localeCompare(b.name));
}