// 官方对话 · 「目录命令实机一次」的取证脚本（**只读**）。
//
// ## 它要回答什么
//
// 前面的取数面取证（`official_chat_minimax_probe.rs` / `official_chat_chatgpt_probe.rs`）
// 走的是**探针自己的** webview ⇒ ⇒ 那是「**副本跑**」（源 profile 只读拷一份给浏览器）。
// PM 追加的这一段要的是另一件事：**插件 host 那条 `catalog` 命令**在**实机**上跑一次
// —— 也就是「host → site-source.js → 桥上 `/v1/official-chat/session-structure`
// → 壳侧那扇**活着的**取数窗」这一整条链。
//
// ## 三栏读数（**不许合并**）
//
// · **实机栏**（`live_bridge`）：桥**活着**时才有 —— 桥由**壳为内核**起
//   （`src-tauri/src/plugins/builtin/bridge.rs`），内核不跑就没有桥 ⇒ ⇒ 这一栏
//   现在多半是「桥不可达」。**桥不可达 ≠ 用户跑不到**，也**≠**用户跑得到。
// · **命令层栏**（`host_command`）：`host/commands.js` 的**真**命令层跑一次
//   `catalog` ⇒ 落「回没回成功／回包里 items 几条／`source.id` 是什么／
//   失败时的 `code`」。
// · **失败路径栏**（`host_command_failure`）：同一套命令层，但取数源回**失败**
//   ⇒ ⇒ 落 host 把壳侧的错误码**归一**成协议那 14 个码里的哪一个。
//
// ## 纪律
//
// · **只读**：不发任何站点请求（取数由壳侧那扇窗做），不新建会话、不发消息；
// · **不落凭据**：桥的 token 一个字都不进读数（只落 `host:port` 与「能不能连上」）；
// · **不落标题/正文**：命令层回包里的条目只落**条数**、**字段名**、**标题长度**；
// · **产品路径只能当参数传**（`--instances-root`）：源码里不许出现产品数据目录的字面量。
//
// ## 用法
//
// ```sh
// node src-tauri/examples/official_chat_catalog_command_probe.mjs \
//   --instances-root "<实例根目录>" \
//   --site minimax \
//   --out "<读数文件路径>"
// ```
//
// 桥活着时（有人在内核起来的壳里开过会话）这条会直接落「实机」读数；
// 桥不活着时它落「桥不可达 ＋ 哪一代凭据为什么失效」，**照样有读数**。

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { connect } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createCatalogCache, createCommands } from '../../plugins/official-chat/host/commands.js';
import { createSeqAllocator } from '../../plugins/official-chat/host/seq-store.js';
import { createDispatcher } from '../../plugins/official-chat/host/rpc.js';
import { createSiteSource } from '../../plugins/official-chat/host/site-source.js';

const CREDENTIAL_FILE = '.xlink-bridge.json';

/** 取参数值（`--k v` 与 `--k=v` 都认）。 */
function argValue(args, key) {
  const index = args.indexOf(key);
  if (index !== -1 && index + 1 < args.length) return args[index + 1];
  const prefix = `${key}=`;
  const found = args.find((one) => one.startsWith(prefix));
  return found ? found.slice(prefix.length) : undefined;
}

/** 递归找文件名匹配的凭据文件（**只找**这一种文件名）。 */
function findCredentialFiles(root) {
  const out = [];
  const walk = (dir, depth) => {
    // ⚠️ **深度要给够**：物化目录的形状是
    // `<实例>/home/extensions/builtin/<插件>/<版本>/<指纹>/compat-a/.xlink-bridge.json`
    // ⇒ ⇒ 从实例根算起有 **8~9 层** ⇒ ⇒ 给 12 层。
    if (depth > 12) return;
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(full, depth + 1);
      } else if (entry.isFile() && entry.name === CREDENTIAL_FILE) {
        out.push(full);
      }
    }
  };
  walk(root, 0);
  return out;
}

/** 进程还活着吗（`kill(pid, 0)` 只探活，不发信号）。 */
function pidAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** 回环端口能连上吗（**连上就断开** ⇒ 不惊动壳）。 */
function probePort(host, port) {
  return new Promise((resolve) => {
    const socket = connect({ host, port });
    const done = (value) => {
      socket.destroy();
      resolve(value);
    };
    socket.setTimeout(1500);
    socket.once('connect', () => done(true));
    socket.once('timeout', () => done(false));
    socket.once('error', () => done(false));
  });
}

/** 造一套内存桩存储（目录命令不写快照 ⇒ 只需要形状）。 */
function makeStore() {
  const written = new Map();
  return {
    save: (snapshot) => {
      const id = String(snapshot?.snapshotId ?? '');
      written.set(id, snapshot);
      return { snapshotId: id, contentDigest: snapshot.contentDigest, path: `mem://${id}` };
    },
    load: (id) => {
      const found = written.get(String(id));
      if (!found) {
        const error = new Error('快照不存在');
        error.code = 'snapshot_invalid';
        throw error;
      }
      return { snapshot: found, body: '' };
    },
    listSessionSeqs: () => ({}),
  };
}

/**
 * 目录响应的**结构重建**（值是占位）。
 *
 * ⚠️ **为什么是重建**：真实响应体**不落盘**（纪律：只落键名／计数／类型）⇒ ⇒
 * 命令层这一栏拿不到真实条目值 ⇒ ⇒ 这里按**实测结构**（顶层键名／数组长度／
 * 条目 6 键）重建一份，**条目值是占位**。
 * ⇒ ⇒ 这一栏能证明的是**命令层的回包形状与 code 路径**，**不是**真实条目内容。
 */
function reconstructedStructure() {
  const itemKeys = ['id', 'title', 'title_type', 'model_type', 'pinned', 'updated_at'];
  const items = [];
  for (let index = 0; index < 6; index += 1) {
    items.push({
      id: String(1790000000000 + index),
      title: `占位标题${index}`,
      title_type: 'USER',
      model_type: 'deepseek-chat',
      pinned: false,
      updated_at: 1792652933.874 - index * 1000,
    });
  }
  return {
    topKeys: ['base_resp', 'has_more', 'next_cursor', 'sessions'],
    code: 0,
    msg: '',
    dataKeys: [],
    sessionsLength: 20,
    firstItemKeys: itemKeys,
    itemTypes: ['string', 'string', 'string', 'string', 'boolean', 'number'],
    hasMore: true,
    items,
  };
}

/** 从命令层回包里只取**形状**（不落标题/正文原文）。 */
function shapeOfPage(page) {
  const items = Array.isArray(page?.items) ? page.items : [];
  return {
    item_count: items.length,
    item_keys: items.length > 0 ? Object.keys(items[0]) : [],
    title_lengths: items.map((item) => String(item?.title ?? '').length),
    has_more: page?.hasMore ?? null,
    cursor: page?.cursor ?? null,
    source: page?.source
      ? {
          kind: page.source.kind,
          id: page.source.id,
          label: page.source.label,
        }
      : null,
  };
}

async function main() {
  const args = process.argv.slice(2);
  const instancesRoot = argValue(args, '--instances-root');
  if (!instancesRoot) {
    console.error('probe: 必须显式给 --instances-root <实例根目录>（产品路径只当参数传，不进源码）');
    process.exit(2);
  }
  const out = argValue(args, '--out');
  if (!out) {
    console.error('probe: 必须显式给 --out <读数文件路径>');
    process.exit(2);
  }
  const site = argValue(args, '--site') ?? 'minimax';

  const report = {
    probe: '官方对话 · 目录命令（host 的 catalog）实机一次',
    site,
    started_at_utc: new Date().toISOString(),
    discipline: [
      '只读：不新建会话、不发消息、不点按钮；取数由壳侧那扇活着的取数窗做',
      '不外接 CDP：只走桥上那条白名单路由（POST /v1/official-chat/session-structure）',
      '桥的 token 一个字都不进读数',
      '回包里只落条数／字段名／标题长度；不落标题原文、不落正文',
    ],
  };

  // ---- 找桥凭据：挑「pid 活着 ＋ 端口连得上」的那一代 ----
  const credentialFiles = existsSync(instancesRoot) ? findCredentialFiles(instancesRoot) : [];
  const candidates = [];
  let liveCredential = null;
  for (const file of credentialFiles) {
    let parsed;
    try {
      parsed = JSON.parse(readFileSync(file, 'utf8'));
    } catch {
      continue;
    }
    const url = String(parsed?.url ?? '');
    const token = String(parsed?.token ?? '');
    const pid = Number(parsed?.pid ?? 0);
    let authority = { host: '', port: 0 };
    try {
      const parsedUrl = new URL(url);
      authority = { host: parsedUrl.hostname, port: Number(parsedUrl.port) };
    } catch {
      /* 记成不可用 */
    }
    const alive = pid > 0 && pidAlive(pid);
    const reachable = alive && authority.port > 0 ? await probePort(authority.host, authority.port) : false;
    candidates.push({
      credential_file: file,
      bridge_host_port: authority.port > 0 ? `${authority.host}:${authority.port}` : null,
      pid,
      pid_alive: alive,
      port_reachable: reachable,
      mtime: statSync(file).mtime.toISOString(),
    });
    if (reachable && liveCredential === null) {
      liveCredential = { file, url, token, pid };
    }
  }
  candidates.sort((left, right) => (left.mtime < right.mtime ? 1 : -1));
  report["bridge"] = {
    credential_file_name: CREDENTIAL_FILE,
    candidates,
    live: liveCredential !== null,
    live_host_port: liveCredential
      ? `${new URL(liveCredential.url).hostname}:${new URL(liveCredential.url).port}`
      : null,
    note: liveCredential
      ? '这一代桥还活着（pid 活着 ＋ 端口连得上）⇒ 实机栏有读数'
      : '**没有活着的桥** ⇒ 桥由壳为内核起、内核没跑就没有 ⇒ 实机栏只能记「桥不可达」（**这不等于用户跑不到**）',
  };

  // ---- 实机栏：走真桥 ----
  const liveStructure = { ok: false, code: 'source_unsupported', detail: '桥不可达（没有活着的桥）' };
  if (liveCredential !== null) {
    const source = createSiteSource({
      env: {
        DSH_XLINK_BUILTIN_BRIDGE_URL: liveCredential.url,
        DSH_XLINK_BUILTIN_BRIDGE_TOKEN: liveCredential.token,
      },
    });
    const result = await source.structure({ site });
    const structure = result?.ok ? result.structure : null;
    liveStructure.ok = result?.ok === true;
    liveStructure.code = result?.ok === true ? null : String(result?.code ?? '');
    liveStructure.detail = result?.ok === true ? null : String(result?.detail ?? '');
    liveStructure.site_envelope = result?.ok === true ? String(result.site ?? '') : null;
    liveStructure.structure_top_keys = structure ? Object.keys(structure) : null;
    liveStructure.sessions_length = structure?.sessionsLength ?? null;
    liveStructure.has_more = structure?.hasMore ?? null;
    liveStructure.item_count = Array.isArray(structure?.items) ? structure.items.length : null;
    liveStructure.first_item_keys =
      Array.isArray(structure?.items) && structure.items.length > 0
        ? Object.keys(structure.items[0])
        : null;
  }
  report["live_bridge"] = liveStructure;

  // ---- 命令层栏：host 的真命令层跑一次 catalog ----
  const root = mkdtempSync(join(tmpdir(), 'official-chat-catalog-command-'));
  const providerId = 'official-chat';
  const accountScope = 'probe/desktop-dev/default/site-A';
  const buildCommands = (siteSource) => {
    // ⚠️ **每栏一个独立的缓存根** ⇒ 否则第二栏会**命中上一栏写的缓存**
    // ⇒ ⇒ 失败栏根本不会去问取数源（那是个假绿）。
    const scopeRoot = mkdtempSync(join(tmpdir(), 'official-chat-catalog-cmd-'));
    return createCommands({
      store: makeStore(),
      catalogCache: createCatalogCache({ rootDir: join(scopeRoot, 'catalog-cache') }),
      captureSource: null,
      siteSource,
      seqAllocator: createSeqAllocator({ root: scopeRoot, providerId }),
      providerId,
      accountScope,
    });
  };

  const runCatalog = async (siteSource) => {
    // ⚠️ `createDispatcher` **直接返回 handler 函数**（不是 `{handler}` 对象）。
    const handler = createDispatcher({ commands: buildCommands(siteSource) });
    const reply = await handler('catalog', { query: '', site, requestId: 'probe-1' });
    return reply;
  };

  // 成功路径：取数源回**重建的结构**（值是占位）。
  //
  // ⚠️ **`env` 里那对假凭据是必须的**：`createSiteSource` 在 **credentials 为 null 时
  // 直接短路**（回 `source_unsupported`）⇒ ⇒ **压根不会用到注入的 transport** ⇒ ⇒
  // 不给这对假凭据，这一栏永远失败（那是个「假红」，不是命令层的错）。
  const fakeEnv = {
    DSH_XLINK_BUILTIN_BRIDGE_URL: 'http://127.0.0.1:1',
    DSH_XLINK_BUILTIN_BRIDGE_TOKEN: 'probe-placeholder-token',
  };
  const successSource = createSiteSource({
    env: fakeEnv,
    transport: async () => ({
      status: 200,
      body: JSON.stringify({ ok: true, structure: reconstructedStructure(), site }),
    }),
  });
  const successReply = await runCatalog(successSource);
  report["host_command"] = {
    ok: successReply?.ok === true,
    code: String(successReply?.error?.code ?? successReply?.code ?? ''),
    detail: String(successReply?.error?.detail ?? successReply?.detail ?? ''),
    page: successReply?.ok === true ? shapeOfPage(successReply.value?.page) : null,
    raw_reply_keys: successReply && typeof successReply === 'object' ? Object.keys(successReply) : null,
    raw_value_keys:
      successReply?.ok === true && successReply.value && typeof successReply.value === 'object'
        ? Object.keys(successReply.value)
        : null,
    note: '**取数源是重建的**（真实响应体不落盘）⇒ 这一栏证明的是**命令层的回包形状与 code 路径**，条目值是占位',
  };

  // 失败路径：取数源回**失败** ⇒ 看 host 把码归一成哪一个。
  const failureSource = createSiteSource({
    env: fakeEnv,
    transport: async () => ({
      status: 200,
      body: JSON.stringify({ ok: false, error: { code: 'catalog_failed', detail: '探针注入的失败' } }),
    }),
  });
  const failureReply = await runCatalog(failureSource);
  report["host_command_failure"] = {
    ok: failureReply?.ok === true,
    code: String(failureReply?.error?.code ?? failureReply?.code ?? ''),
    detail: String(failureReply?.error?.detail ?? failureReply?.detail ?? ''),
    raw_reply_keys: failureReply && typeof failureReply === 'object' ? Object.keys(failureReply) : null,
    note: '取数源回 `{ok:false, error:{code}}` ⇒ 这一栏落的是**host 归一后的 code**（协议 14 码闭集内）',
  };

  report["catalog_cache_dir"] = join(root, 'catalog-cache');
  // ---- 菜单层：这一栏是**源码形态**（实机要在 UI 里点 ⇒ 本单做不到） ----
  report["menu_layer"] = {
    status: '推定（源码形态）',
    source_file: 'plugins/official-chat/client/client.js',
    success_shape: {
      groups: 2,
      detail: '第 1 组「来源站」＝ `siteRows(query)`（`SITE_OPTIONS` 三条：deepseek／minimax／chatgpt；有关键词时按 label/code 过滤）；第 2 组「Web Chat」＝ 会话行，行数 ＝ 回包 `items` 条数（协议上限 6）',
    },
    failure_shape: {
      groups: 1,
      rows: 1,
      detail: '目录失败时那两处失败分支**直接返回一行说明** ⇒ **没有「来源站」分组**；菜单里只剩那一行',
    },
    label_rule: 'host 的 `SITE_LABELS` 只有 `deepseek` 一条 ⇒ 另两站回退成**小写短码**；client 侧用自己那份 `SITE_OPTIONS` 给品牌名 ⇒ **「DeepSeek」这个字样只可能是 deepseek 站**',
    unmeasured: 'UI 实机（菜单里到底几组几行）本单没做 —— 要在壳里点一次 @ 菜单',
  };
  report["finished_at_utc"] = new Date().toISOString();
  report["reading_digest"] = createHash('sha256').update(JSON.stringify(report)).digest('hex');

  const parent = dirname(out);
  if (parent && !existsSync(parent)) mkdirSync(parent, { recursive: true });
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`PROBE_REPORT_BEGIN\n${JSON.stringify(report, null, 2)}\nPROBE_REPORT_END`);
  console.log(`probe: 读数已落 ${out}`);
}

main().catch((error) => {
  console.error(`probe: 失败：${error?.stack ?? error}`);
  process.exit(1);
});
