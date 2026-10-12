/**
 * 中文覆盖层：把上游英文技能正文换成 `i18n/zh-CN/` 里的中文副本。
 *
 * ## 为什么覆盖的是「物化目录里的那一份」，而不是在读取时改写
 *
 * 工作台里 ponytail 的三处 prompt 全部来自 `<root>/skills/<name>/SKILL.md` 这**同一批文件**：
 *
 * · 技能列表显示的 description —— `skills.js` 的 provider 从这里解析 frontmatter；
 * · 技能正文 —— provider 的 `get()` 读同一个文件；
 * · 会话开始注入的规则集 —— 上游 `hooks/ponytail-instructions.js` 里
 *   `SKILL_PATH = path.join(__dirname, '..', 'skills', 'ponytail', 'SKILL.md')`，
 *   **直接读盘、绕过 provider**。
 *
 * 所以只有把这批文件本身换成中文，三处才一起变中文。只在 provider 层拦截的话，
 * 前两处中文、第三处仍是英文 —— 而第三处恰恰是每轮会话开头进上下文的那份。
 * 这也是不 fork 上游 `ponytail-instructions.js` 的理由：换文件比换逻辑更不容易漂。
 *
 * ## 为什么不 vendor 进仓库（这条已经翻过一次）
 *
 * 上游内容本来是**不随应用分发**的，由壳从 npm 拉取并自动跟随更新（见 README）。
 * 中文层是这个决定的**延伸而不是推翻**：中文正文是我们自己的资产，而它**依据哪一版上游**
 * 由 manifest 逐文件记着指纹。上游动了 → 指纹对不上 → 回落英文（下面 §回落），
 * 而不是拿一份旧中文假装自己是最新的。
 *
 * ## 覆盖前必须备份原件
 *
 * 回落要能把英文**还原回去**，所以覆盖时先把上游原文存到 `<root>/.xlink-i18n/<locale>/upstream/`。
 * 不备份就等于删掉上游内容，而那时已经没人知道它原本是什么了。
 *
 * ## 陈旧靠指纹判定，不靠版本号
 *
 * 物化目录里**没有**上游版本号可读：`skills/plugin.json` 是上游的 skill 清单，
 * 没有 `version`；根上的 `package.json` 是**我们**的适配层，也不是。
 * 唯一可靠的事实是每个文件自己的 sha256 —— 译文记着它依据的那一版指纹，
 * 比对不上就是上游动了。这比版本号更准：上游改内容但不发版（或者发版没改这个文件）
 * 都能分辨，而版本号分辨不了。
 *
 * 上游版本号只用来把话说明白，从壳写的数据目录里**顺手**读，读不到就不说 ——
 * 消息缺一个版本号好过报一个猜来的。
 *
 * ## 幂等
 *
 * 每次内核启动都会跑一遍。所以判据必须能区分四种状态：
 * · 当前文件是我们上次写的中文 → 什么都不做；
 * · 当前文件是译文的新版本（我们改了中文层）→ 重写；
 * · 当前指纹等于 manifest 记的上游指纹 → 备份后覆盖；
 * · 三者都不是（上游动了，或目录被重物化）→ 回落英文。
 *
 * 写盘先写临时文件再 rename：崩溃在中间留下的半截 SKILL.md 会让规则集变成乱码，
 * 而那种失败是静默的（hook 失败不阻断会话）。
 *
 * @module xlink-ponytail/i18n
 */
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

/** 中文层相对插件根的位置。 */
export const LOCALE = "zh-CN";
const I18N_DIR = join("i18n", LOCALE);
/** 备份与状态标记：上游正文存在这里，不在 `skills/` 里，避免被当成技能被发现。 */
const STATE_DIR = ".xlink-i18n";

const SKILL_FILE = "SKILL.md";

/** sha256。用于「这份文件是不是译文所依据的那一版上游」。 */
export function digest(text) {
  return createHash("sha256").update(text, "utf8").digest("hex");
}

/**
 * 读中文层的清单。
 *
 * **读不出来返回 `null` 而不是空对象**：空对象会让调用方以为「有清单但一个文件都没有」，
 * 那与「没有中文层」是两种不同的事——前者该报错，后者是正常安装。
 *
 * @param {string} root 插件根（物化目录）。
 * @returns {{locale: string, upstream: object, files: Record<string,string>}|null}
 */
export function readManifest(root) {
  try {
    const manifest = JSON.parse(readFileSync(join(root, I18N_DIR, "manifest.json"), "utf8"));
    if (typeof manifest?.files !== "object" || manifest.files === null) return null;
    return manifest;
  } catch {
    return null;
  }
}

/** 原子写：临时文件 + rename。 */
function writeAtomic(path, content) {
  const tmp = `${path}.tmp-${process.pid}`;
  writeFileSync(tmp, content, "utf8");
  renameSync(tmp, path);
}

/**
 * 应用中文覆盖层。
 *
 * 纯文件操作，**不做任何猜测**：每个文件的处置都由它的 sha256 决定，
 * 四种情形之外一律不写。
 *
 * @param {string} root 插件根（物化目录）。
 * @param {object} [deps] 注入的 logger 与 dshHome，便于测试。
 * @returns {{applied: string[], restored: string[], skipped: string[], moved: string[],
 *            stale: boolean, reason: string, upstreamVersion: string|null}}
 */
export function applyLocale(root, deps = {}) {
  const log = deps.logger ?? { info() {}, warn() {} };
  const result = {
    applied: [],
    restored: [],
    skipped: [],
    moved: [],
    stale: false,
    reason: "",
    upstreamVersion: readUpstreamVersion(deps.dshHome ?? process.env.DSH_HOME),
  };
  const manifest = readManifest(root);
  if (manifest === null) {
    result.reason = "没有中文层清单，用上游原文";
    return result;
  }

  const backupDir = join(root, STATE_DIR, LOCALE, "upstream");
  const appliedMarker = join(root, STATE_DIR, LOCALE, "applied.json");
  // 上一轮我们写下去的译文指纹。没有它就分不出「已是中文」与「上游变了」。
  let applied;
  try {
    applied = JSON.parse(readFileSync(appliedMarker, "utf8"));
  } catch {
    applied = null;
  }

  for (const [name, upstreamSha] of Object.entries(manifest.files)) {
    const target = join(root, "skills", name, SKILL_FILE);
    if (!existsSync(target)) continue; // 上游还没这个技能：不凭空造
    const current = readFileSync(target, "utf8");
    const currentSha = digest(current);

    const translated = readTranslation(root, name);
    if (translated === null) continue; // 中文层缺这一份：保持上游原文

    // 已经是译文，且没变过 —— 什么都不做。
    if (currentSha === digest(translated)) {
      result.skipped.push(name);
      continue;
    }
    // 当前就是译文，但内容与现在这份中文层不同（我们改了中文层）→ 重写。
    if (applied?.files?.[name] === currentSha) {
      writeAtomic(target, translated);
      result.applied.push(name);
      continue;
    }
    // 当前内容正好是译文所依据的那一版上游 → 备份后覆盖。
    if (currentSha === upstreamSha) {
      try {
        mkdirSync(backupDir, { recursive: true });
        copyFileSync(target, join(backupDir, `${name}${SKILL_FILE}`));
        writeAtomic(target, translated);
        result.applied.push(name);
      } catch (error) {
        log.warn?.(`ponytail: ${name} 覆盖失败，保留上游原文：${String(error?.message ?? error)}`);
      }
      continue;
    }
    // 三者都不是：上游已经动了。
    result.moved.push(name);
    // 手上有覆盖前存下的原件 → 还原英文；没有的话当前这份**就是**新的上游原文，
    // 本来就是对的，不动它。
    const backup = join(backupDir, `${name}${SKILL_FILE}`);
    if (existsSync(backup)) {
      try {
        writeAtomic(target, readFileSync(backup, "utf8"));
        result.restored.push(name);
      } catch (error) {
        log.warn?.(`ponytail: ${name} 回落英文失败：${String(error?.message ?? error)}`);
      }
    } else {
      result.skipped.push(name);
    }
  }

  // 记下这一轮写下去的译文指纹，下次启动才分得出「已是中文」。
  const next = { locale: LOCALE, upstreamVersion: manifest.upstream?.version ?? null, files: {} };
  for (const name of [...result.applied, ...result.skipped]) {
    const target = join(root, "skills", name, SKILL_FILE);
    if (existsSync(target)) next.files[name] = digest(readFileSync(target, "utf8"));
  }
  try {
    mkdirSync(join(root, STATE_DIR, LOCALE), { recursive: true });
    writeAtomic(appliedMarker, `${JSON.stringify(next, null, 2)}\n`);
  } catch {
    /* 标记写不下去只影响下次判重，不影响本次内容已正确 */
  }

  result.stale = result.moved.length > 0;
  const base = manifest.upstream?.version ?? "未知版本";
  const installed = result.upstreamVersion;
  result.reason = result.stale
    ? `上游内容已更新（当前 ${installed ?? "未知版本"}，中文层对应 ${base}），${result.restored.length} 个技能回落英文`
    : `中文层 ${LOCALE} 对应上游 ${base}`;
  if (result.applied.length > 0 || result.restored.length > 0 || result.stale) {
    const level = result.stale ? "warn" : "info";
    log[level]?.(
      `ponytail: ${result.reason}（覆盖 ${result.applied.length} 个，未动 ${result.skipped.length} 个）`,
    );
  }
  return result;
}

/**
 * 壳记录的上游版本号，**只为把话说明白**，不参与任何判定。
 *
 * 物化目录里没有它：`skills/plugin.json` 是上游的技能清单（无 `version`），
 * 根上的 `package.json` 是我们自己的适配层。壳把版本写在
 * `<DSH_HOME>/plugins/builtin/<id>/upstream.json` 里。
 *
 * @param {string|undefined} dshHome
 * @returns {string|null} 读不到返回 `null`，**不猜**。
 */
export function readUpstreamVersion(dshHome) {
  if (!dshHome) return null;
  try {
    const state = JSON.parse(
      readFileSync(join(dshHome, "plugins", "builtin", "ponytail", "upstream.json"), "utf8"),
    );
    return typeof state?.version === "string" ? state.version : null;
  } catch {
    return null;
  }
}

/** 读一份中文译文，读不出来返回 `null`。 */
function readTranslation(root, name) {
  try {
    return readFileSync(join(root, I18N_DIR, "skills", name, SKILL_FILE), "utf8");
  } catch {
    return null;
  }
}

/**
 * 中文层里的技能名。与 manifest 的键一致，供测试与诊断用。
 * @param {string} root 插件根。
 * @returns {string[]}
 */
export function translatedSkills(root) {
  try {
    return readdirSync(join(root, I18N_DIR, "skills"), { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name);
  } catch {
    return [];
  }
}

/** 删掉覆盖状态（回滚到「每次启动重新判」的干净态）。测试与排障用。 */
export function clearLocaleState(root) {
  rmSync(join(root, STATE_DIR), { recursive: true, force: true });
}