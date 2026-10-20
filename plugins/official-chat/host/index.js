/**
 * dsh-xlink 官网对话引用插件（Host 侧）。
 *
 * ## 它有三条职责
 *
 * ### ① G1 起：让客户端半边被内核发现（锚点）
 *
 * 内核的 `dsh-client-modules` 是**扫 host Loader 的 entries** 找带 `dsh.client`
 * 声明的包（`dsh-client-modules/index.js@0.2.1-rc.2:105-119`、
 * `dsh-client-modules/index.js@0.2.1-rc.2:720-750`、
 * `dsh-client-modules/index.js@0.2.1-rc.2:762-780`；该文件 SHA-256
 * `D2D89461E1E7F15D6E24CD8AC3FAAA2B931E066D5B5300F3C38AEA29DEF38BCF`，
 * 现役 release 树
 * `C:\Users\zxx\.dsh-xlink\dsh\desktop\kernels\0.2.1-rc.2`）：Loader 行的
 * `name` 指向哪个文件，kernel 就沿它向上找最近的 `package.json`，那份清单里
 * 必须有 `dsh.client.platform === "web"` 与 `exports["./client"]`。
 *
 * 也就是说：**没有 host 半边，这个插件的 client/client.js 根本不会进浏览器的
 * 模块图**。本文件因此是「客户端半边存在」的锚点，不是一个空壳——它必须
 * 真的被内核 import 成功（import 失败时内核只记一行 warning，插件的开关在
 * 界面上照常显示「已启用」；那次事故记在 `builtins.rs` 里 memory 那条 spec 的
 * 注释上）。
 *
 * ### ② P1 起：固定快照的**唯一存储**
 *
 * 协议 `docs/features/official-chat/official-chat-p1-protocol.md` 的
 * **S-3** 规定「落盘后单一事实源 = 快照文件」——句柄的 pending/ready/failed
 * **不再另存内存状态**，由快照文件派生；**S-4** 规定「任何一次请求发生时，
 * 该 occurrence 指向的快照必须已落盘，且落盘内容与模型输入里的正文逐字节
 * 相同」。这两条都要求「有一个地方真的把快照写成文件」，那个地方就是本文件。
 *
 * ### ③ P1 起：**客户端命令的落点**（协议 §5 的五个命令）
 *
 * 协议 §5 写「客户端命令职责：`capabilities` / `catalog` / `capture` /
 * `cancel` / `preview`」。这五个命令在本文件注册的一条 RPC 通道上：
 * `RPC_CHANNEL`（`host/rpc.js`）。**通道机制是内核既有的 `connection` 服务**：
 *
 * · Host 侧：`ctx.connection.rpc.handle(channel, handler)`
 *   （`dsh-client-connection/lib/index.js@0.2.1-rc.2:724-729` 的 `rpc` getter；
 *   服务本身在 `dsh-client-connection/lib/index.js@0.2.1-rc.2:710` 的
 *   `super(ctx, "connection")`；注册落到 Web 服务器的路由在
 *   `dsh-client-connection/lib/index.js@0.2.1-rc.2:856-871`）。
 * · Browser 侧：`ctx.get('connection').rpc.call(channel, endpoint, payload)`
 *   （`dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1212-1231`，
 *   页面在 `dsh-client-connection/lib/client.browser.js@0.2.1-rc.2:1497`
 *   的 `ctx.provide("connection", handle)`）。
 *
 * **为什么不是「host 挂个服务、client 去 get」**：client 半边是**浏览器页面里
 * 的脚本**，host 半边是**内核 Node 进程里的插件**——两棵不同的 cordis 树，
 * 页面树里 `ctx.get('xlink-official-chat')` 永远解析不到（实测：G1 实跑里
 * 快照落盘一路失败，就是走的这条不存在的路）。而 cordis 的 `ctx.set(name, value)`
 * 在该属性没被 `provide` 过时**直接抛错**
 * （`cordis/lib/index.js@0.2.1-rc.2:782-789`：`cannot set property "..." without
 * provide`），所以「host set + client get」这条路**两头都不通**。
 *
 * ### ④ P1 v0.2 起：**快照 id 的分配权**（协议 §2 S-1）
 *
 * v0.2 明写「**`seq` 由 host 侧统一分配**（host 持有并持久化已用序号，
 * **client 不得自造**）；「本实例内单调递增」必须**跨进程**成立」。
 *
 * 分配器在 `host/seq-store.js`：**排他创建**（`wx`）逐个认领序号并落盘，
 * 于是 dev 壳与 release 壳**同开**、或任一进程重启，都不会发出同一个
 * `snapshotId`（那会让 host 按「同 id 不重写」拒掉，用户看到「这次选择没
 * 生效」）。认领目录在 `<实例 official-chat 根>/seq-claims/<provider>/<会话短码>/`。
 *
 * ## 路径：跟着后端解析，**不在代码里展开真实用户 home**
 *
 * 协议 §5 写的是 `<实例 official-chat 根>/snapshots/<provider>/<snapshotId>.json`，
 * 并且要求「所有路径跟随后端路径解析（按壳模式、内核族、实例、站点账户隔离）」。
 *
 * 这个「后端」就是**壳给内核进程注入的 `DSH_HOME`**：壳的适配器把每个实例的
 * 内核进程指向 `kernels/<family>/instances/<id>/home/`
 * （`src-tauri/src/kernel/kernel_adapter.rs:21-23` 与
 * `src-tauri/src/kernel/kernel_adapter.rs:472` 的
 * `.env("DSH_HOME", &dsh_home)`；`dsh_home` 来自
 * `paths::instance_dsh_home(family, id)`）。也就是说**内核进程里的
 * `DSH_HOME` 就是这个实例的 home**，天然按实例隔离、dev/release 分家。
 *
 * 本文件因此只做一件事：`<DSH_HOME>/official-chat/snapshots/<provider>/`。
 * 它**不读** `~`、**不拼** `C:\Users\<name>\...`、**不猜**任何用户目录——
 * 根目录整个由环境给出。后端没给（`DSH_HOME` 缺失）时**如实报不可用**，
 * 绝不静默落到某个默认位置（那正是 2026-10-20 13:48 那次事故的形状：
 * 「压根不给目录」= 浏览器用默认目录 = 用户的真实安装目录）。
 *
 * 覆盖项 `DSH_XLINK_OFFICIAL_CHAT_ROOT` 供壳（或测试）显式指定根目录；
 * 没有它就用 `DSH_HOME/official-chat`。
 *
 * ## 协议口径只有一份（在 `host/protocol.js`）
 *
 * 快照格式、规范化、摘要、句柄形态、状态机**全部**由 `host/protocol.js` 定义
 * （它是协议 §6「同一组夹具，JS 与 Rust 各跑一遍」里 JS 那一半的实现）。
 * 本文件**不再自带一份**口径——两份算法同时躺在一个文件里，谁也不知道哪份
 * 是活的，那正是 P1 要消灭的形状。
 *
 * 本文件只保留两样**它自己的**东西：
 * · **路径安全闸**（`SNAPSHOT_ID_RE` / `PROVIDER_RE`）：`snapshotId` 直接进
 *   文件路径，所以只认小写字母数字与那一个分隔符 `~`——路径分隔符、`.`、
 *   空格、相对路径片段一律不认。
 * · **落盘与读回**：不可变写（同 id 不重写）、摘要校验、版本校验。
 *
 * ## 为什么这里不打业务日志
 *
 * 插件里的 `ctx.logger.*` 在这个内核里一行都不进 stdout（memory 那条教训，
 * 见 `plugins/memory/host/index.js` 的注释），所以「快照到底有没有落盘」这件事
 * **不能靠插件自报**——由客户端半边的探针 + 壳侧的实际文件回答。插件只记
 * 失败原因（那是给排查的人看的），不记正文（正文是用户私有内容）。
 *
 * @module xlink-official-chat
 */

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

import {
  SCHEMA_VERSION,
  bodyBytes,
  contentDigest,
  joinMessages,
  normalizeMessages,
} from './protocol.js';
import { createCatalogCache, createCommands, createFixtureSource } from './commands.js';
import { RPC_CHANNEL, createRpcHandler } from './rpc.js';
import { createSeqAllocator } from './seq-store.js';

/** 内核按这个名字认这个插件（也是 settingsNs 的来源，也是服务的名字）。 */
const name = 'xlink-official-chat';

/**
 * 快照格式版本（协议 §4：首版 = 1）。
 *
 * **口径在 `host/protocol.js`**（`SCHEMA_VERSION`）：这个常量必须与协议夹具、
 * Rust 那半三处一致（协议 §6），所以这里**不再写死一个数字**。
 */
export const SNAPSHOT_SCHEMA_VERSION = SCHEMA_VERSION;

/**
 * `ref` / `snapshotId` 的长度上限（协议 S-1：整串 ≤ 24 个可见字符）。
 *
 * 上限不是审美问题：内核自带 source 的 `clipboardText: (ref) => ref`
 * （`ui-reference/client.js@0.2.1-rc.2:363`，SHA-256
 * `3D179111C51381733C7D0EC13C60EE4D9444C1DFB474EF28A0127654B8BA28A3`）
 * 加上 `ui-conversation/lib/client.js@0.2.1-rc.2:4356` 的
 * `reference.length !== reference.clipboardText.length` ⇒
 * **ref 的长度就是用户编辑器里那枚 chip 的宽度**。
 */
export const MAX_SNAPSHOT_ID_LEN = 24;

/**
 * 快照 id 的形状（协议 S-1）：`<providerShort>~<sessionShort>~<seq>`。
 *
 * · `providerShort`：provider 稳定短码，≤ 4 字符；
 * · `sessionShort`：官网会话 id 的稳定短码，≤ 12 字符；
 * · `seq`：该会话在**本实例内**的 pick 序号，4 位。
 *
 * **形状是安全边界，不只是格式**：`snapshotId` 会直接进文件路径，
 * 所以这里只认小写字母数字与那一个分隔符 `~`——路径分隔符、`.`、
 * 空格、相对路径片段一律不认（`..`、`.`、`/`、`\` 都匹配不上）。
 *
 * **与 `host/protocol.js` 的 `parseSnapshotId` 的分工**：那边判「形态对不对」
 * （协议口径），这里判「能不能安全进路径」（路径安全）。两道闸都要过。
 */
const SNAPSHOT_ID_RE = /^[a-z0-9]{1,4}~[a-z0-9]{1,12}~\d{4}$/;

/** provider 目录名（`<root>/snapshots/<provider>/`）。同样只认安全字符。 */
const PROVIDER_RE = /^[a-z0-9][a-z0-9-]{0,15}$/;

/** 单条正文上限（协议 §4：最大正文字节 256 KB，超过 → `body_too_large`）。 */
export const MAX_BODY_BYTES = 256 * 1024;

/**
 * provider 稳定 id。
 *
 * **与 `host/protocol.js` 的 `PROVIDER_SHORT` 同源**：句柄 `ref` 的第一段就是
 * provider 短码。
 *
 * **v0.2 起句柄由 host 分配**（协议 §2 S-1：id 分配权归 host，客户端不得
 * 自造）——所以这里**不再需要**与客户端半边对源：客户端拿到的是 host 已经
 * 分配好的 `snapshotId`，它原样当 `ref` 用。
 *
 * P1 只有一个来源（G1 固定夹具），短码就是 `ds`；P4 接真实站点时**两边一起改**。
 */
const PROVIDER_ID = 'ds';

/**
 * G1 固定夹具（**服务端这一份**，协议 §5：客户端传的会话 id 只是一致性校验，
 * 不构成授权——所以正文只能由服务端取）。
 *
 * **与客户端半边那份逐字相同**：客户端那份只用来渲染 `@` 菜单的标题，服务端
 * 这一份才是正文的来源；两份不一致时模型输入会与菜单显示的东西对不上。
 */
const G1_FIXTURES = [
  {
    sessionId: 'g1-alpha',
    title: 'G1 固定对话 Alpha',
    body: '【G1 固定正文 Alpha】第一行固定正文。\n第二行固定正文。',
  },
  {
    sessionId: 'g1-beta',
    title: 'G1 固定对话 Beta',
    body: '【G1 固定正文 Beta】只有一行。',
  },
];

/**
 * 这个 id 是不是合法的快照 id（**路径安全**那一道闸）。
 *
 * @param {unknown} id - 待校验的 id。
 * @returns {boolean} 合法即 true。
 */
export function isValidSnapshotId(id) {
  const text = String(id ?? '');
  return text.length <= MAX_SNAPSHOT_ID_LEN && SNAPSHOT_ID_RE.test(text);
}

/**
 * 快照 id 的 provider 段（id 的第一段）。
 *
 * 读快照**只按固定 id**：路径里的 provider 段从 id 里取，调用方不能另外指定
 * 目录（协议 §5「桥不接受任意路径」的同一条纪律）。
 *
 * @param {string} id - 快照 id。
 * @returns {string} provider 短码。
 */
export function providerOfSnapshotId(id) {
  return String(id ?? '').split('~')[0] ?? '';
}

/**
 * 快照的**模型可见正文**：各条消息的文本按顺序用空行分隔。
 *
 * **口径来自 `host/protocol.js`**（`normalizeMessages` + `joinMessages`）：
 * 消息先规范化（角色名小写、文本取串），再按 `MESSAGE_SEPARATOR` 拼。
 * 摘要算在**拼接结果**上（`contentDigest`），于是「盘上的正文」与
 * 「摘要」与「模型输入的正文」三者逐字节相同（S-4）。
 *
 * @param {{messages?: {text?: string}[]}} snapshot - 快照。
 * @returns {string} 模型可见正文。
 */
export function bodyOf(snapshot) {
  return joinMessages(normalizeMessages(snapshot?.messages));
}

/**
 * 快照存储的根目录。
 *
 * 优先用显式覆盖项 `DSH_XLINK_OFFICIAL_CHAT_ROOT`；否则用
 * `<DSH_HOME>/official-chat`。**两者都没有 → 存储不可用**（不猜、不兜底）。
 *
 * @param {Record<string, string | undefined>} env - 环境变量表。
 * @returns {string | null} 根目录；不可用时 null。
 */
export function resolveSnapshotRoot(env = {}) {
  const override = String(env.DSH_XLINK_OFFICIAL_CHAT_ROOT ?? '').trim();
  if (override !== '') return override;
  const home = String(env.DSH_HOME ?? '').trim();
  if (home === '') return null;
  return join(home, 'official-chat');
}

/**
 * 造一个错误，带**稳定错误码**（协议 §3：13 个码，码名不随文案变）。
 *
 * @param {string} code - 错误码。
 * @param {string} message - 人看的说明。
 * @returns {Error & {code: string}} 错误。
 */
function snapshotError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

/**
 * 快照存储。
 *
 * @param {Record<string, string | undefined>} [env] - 环境变量表（默认 `process.env`）。
 * @returns {{root: string|null, available: boolean, save: Function, load: Function, listSessionSeqs: Function}} 存储。
 */
export function createSnapshotStore(env = process.env) {
  const root = resolveSnapshotRoot(env);

  const requireRoot = () => {
    if (root === null) {
      throw snapshotError(
        'snapshot_store_unavailable',
        '快照存储不可用：内核没有给出实例 home（DSH_HOME 缺失）',
      );
    }
    return root;
  };

  /**
   * 落盘一份快照。**同一 id 不重写**（协议 §6：快照文件不可变）。
   *
   * @param {object} snapshot - 快照。
   * @returns {{snapshotId: string, contentDigest: string, path: string}} 落盘结果。
   */
  const save = (snapshot) => {
    const base = requireRoot();
    const snapshotId = String(snapshot?.snapshotId ?? '');
    if (!isValidSnapshotId(snapshotId)) {
      throw snapshotError('snapshot_invalid', `快照 id 不合法：${snapshotId}`);
    }
    const providerId = String(snapshot?.providerId ?? providerOfSnapshotId(snapshotId));
    if (!PROVIDER_RE.test(providerId)) {
      throw snapshotError('snapshot_invalid', `provider 不合法：${providerId}`);
    }
    const messages = normalizeMessages(snapshot?.messages);
    const body = joinMessages(messages);
    const bytes = bodyBytes(messages);
    if (bytes > MAX_BODY_BYTES) {
      throw snapshotError('body_too_large', `正文超限：${bytes} 字节`);
    }
    const digest = contentDigest(body);
    const claimed = String(snapshot?.contentDigest ?? '');
    if (claimed !== '' && claimed !== digest) {
      // 调用方报的摘要与实际正文对不上 → 快照在**传进来之前**就坏了。
      throw snapshotError('snapshot_invalid', '快照摘要与正文不一致');
    }
    const stored = {
      schemaVersion: SNAPSHOT_SCHEMA_VERSION,
      snapshotId,
      providerId,
      accountScope: String(snapshot?.accountScope ?? ''),
      sessionId: String(snapshot?.sessionId ?? ''),
      branch: String(snapshot?.branch ?? ''),
      messages,
      contentDigest: digest,
      // **协议 v0.2 裁定：`captureResult` 的形态 = 对象**（三字段齐全）。
      // `ok` 兼容字符串 `'ok'`（等价 `{status:'ok',omitted:[],unsupported:[]}`），
      // `partial` / `failed` **必须用对象**——用字符串会丢「省略了什么」，
      // 违反「不静默裁掉」纪律。
      //
      // 这里**原样透传**：命令层已经把它规范成对象了（`host/protocol.js` 的
      // `buildSnapshot` 逐字段落三字段），存储层再转一次就成第二份口径。
      captureResult: snapshot?.captureResult ?? { status: 'ok', omitted: [], unsupported: [] },
      capturedAt: String(snapshot?.capturedAt ?? ''),
      sourceUpdatedAt: String(snapshot?.sourceUpdatedAt ?? ''),
      sourceFingerprint: String(snapshot?.sourceFingerprint ?? ''),
    };
    const dir = join(base, 'snapshots', providerId);
    mkdirSync(dir, { recursive: true, mode: 0o700 });
    const path = join(dir, `${snapshotId}.json`);
    try {
      // `wx` = 文件已存在就报错。**不覆盖**是协议 §6 的硬要求。
      writeFileSync(path, `${JSON.stringify(stored, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
    } catch (error) {
      if (error?.code === 'EEXIST') {
        throw snapshotError('snapshot_exists', `快照已存在（同 id 不重写）：${snapshotId}`);
      }
      throw snapshotError('snapshot_invalid', `快照落盘失败：${String(error?.message ?? error)}`);
    }
    return { snapshotId, contentDigest: digest, path };
  };

  /**
   * 读回一份快照（**只按固定 id**）。
   *
   * @param {string} snapshotId - 快照 id。
   * @returns {{snapshot: object, body: string}} 快照与它的模型可见正文。
   */
  const load = (snapshotId) => {
    const base = requireRoot();
    const id = String(snapshotId ?? '');
    if (!isValidSnapshotId(id)) {
      throw snapshotError('snapshot_invalid', `快照 id 不合法：${id}`);
    }
    const providerId = providerOfSnapshotId(id);
    const path = join(base, 'snapshots', providerId, `${id}.json`);
    let raw;
    try {
      raw = readFileSync(path, 'utf8');
    } catch (error) {
      if (error?.code === 'ENOENT') {
        // 找不到 = 缺失 = 拒（协议 S-6：快照被清理后拒，**不静默重读**）。
        throw snapshotError('snapshot_invalid', `快照不存在：${id}`);
      }
      throw snapshotError('snapshot_invalid', `快照读不出来：${String(error?.message ?? error)}`);
    }
    let parsed;
    try {
      parsed = JSON.parse(raw);
    } catch (error) {
      throw snapshotError('snapshot_invalid', `快照不是合法 JSON：${id}`);
    }
    if (Number(parsed?.schemaVersion) !== SNAPSHOT_SCHEMA_VERSION) {
      throw snapshotError('schema_changed', `快照版本不支持：${String(parsed?.schemaVersion)}`);
    }
    const body = bodyOf(parsed);
    if (contentDigest(body) !== String(parsed?.contentDigest ?? '')) {
      // 摘要不匹配 = 快照损坏 → 拒，**不是重读**（协议 §1.3）。
      throw snapshotError('snapshot_invalid', `快照摘要不匹配（已损坏）：${id}`);
    }
    return { snapshot: parsed, body };
  };

  /**
   * 每个会话短码已经用掉的最大序号（供客户端把本地序号接上去）。
   *
   * @param {string} [providerId] - provider 短码。
   * @returns {Record<string, number>} 会话短码 → 已用的最大序号。
   */
  const listSessionSeqs = (providerId = '') => {
    const out = {};
    if (root === null) return out;
    const provider = String(providerId ?? '');
    if (!PROVIDER_RE.test(provider)) return out;
    const dir = join(root, 'snapshots', provider);
    let names;
    try {
      names = readdirSync(dir);
    } catch {
      return out;
    }
    for (const fileName of names) {
      if (!fileName.endsWith('.json')) continue;
      const id = fileName.slice(0, -'.json'.length);
      const parts = id.split('~');
      if (parts.length !== 3) continue;
      const seq = Number(parts[2]);
      if (!Number.isFinite(seq)) continue;
      const key = `${parts[0]}~${parts[1]}`;
      if (seq > (out[key] ?? 0)) out[key] = seq;
    }
    return out;
  };

  return { root, available: root !== null, save, load, listSessionSeqs };
}

/**
 * 这个插件的 host 半边消费**两个**内核服务：`connection` 与 `webServer`。
 *
 * ## 为什么 `connection` 必须声明
 *
 * 内核的 `connection` 服务是 RPC 通道的宿主，声明之后 cordis 才会在它可用时
 * 激活这条 fiber（不声明就只能在 apply 里「碰运气」地读一次，读不到就是
 * 永久性的哑火）。
 *
 * **它不是投机声明**：这个服务由内核自己的 `dsh-client-connection` 提供
 * （`dsh-client-connection/lib/index.js@0.2.1-rc.2:710`），浏览器侧同款在
 * `client.browser.js@0.2.1-rc.2:1497`——**没有它页面连内核都通不了**。
 *
 * ## `webServer`：**声明了也救不了 `rpc.handle`（2026-10-20 真机实测 p1-r3）**
 *
 * 内核的 RPC 注册走 `connection.rpc.handle(channel, handler)`
 * （`dsh-client-connection/lib/index.js@0.2.1-rc.2:727`：
 * `handle: (channel, handler) => this.register(owner, channel, handler)`），
 * 而 `register` 的最后一步是
 * `owner.effect(() => owner.webServer.register(route), …)`
 * （同文件 `dsh-client-connection/lib/index.js@0.2.1-rc.2:871`）——**属性访问**，
 * cordis 查不到就抛 `cannot get property "webServer" without inject`。
 *
 * **实测（探针 p1-r3，隔离 home）**：把 `webServer` 加进本插件的 `inject`
 * **之后仍然抛同一个错**，栈是
 * `dsh-client-connection/lib/index.js@0.2.1-rc.2:727` →
 * `dsh-client-connection/lib/index.js@0.2.1-rc.2:871` → `owner.webServer`。
 * ⇒ **在插件侧声明 `webServer` 不足以让 `rpc.handle` 成立**（此前「owner 是读取方
 * 的 ctx、声明就能解决」的判断是错的，已按实测改正）。
 *
 * **为什么**（cordis 机制）：
 * · `get rpc()` 里写的是 `const owner = this.ctx`
 *   （`dsh-client-connection/lib/index.js@0.2.1-rc.2:725`）；
 * · 属性查找取的是 `(ctx[symbols.shadow] ?? ctx).fiber`
 *   （`@deepseek-ai/cordis/lib/index.js@0.2.1-rc.2:683`），而 `symbols.shadow`
 *   由 `createShadow` 设成**服务自己的 ctx**
 *   （`@deepseek-ai/cordis/lib/index.js@0.2.1-rc.2:111-116`）；
 * ⇒ **`owner.webServer` 的查找发生在「内核 connection 插件」那个 fiber scope 里**，
 *   不是我们这个 scope。而内核 connection 插件**从不 inject `webServer`**
 *   （它一律用安全读 `this.owner.get("webServer")`，
 *   `dsh-client-connection/lib/index.js@0.2.1-rc.2:745` 与
 *   `dsh-client-connection/lib/index.js@0.2.1-rc.2:778`）⇒ 查不到 ⇒ 抛。
 *
 * ## 那为什么还留着 `webServer`
 *
 * 因为**它在我们自己的 scope 里确实可用**（实测：声明它之后 fiber 照常激活、
 * apply 照常执行 ⇒ 该服务在我们这个 scope 里可解析）。而这正是**方案②**
 * （插件自己 `ctx.webServer.register(route)` + 自实现 client-request 信封）
 * 的前置条件。**它不是 `rpc.handle` 的修复**，别把它当成修复。
 *
 * **对照：内核自己的 API Gateway 为什么没事**——它用的是
 * `connection.rpc.intercept('/api', …)`
 * （`dsh-api-gateway/lib/index.js@0.2.1-rc.2:773`），
 * 而 `registerInterceptor`
 * （`dsh-client-connection/lib/index.js@0.2.1-rc.2:873-885`）
 * **根本不碰 `owner.webServer`**（它只往 `interceptors` map 里塞一条），
 * `/api` 的真实路由由 API Gateway 自己注册。**`handle` 与 `intercept` 不是
 * 一回事**：只有 `handle` 才会去动 webServer。
 *
 * @type {string[]}
 */
const inject = ['connection', 'webServer'];

/**
 * 本实例的「账户/实例范围」（**服务端推导**，协议 §5：客户端传入的会话标识
 * 只用于一致性校验，**不构成授权**）。
 *
 * **只用摘要、不用路径**：实例 home 是用户目录，落盘物里**不许**出现它
 * （仓库有专门判据）；而摘要足够区分 dev/release 与不同实例。P1 的来源是
 * 固定夹具，没有真实站点账户——真实账户这一维属 P4。
 *
 * @param {Record<string, string | undefined>} [env] - 环境变量表。
 * @returns {string} 范围；**空串 = 不可得**（此时按 `account_changed` 拒）。
 */
export function instanceScope(env = process.env) {
  const home = String(env.DSH_HOME ?? '').trim();
  if (home === '') return '';
  const digest = createHash('sha256').update(home, 'utf8').digest('hex').slice(0, 32);
  return `dsh/${digest}/${PROVIDER_ID}`;
}

/**
 * 插件入口。内核 Loader import 这个文件时调用它。
 *
 * **这里不抛**：注册不上就如实记一行并继续——插件的 @ 来源与 G1 结论不受
 * 影响，只是引用落不了盘（客户端侧会拒，不会假装就绪）。
 *
 * @param {import('@deepseek-ai/cordis').Context} ctx - Host 根上下文。
 */
function apply(ctx) {
  const store = createSnapshotStore();
  const root = store.root;

  // 目录缓存是**独立于快照**的缓存物（协议 §5：两个路径家族分开）。
  // 存储根不可用时用**空缓存**：目录照常从取数源取（慢一点），但**不写盘**
  // ——绝不因为缓存没地方放就把正文写去别处。
  const catalogCache =
    root === null
      ? { read: () => null, write: () => {} }
      : createCatalogCache({ rootDir: join(root, 'catalog-cache') });

  const commands = createCommands({
    store,
    catalogCache,
    captureSource: createFixtureSource(G1_FIXTURES),
    // **id 分配权归 host**（协议 v0.2 S-1）：序号由分配器**持久化认领**，
    // 客户端不得自造——跨进程单调（dev 壳与 release 壳同开也不撞）。
    seqAllocator: createSeqAllocator({ root, providerId: PROVIDER_ID }),
    providerId: PROVIDER_ID,
    // **账户范围（服务端推导）**：协议 §5「客户端传入的会话标识用于一致性校验，
    // 不构成授权」+ §8-6「指纹比对的校验点在 host 命令层；当前指纹不可得 →
    // 拒（`account_changed`），不得放行」。
    //
    // **必须给**：空范围 ⇒ 指纹不可得 ⇒ 命令层按 `account_changed` **一律拒**
    // （capture 与 preview 都拒）——那时整个引用功能是死的，而且症状是
    // 「读得到快照却发不出去」，很难一眼看出是范围没给。
    accountScope: instanceScope(),
  });

  // **注册 RPC 端点**：内核 `connection` 服务的 `rpc.handle`。
  //
  // **为什么挂在 `ctx.effect` 上**：`ctx.connection` 是**反应式**的服务属性
  // （内核自己也这么写：`ctx.effect(() => ctx.connection.fetch.register({...}))`，
  // 见 `dsh-api-gateway/lib/index.js@0.2.1-rc.2:2482`）。挂在 effect 上意味着
  //   · 服务**还没就绪**时不注册（`inject: ['connection']` 让 cordis 等它）；
  //   · 插件被卸载 / 服务被换掉时，路由**跟着收掉**（`rpc.handle` 自己返回
  //     一个 effect 清理函数，见 `dsh-client-connection/lib/index.js@0.2.1-rc.2:855-871`）。
  //
  // **不用 `ctx.set` 挂服务**：cordis 要求该属性先被 `provide` 过，否则直接抛
  // `cannot set property "..." without provide`（`cordis/lib/index.js@0.2.1-rc.2:782-789`）；
  // 而浏览器页面里的 client 半边与内核 Node 进程里的 host 半边**不在同一棵
  // cordis 树**——`ctx.get('xlink-official-chat')` 在页面树里永远解析不到。
  // ⇒ RPC 是**唯一可行**的通道（协议 v0.2 §5 已把这条写死）。
  ctx.effect(() => {
    const connection = ctx.connection;
    if (connection === undefined || connection === null || typeof connection.rpc?.handle !== 'function') {
      ctx.logger?.warn?.(
        `${name}: 内核没有提供 connection 服务，官网引用的 RPC 端点注册不上` +
          '（客户端半边会如实报「host 不可达」，引用不会落盘）',
      );
      return undefined;
    }
    return connection.rpc.handle(RPC_CHANNEL, createRpcHandler({ commands, logger: ctx.logger }));
  }, `${name}: ${RPC_CHANNEL} RPC 通道`);
}

export { apply, inject, name };
export { contentDigest } from './protocol.js';
