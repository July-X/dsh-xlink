// P4 判据**第 5 条**（能力面）的自证。
//
// ## 5d（协议 v0.23 §4.5 · 冷档案）的自证
//
// · **反向自证**：每条断点「改坏一处」⇒ **对应那条断言**必须红（不许靠别的断言顶红）。
// · **集成自证**：在**影子目录**里放一份**改坏的**冷档案模块 ⇒ 真实抽取路径
//   （`readColdProfileReading` → `judgeColdProfile`）必须抓到 ⇒ ⇒ 工作树一个字不动。
//
// @module scripts/check-official-chat-p4-surface-test

import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import {
  PASS,
  buildScenarios,
  judgeClientSite,
  judgeColdProfile,
  judgeSurface,
  readClientSiteReading,
  readColdProfileReading,
  readReading,
} from './check-official-chat-p4-surface.mjs';
import {
  BODY_CAPTURE_REL,
  CLIENT_REL,
  COLD_PROFILE_REL,
  repoRoot,
} from './lib/p4-source-shape.mjs';

const REPO = repoRoot(import.meta.url);

/**
 * 造一个**影子根**：冷档案模块 ＋ 正文链模块 ＋ 目录链模块（**只读**，不动工作树）。
 *
 * @param {Record<string, string>} files - 相对影子根的路径 → 内容。
 * @returns {string} 影子根绝对路径。
 */
function makeShadow(files) {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-p4-5d-shadow-'));
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, '..'), { recursive: true });
    writeFileSync(abs, content, 'utf8');
  }
  return dir;
}

test('P4-5：场景表逐条自证（常量脚本 ＋ 命令白名单 ＋ 只碰官方对话 ＋ 不写站点数据）', () => {
  for (const sc of buildScenarios()) {
    const { ok, checks } = judgeSurface(sc.reading);
    const behaved = sc.expect === PASS ? ok : !ok;
    assert.equal(
      behaved,
      true,
      `场景 ${sc.id} 的判定与期望不符：${checks
        .map((c) => `${c.name}=${c.status}（${c.detail}）`)
        .join('；')}`,
    );
  }
});

test('P4-5：真实仓库读数全过', () => {
  const { ok, checks } = judgeSurface(readReading(REPO));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});

test('P4-5d：真实仓库读数全过（冷档案的形态）', () => {
  const { ok, checks } = judgeColdProfile(readColdProfileReading(REPO));
  const bad = checks.filter((c) => c.status !== PASS);
  assert.equal(ok, true, bad.map((c) => `${c.name}：${c.detail}`).join('；'));
});

test('P4-5d：反向自证 · 逐条「改坏一处」⇒ **对应那条**必须红', () => {
  const good = readColdProfileReading(REPO);
  const cases = [
    {
      what: '白名单里混进 `Default/IndexedDB`（那是「冷」的定义）',
      expect: '没有 `IndexedDB`',
      mutate: (r) => ({ ...r, whitelistDirs: [...r.whitelistDirs, 'Default/IndexedDB'] }),
    },
    {
      what: '白名单里少了 `Default/Local Storage`（登录态没了）',
      expect: '带上了 `Local Storage`',
      mutate: (r) => ({
        ...r,
        whitelistDirs: r.whitelistDirs.filter((p) => !p.includes('Local Storage')),
      }),
    },
    {
      what: '目录那条链也改用冷档案了',
      expect: '只服务「正文那条链」',
      mutate: (r) => ({ ...r, coldInStructure: true }),
    },
    {
      what: '正文那条链不用冷档案了（修复静默失效）',
      expect: '只服务「正文那条链」',
      mutate: (r) => ({ ...r, coldInBody: false }),
    },
  ];
  for (const c of cases) {
    const { ok, checks } = judgeColdProfile(c.mutate(good));
    assert.equal(ok, false, `改坏「${c.what}」后整条必须判红`);
    const hit = checks.find((x) => x.status === 'fail' && x.name.includes(c.expect));
    assert.ok(
      hit,
      `改坏「${c.what}」后必须由含「${c.expect}」的断言报红（实际：${checks
        .map((x) => `${x.name}=${x.status}`)
        .join('；')}）`,
    );
  }
});

test('P4-5d：集成自证 · 影子目录里放一份**加进 IndexedDB** 的冷档案 ⇒ 5d 必须红', () => {
  const cold = readFileSync(join(REPO, COLD_PROFILE_REL), 'utf8');
  // 把 `Default/IndexedDB` 塞进白名单（**这正是「冷被稀释」的那一刀**）
  const patched = cold.replace(
    'const WHITELIST_DIRS: &[&str] = &["Default/Local Storage", "Default/Network"];',
    'const WHITELIST_DIRS: &[&str] = &["Default/Local Storage", "Default/Network", "Default/IndexedDB"];',
  );
  assert.notEqual(patched, cold, '改坏没生效 ⇒ 冷档案模块的形态变了（请同步本测试）');

  const dir = makeShadow({
    [COLD_PROFILE_REL]: patched,
    [BODY_CAPTURE_REL]: readFileSync(join(REPO, BODY_CAPTURE_REL), 'utf8'),
    'src-tauri/src/official_chat/session_structure.rs': readFileSync(
      join(REPO, 'src-tauri/src/official_chat/session_structure.rs'),
      'utf8',
    ),
  });
  try {
    const { ok, checks } = judgeColdProfile(readColdProfileReading(dir));
    assert.equal(ok, false, '白名单里进了 IndexedDB ⇒ 5d 必须判红（不许假绿）');
    const hit = checks.find((x) => x.status === 'fail' && x.name.includes('没有 `IndexedDB`'));
    assert.ok(hit, `必须由「没有 IndexedDB」那条报红（实际：${checks.map((x) => x.name).join('；')}）`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** 客户端那半边的源码（5e 的被测对象）。 */
const CLIENT_SRC = readFileSync(join(REPO, CLIENT_REL), 'utf8');
/** 壳侧那三个站点常量所在文件。 */
const STRUCT_REL = 'src-tauri/src/official_chat/session_structure.rs';

/**
 * 造一份**影子根**（客户端那半边 ＋ 壳侧常量那个文件）。
 *
 * @param {string} clientCode - 客户端源码。
 * @returns {string} 影子根绝对路径。
 */
function makeClientShadow(clientCode) {
  return makeShadow({
    [CLIENT_REL]: clientCode,
    [STRUCT_REL]: readFileSync(join(REPO, STRUCT_REL), 'utf8'),
  });
}

test('P4-5e：正向 · 载荷里 `site` 是**可选键**时 ⇒ 5e 全绿（先证明这条不是恒红）', () => {
  // 把目录请求载荷改成**正确写法**（`...sitePayload(currentSite())`）⇒ 键整个不出现
  const patched = CLIENT_SRC.replace(
    'scheduleCatalog(host, { query, refresh, site: currentSite() })',
    'scheduleCatalog(host, { query, refresh, ...sitePayload(currentSite()) })',
  );
  assert.notEqual(patched, CLIENT_SRC, '没找到那处载荷组装 ⇒ 客户端写法变了（请同步本测试）');
  const dir = makeClientShadow(patched);
  try {
    const { ok, checks } = judgeClientSite(readClientSiteReading(dir));
    assert.equal(ok, true, `正确写法下 5e 必须全绿：${checks.map((c) => `${c.name}：${c.detail}`).join('；')}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('P4-5e：负向自证 · 客户端那三处**改坏一处** ⇒ 对应那条必须红', () => {
  const cases = [
    {
      // 工作树现状就是**字面 `site:` 键** ⇒ 没选站时会多一个 `site: ''`（键没整个不出现）
      // ⇒ 派单口径下这就是「坏」的那一档。
      // ⚠️ 这一格是「现状即坏」：正向用例已证明把它改成 `...sitePayload(currentSite())` 后全绿
      // ⇒ 所以 5e-b 不是恒红，它红的是**这一处具体写法**。
      what: '目录请求载荷是字面 `site:` 键（没选站时会多一个 `site: \'\'`）',
      expect: '可选键',
      patch: (src) => src,
      sameAsSource: true,
    },
    {
      what: '取正文那次改用**当前站**（不是这条引用当初那个站）',
      expect: '当初那个站',
      patch: (src) => src.replace('sitePayload(siteOfRef(snapshotId))', 'sitePayload(currentSite())'),
    },
    {
      what: '客户端站表里的短码与壳侧常量不一致',
      expect: '双向对账',
      patch: (src) => src.replace("code: 'minimax'", "code: 'minimax2'"),
    },
  ];
  for (const c of cases) {
    const patched = c.patch(CLIENT_SRC);
    if (!c.sameAsSource) {
      assert.notEqual(patched, CLIENT_SRC, `改坏「${c.what}」没生效 ⇒ 客户端写法变了（请同步本测试）`);
    }
    const dir = makeClientShadow(patched);
    try {
      const { ok, checks } = judgeClientSite(readClientSiteReading(dir));
      assert.equal(ok, false, `改坏「${c.what}」后整条必须判红`);
      const hit = checks.find((x) => x.status === 'fail' && x.name.includes(c.expect));
      assert.ok(
        hit,
        `改坏「${c.what}」后必须由含「${c.expect}」的断言报红（实际：${checks
          .map((x) => `${x.name}=${x.status}`)
          .join('；')}）`,
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
});
