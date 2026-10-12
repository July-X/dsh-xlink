import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { applyLocale, digest, readManifest } from '../host/i18n.js';

const pluginRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const sha256 = (text) => createHash('sha256').update(text, 'utf8').digest('hex');

/** 上游英文的**真实**裁剪规则（与 `hooks/ponytail-instructions.js` 同款）。
 *
 * 抄它是有意的：中文层最危险的一种坏法是「看着对、实际被过滤掉」——译文里把
 * `| **full** |` 写成 `| **全量** |`，强度过滤就会在 full 档把这一行整行删掉，
 * 而文件读起来完全正常。这里用上游自己的规则跑译文，那一行删不删当场可见。 */
function filterSkillBodyForMode(body, mode) {
  const normalize = (m) => {
    const s = String(m ?? '').trim().toLowerCase();
    return ['lite', 'full', 'ultra', 'off'].includes(s) ? s : null;
  };
  return String(body)
    .replace(/^---[\s\S]*?---\s*/, '')
    .split(/\r?\n/)
    .filter((line) => {
      const table = line.match(/^\|\s*\*\*(.+?)\*\*\s*\|/);
      if (table) {
        const label = normalize(table[1]);
        if (label) return label === mode;
      }
      const example = line.match(/^-\s*([^:]+):\s*"/);
      if (example) {
        const label = normalize(example[1]);
        if (label) return label === mode;
      }
      return true;
    })
    .join('\n');
}

/**
 * 造一个「已物化 + 已同步上游」的插件根。上游正文用夹具，不碰仓库里的真实内容。
 *
 * `manifestFrom` 是**清单里记的那份上游**的正文，默认就是磁盘上这份。
 * 「上游更新了」的场景必须把它指向**旧**文本——否则清单与磁盘一致，
 * 覆盖层会正确地认为「这正是我译过的那一版」而覆盖上去，测不到回落分支。
 */
function fixture({ upstream, translated, manifestFrom = upstream, withBackup = false }) {
  const root = mkdtempSync(join(tmpdir(), 'ponytail-i18n-'));
  for (const [name, body] of Object.entries(upstream)) {
    const dir = join(root, 'skills', name);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'SKILL.md'), body, 'utf8');
  }
  const files = {};
  for (const name of Object.keys(translated ?? {})) {
    const dir = join(root, 'i18n', 'zh-CN', 'skills', name);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'SKILL.md'), translated[name], 'utf8');
    const basis = manifestFrom?.[name];
    if (typeof basis === 'string') files[name] = sha256(basis);
  }
  writeFileSync(
    join(root, 'i18n', 'zh-CN', 'manifest.json'),
    `${JSON.stringify(
      { locale: 'zh-CN', upstream: { package: '@dietrichgebert/ponytail', version: '5.1.0' }, files },
      null,
      2,
    )}\n`,
    'utf8',
  );
  if (withBackup) {
    const backup = join(root, '.xlink-i18n', 'zh-CN', 'upstream');
    mkdirSync(backup, { recursive: true });
    for (const [name, body] of Object.entries(upstream)) {
      writeFileSync(join(backup, `${name}SKILL.md`), body, 'utf8');
    }
  }
  return root;
}

const UPSTREAM = `---
name: ponytail
description: Lazy senior dev mode.
---

# Ponytail

You are a lazy senior developer.

## Levels

| Level | Behavior |
|-------|----------|
| **lite** | Build what was asked. |
| **full** | The rules above. Default. |
| **ultra** | Also question the request. |
`;

const TRANSLATED = `---
name: ponytail
description: 以懒散资深工程师的方式做编码任务。
---

# Ponytail

你是一名懒散的资深开发者。

## 档位

| 档位 | 行为 |
|-------|----------|
| **lite** | 只做被要求的改动。 |
| **full** | 上面这些规则。默认档。 |
| **ultra** | 还要质疑需求本身。 |
`;

test('上游正文与清单指纹一致时覆盖成中文', () => {
  const root = fixture({ upstream: { ponytail: UPSTREAM }, translated: { ponytail: TRANSLATED } });
  const result = applyLocale(root);
  assert.deepEqual(result.applied, ['ponytail']);
  assert.deepEqual(result.restored, []);
  assert.equal(result.stale, false);
  assert.equal(readFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), 'utf8'), TRANSLATED);
});

test('覆盖前存下上游原件——没有它回落就是删除上游内容', () => {
  const root = fixture({ upstream: { ponytail: UPSTREAM }, translated: { ponytail: TRANSLATED } });
  applyLocale(root);
  const backup = join(root, '.xlink-i18n', 'zh-CN', 'upstream', 'ponytailSKILL.md');
  assert.equal(readFileSync(backup, 'utf8'), UPSTREAM, '原件必须逐字存下');
});

test('重复应用是幂等的：不重复写盘，也不把已覆盖的文件误判成上游更新', () => {
  const root = fixture({ upstream: { ponytail: UPSTREAM }, translated: { ponytail: TRANSLATED } });
  applyLocale(root);
  const second = applyLocale(root);
  assert.deepEqual(second.applied, [], '第二次不该再覆盖');
  assert.deepEqual(second.skipped, ['ponytail']);
  assert.equal(second.stale, false, '已覆盖的文件不是「上游变了」');
  assert.equal(readFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), 'utf8'), TRANSLATED);
});

test('上游内容变了且有原件时回落英文，并把陈旧如实报出来', () => {
  const root = fixture({
    upstream: { ponytail: UPSTREAM },
    translated: { ponytail: TRANSLATED },
    withBackup: true,
  });
  applyLocale(root);
  const moved = `${UPSTREAM}\n<!-- upstream 5.2.0 changed this -->\n`;
  writeFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), moved, 'utf8');

  const result = applyLocale(root);
  assert.deepEqual(result.restored, ['ponytail']);
  assert.deepEqual(result.moved, ['ponytail']);
  assert.equal(result.stale, true);
  assert.equal(readFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), 'utf8'), UPSTREAM);
  assert.match(result.reason, /上游内容已更新/);
});

test('上游变了但没有原件时一个字都不写（当前那份就是新的上游原文）', () => {
  const brandNew = '---\nname: ponytail\ndescription: 全新上游\n---\n\nbrand new body\n';
  const root = fixture({
    upstream: { ponytail: brandNew },
    translated: { ponytail: TRANSLATED },
    manifestFrom: { ponytail: UPSTREAM }, // 清单仍记着旧的那一版
  });
  const result = applyLocale(root);
  assert.deepEqual(result.moved, ['ponytail']);
  assert.deepEqual(result.restored, []);
  assert.equal(result.stale, true);
  assert.equal(
    readFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), 'utf8'),
    brandNew,
    '不能拿旧中文盖掉新的上游原文',
  );
});

test('上游缺某个技能时不凭空造出来', () => {
  const root = fixture({
    upstream: {},
    translated: { ponytail: TRANSLATED },
  });
  const result = applyLocale(root);
  assert.deepEqual(result.applied, []);
});

test('没有中文层清单时保持上游原文，也不报错', () => {
  const root = mkdtempSync(join(tmpdir(), 'ponytail-i18n-empty-'));
  mkdirSync(join(root, 'skills', 'ponytail'), { recursive: true });
  writeFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), UPSTREAM, 'utf8');
  const result = applyLocale(root);
  assert.match(result.reason, /没有中文层清单/);
  assert.equal(readFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), 'utf8'), UPSTREAM);
  rmSync(root, { recursive: true, force: true });
});

test('改了中文层之后，重新应用会写新译文而不是当成上游更新', () => {
  const root = fixture({ upstream: { ponytail: UPSTREAM }, translated: { ponytail: TRANSLATED } });
  applyLocale(root);
  const revised = TRANSLATED.replace('懒散的资深开发者', '懒得写代码的资深开发者');
  const dir = join(root, 'i18n', 'zh-CN', 'skills', 'ponytail');
  writeFileSync(join(dir, 'SKILL.md'), revised, 'utf8');

  const result = applyLocale(root);
  assert.deepEqual(result.applied, ['ponytail']);
  assert.equal(result.stale, false, '我们自己的译文更新不算上游变了');
  assert.equal(readFileSync(join(root, 'skills', 'ponytail', 'SKILL.md'), 'utf8'), revised);
});

// ── 仓库里那份真译文 ────────────────────────────────────────────────────────

/** 随包的中文层：清单里的每个键都必须真有对应文件，且清单非空。 */
test('随包中文层的清单与文件一一对应', () => {
  const manifest = readManifest(pluginRoot);
  assert.notEqual(manifest, null, 'i18n/zh-CN/manifest.json 必须存在且可解析');
  assert.ok(Object.keys(manifest.files).length > 0, '清单不能是空的——空清单等于没有中文层');
  for (const name of Object.keys(manifest.files)) {
    const path = join(pluginRoot, 'i18n', 'zh-CN', 'skills', name, 'SKILL.md');
    const raw = readFileSync(path, 'utf8');
    assert.match(raw, new RegExp(`^---\\s*\\nname:\\s*${name}\\s*$`, 'm'), `${name} 的 frontmatter 名字对不上`);
    assert.ok(digest(raw).length === 64);
    assert.match(manifest.files[name], /^[0-9a-f]{64}$/, `${name} 的上游指纹必须是 sha256`);
  }
});

/**
 * 真实译文必须扛得住上游**自己的**强度裁剪。
 *
 * 这是中文层最容易悄悄坏掉的一处：`| **full** |` 被写成 `| **全量** |` 时，
 * full 档会把那一行整行删掉——文件读起来完全正常，只有注入给模型的规则集少了
 * 一条。所以按档位逐个验「留下的正好是那一行」，而不是只验文件存在。
 */
test('真实译文在三档下各自只留下自己那一行（上游的裁剪规则）', () => {
  const path = join(pluginRoot, 'i18n', 'zh-CN', 'skills', 'ponytail', 'SKILL.md');
  const raw = readFileSync(path, 'utf8');
  for (const mode of ['lite', 'full', 'ultra']) {
    const rows = filterSkillBodyForMode(raw, mode)
      .split('\n')
      .filter((line) => /^\|\s*\*\*/.test(line));
    assert.equal(rows.length, 1, `${mode} 档应当恰好留下一行档位行，实际 ${rows.length} 行`);
    assert.match(rows[0], new RegExp(`\\*\\*${mode}\\*\\*`), `${mode} 档留下的是别的档位行：${rows[0]}`);
  }
});

/** 档位行必须字面保留英文档位名——它们是裁剪规则的键，中文化就等于删档位。 */
test('译文里的档位标签保持字面的 lite/full/ultra', () => {
  const raw = readFileSync(join(pluginRoot, 'i18n', 'zh-CN', 'skills', 'ponytail', 'SKILL.md'), 'utf8');
  for (const mode of ['lite', 'full', 'ultra']) {
    assert.match(raw, new RegExp(`^\\|\\s*\\*\\*${mode}\\*\\*\\s*\\|`, 'm'), `缺 ${mode} 的档位行`);
  }
});

/** 每个技能都要有中文 description——技能列表显示的就是它。 */
test('六个技能都有中文 description', () => {
  const manifest = readManifest(pluginRoot);
  for (const name of Object.keys(manifest.files)) {
    const raw = readFileSync(join(pluginRoot, 'i18n', 'zh-CN', 'skills', name, 'SKILL.md'), 'utf8');
    const description = /^description:\s*>?\s*([\s\S]*?)(?=^---|\n\n)/mu.exec(raw)?.[1] ?? '';
    assert.match(description, /[一-龥]/, `${name} 的 description 里没有中文`);
  }
});