/**
 * 技能正文发现、frontmatter 解析与 provider 构造（纯文件操作，无第三方依赖）。
 *
 * 单独成文件有两个理由，都是为了让**真被测试执行的那份代码**就是运行时那份：
 * 1. `host/index.js` 的唯一外部 import 是 `@deepseek-ai/dsh-skill`（物化目录里
 *    由壳 peer 链接过来，仓库里并不存在）。解析器与 provider 工厂一旦也住在
 *    那个文件里，测试就得先 import 整个 index.js —— 而那在 `npm run check` 的
 *    环境里必然失败。住在这里，测试可以直接 import 它，跑的是**同一个
 *    provider**，不是测试里抄一遍的规则。
 * 2. provider 的两个方法是**同一批候选的两个视图**。2026-10-12 第一版把它们
 *    各写一遍（字段只填在 `get()` 里，`list()` 只回 name/description），真机
 *    启动立刻炸 `skill provider "dsh-ponytail" returned skill "ponytail" with a
 *    non-string source`——内核对 `list()` 的每个候选**逐字段**校验（name /
 *    description / invocation / source / rank / provider），少一个就在第一轮
 *    发现时抛 TypeError。而当时那条判据只读 index.js 的源码文本，看到
 *    `rank: BUNDLED_SKILL_RANK` 就在源码里，全绿。**两份实现 = 这个 bug**。
 *    现在候选只构造一次，两个方法都从它派生。
 *
 * 依赖形状：`parseSkill` 纯字符串 → 对象；`readCandidates` 碰磁盘；
 * `createProvider` 不碰任何外部包（rank 由调用方传进来）。
 */
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { join } from "node:path";

/** 内核对技能名的要求（`dsh-skill` 的 `isSkillName` 同一条规则）。 */
export const SKILL_NAME_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;

/** 技能目录 → SKILL.md 的固定形状（与内核 `dsh-skill-filesystem` 一致）。 */
export const SKILL_FILE = "SKILL.md";

/** 本 provider 在技能表里的名字。内核要求 `candidate.provider` 与它逐字相同。 */
export const PROVIDER_NAME = "dsh-ponytail";

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
 * 扫描技能目录，返回**完整的候选**（不只是 name / description）。
 *
 * 每个字段都是内核对 `list()` 的候选**逐字段校验**过的（`dsh-skill/lib/index.js`
 * 的 `validateCandidate`）：name 必须是 kebab-case、description 非空、
 * invocation 两个布尔、source 是字符串、rank 是有限数、provider 与 provider 名
 * 逐字相同、path 若在则必须是字符串。少填任何一项，工作台每次开新对话都会抛
 * `skill provider "…" returned skill "…" with a non-string source` 那一串
 * TypeError——所以候选在这里**构造一次**，`list()` 与 `get()` 都从它派生。
 *
 * 符号链接的目录**跳过**：内核那边把符号链接排除在发现之外（避免同一份内容被
 * 两条路径各认一次），这里跟它对齐，避免装出来的技能在内核那边少一个或多一个。
 *
 * @param {string} skillsDir - 含 `<name>/SKILL.md` 的目录。
 * @param {number} rank - 本 provider 的优先级（bundled = 600）。
 * @returns {Array<object>} 内核可直接验收的候选。
 */
export function readCandidates(skillsDir, rank) {
  if (!existsSync(skillsDir) || !statSync(skillsDir).isDirectory()) return [];
  const candidates = [];
  for (const entry of readdirSync(skillsDir, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.isSymbolicLink()) continue;
    const directory = join(skillsDir, entry.name);
    const path = join(directory, SKILL_FILE);
    if (!existsSync(path)) continue;
    const parsed = parseSkill(readFileSync(path, "utf8"));
    if (parsed === null) continue;
    candidates.push(candidate(parsed, directory, path, rank));
  }
  return candidates.sort((a, b) => a.name.localeCompare(b.name));
}

/** 一个候选的全部字段。**只在这里构造一次**——见 [`readCandidates`] 的注释。 */
function candidate({ name, description }, directory, path, rank) {
  return {
    name,
    description,
    // 模型能按需调用，用户也能显式调用（/ponytail-review 这类命令型技能）。
    invocation: { modelInvocable: true, userInvocable: true },
    provider: PROVIDER_NAME,
    source: "bundled",
    rank,
    resourceBase: { kind: "directory", path: directory },
    path,
    locator: path,
  };
}

/** 带中止信号的正文读取；读不出来返回 undefined 而不是抛错——技能表把
 *  undefined 理解成「这个技能没了」，而抛错会让整个发现过程失败。 */
async function readBody(path, signal) {
  const { readFile } = await import("node:fs/promises");
  try {
    return await readFile(path, { encoding: "utf8", signal });
  } catch {
    return undefined;
  }
}

/**
 * 构造技能 provider。
 *
 * 候选来自**两个**目录，并按这个顺序合并：
 * 1. `<root>/skills` —— 上游内容（2026-10-12 起由壳从上游拉取后物化在实例里；
 *    **不随应用分发**，所以这里可能还不存在）。
 * 2. `<root>/adapter/skills` —— 我们自己的技能（目前只有 `ponytail-update`：
 *    拉上游的那条命令）。它必须**独立于上游内容存在**：上游整个目录被换掉时，
 *    「怎么更新」这个入口不该跟着消失。
 *
 * 同名时**上游优先**（先加进来的赢）：`ponytail-update` 这类名字上游哪天也用
 * 了，用他们的比用我们的更合适——我们的那条要的是这个插件的桥。
 *
 * 两个方法共享同一批候选：`list()` 原样返回，`get()` 在其上去掉内核自己消费的
 * `rank` / `locator` 再补上 `content`。**正文现读**而不是启动时读进内存——
 * 物化目录可能在壳启动前被重写（`builtin::runtime::refresh_enabled` 与上游
 * 同步都会动它），读一次会拿到上一版的正文。
 *
 * @param {string} root - 插件根（物化目录）。
 * @param {number} rank - bundled 优先级（`BUNDLED_SKILL_RANK`）。
 * @returns {{name: string, candidates: Array<object>, list: Function, get: Function}}
 */
export function createProvider(root, rank) {
  const candidates = dedupe([
    ...readCandidates(join(root, "skills"), rank),
    ...readCandidates(join(root, "adapter", "skills"), rank),
  ]);
  return {
    name: PROVIDER_NAME,
    candidates,
    list: () => Promise.resolve(candidates),
    async get(candidate, options) {
      const { rank: _rank, locator, ...summary } = candidate;
      const raw = await readBody(locator ?? candidate.path, options?.signal);
      if (raw === undefined) return undefined;
      const parsed = parseSkill(raw);
      if (parsed === null) return undefined;
      return { ...summary, content: parsed.body };
    },
  };
}

/** 同名时**保留先到的**那份（上游在前）。 */
function dedupe(candidates) {
  const seen = new Set();
  return candidates.filter((item) => (seen.has(item.name) ? false : seen.add(item.name)));
}