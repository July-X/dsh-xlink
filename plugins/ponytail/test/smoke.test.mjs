import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { createProvider, parseSkill, PROVIDER_NAME, SKILL_NAME_RE } from '../host/skills.js';

const pluginRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const hostSource = readFileSync(join(pluginRoot, 'host', 'index.js'), 'utf8');

/** 与内核 `dsh-skill` 的 `BUNDLED_SKILL_RANK` 同值。上游若改了这个数字、
 *  本仓还按旧数字注册时，下面那条断言就是那个看得见的信号。 */
const BUNDLED_SKILL_RANK = 600;

/**
 * 内核对 `list()` 候选的**逐字段**校验，原样抄自
 * `@deepseek-ai/dsh-skill/lib/index.js` 的 `validateCandidate`（0.2.1-alpha.1 实读）。
 *
 * 这段抄写是有意的：它抄的是**内核会做的事**，不是本仓的意图。第一版判据只读
 * 源码文本看「有没有写过 rank / invocation」，而那些字段当时只填在 `get()` 里，
 * `list()` 返回的候选缺 `source` / `rank` / `provider` —— 真机启动立刻
 * `skill provider "dsh-ponytail" returned skill "ponytail" with a non-string source`。
 * 判据钉「算出来的结果」之后，那一行再也藏不住。
 */
function validateCandidateLikeKernel(candidate, providerName) {
  assert.equal(typeof candidate.name, 'string', 'name 必须是字符串');
  assert.ok(SKILL_NAME_RE.test(candidate.name), `invalid skill name "${candidate.name}"`);
  assert.equal(typeof candidate.description, 'string', 'description 必须是字符串');
  assert.ok(candidate.description.length > 0, 'description 不能为空');
  const policy = candidate.invocation;
  assert.equal(typeof policy, 'object', 'invocation 必须是对象');
  assert.ok(!Array.isArray(policy));
  assert.equal(typeof policy.modelInvocable, 'boolean');
  assert.equal(typeof policy.userInvocable, 'boolean');
  assert.equal(typeof candidate.source, 'string', 'source 必须是字符串（真机踩过这条）');
  assert.equal(typeof candidate.rank, 'number', 'rank 必须是数字（真机踩过这条）');
  assert.ok(Number.isFinite(candidate.rank));
  assert.equal(typeof candidate.provider, 'string');
  assert.equal(candidate.provider, providerName, 'provider 必须与 provider 名逐字相同');
  assert.ok(candidate.path === undefined || typeof candidate.path === 'string');
}

/** 造一个带夹具技能目录的插件根（**不碰仓库里��真实内容**）。 */
function fixtureRoot(specs) {
  const root = mkdtempSync(join(tmpdir(), 'ponytail-fixture-'));
  for (const [name_, body] of Object.entries(specs)) {
    const dir = join(root, 'skills', name_);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'SKILL.md'), body, 'utf8');
  }
  return root;
}

const UPSTREAM_SKILL = `---
name: ponytail-review
description: >
  评审当前改动里的过度设计。
---

# review

body
`;

test('上游技能从物化目录读到，候选逐字段过得了内核校验', async () => {
  const root = fixtureRoot({ 'ponytail-review': UPSTREAM_SKILL });
  const provider = createProvider(root, BUNDLED_SKILL_RANK);
  const candidates = await provider.list();
  assert.equal(provider.name, PROVIDER_NAME);
  assert.equal(candidates.length, 1);
  for (const candidate of candidates) validateCandidateLikeKernel(candidate, provider.name);
  assert.equal(candidates[0].rank, BUNDLED_SKILL_RANK);
});

test('get() 取得到正文，且与 list() 的那一项是同一个技能', async () => {
  const root = fixtureRoot({ 'ponytail-review': UPSTREAM_SKILL });
  const provider = createProvider(root, BUNDLED_SKILL_RANK);
  const [first] = await provider.list();
  const definition = await provider.get(first, {});
  assert.ok(definition !== undefined, 'get() 不该返回 undefined');
  assert.equal(definition.name, first.name);
  assert.equal(definition.provider, PROVIDER_NAME);
  assert.equal(typeof definition.content, 'string');
  assert.match(definition.content, /# review/);
  // rank / locator 是 provider 自己消费的，**不许**漏回定义里：内核会把它们
  // 当作技能元数据往下传。
  assert.equal(definition.rank, undefined);
  assert.equal(definition.locator, undefined);
  // 读不出来的正文按「这个技能没了」处理，而不是抛错炸掉整个发现过程。
  const missing = await provider.get({ ...first, locator: join(root, 'nope', 'SKILL.md') }, {});
  assert.equal(missing, undefined);
});

test('目录名与 frontmatter 的 name 不一致时仍能取到正文（按路径取，不按名字反推）', async () => {
  const root = fixtureRoot({ wrongDir: `---\nname: ponytail-review\ndescription: d\n---\nbody` });
  const provider = createProvider(root, BUNDLED_SKILL_RANK);
  const [candidate] = provider.candidates;
  // 内核按 `name` 注册与展示，正文则由我们给的 `locator` 取。上游哪天改了目录名
  // 而没改 frontmatter（或反过来），这里**不会**把技能读丢——取正文的路径是发现
  // 那一刻就定下来的，不是用 name 反推的。
  assert.equal(candidate.name, 'ponytail-review');
  assert.ok(candidate.path.endsWith(join('wrongDir', 'SKILL.md')));
  const definition = await provider.get(candidate, {});
  assert.equal(definition.content, 'body');
});

test('自带技能与上游技能合并，同名时上游优先', () => {
  const root = fixtureRoot({ 'ponytail-review': UPSTREAM_SKILL });
  const adapter = join(root, 'adapter', 'skills', 'ponytail-review');
  mkdirSync(adapter, { recursive: true });
  writeFileSync(
    join(adapter, 'SKILL.md'),
    '---\nname: ponytail-review\ndescription: 我们自己那份\n---\nours',
    'utf8',
  );
  const ours = join(root, 'adapter', 'skills', 'ponytail-update');
  mkdirSync(ours, { recursive: true });
  writeFileSync(
    join(ours, 'SKILL.md'),
    '---\nname: ponytail-update\ndescription: 立刻拉上游最新版\n---\nupdate',
    'utf8',
  );
  const provider = createProvider(root, BUNDLED_SKILL_RANK);
  const names = provider.candidates.map((c) => c.name);
  assert.deepEqual(names, ['ponytail-review', 'ponytail-update']);
  // 上游那份赢：它先被收集。取错的话，用户拿到的是我们那份只讲「怎么更新」的
  // 正文，而不是上游真正的评审规则。
  assert.equal(
    provider.candidates.find((c) => c.name === 'ponytail-review').description.includes('评审'),
    true,
  );
});

test('上游还没同步时，插件仍注册我们自己的技能（不是空注册）', () => {
  const root = join(fixtureRoot({}), 'adapter-root');
  const ours = join(root, 'adapter', 'skills', 'ponytail-update');
  mkdirSync(ours, { recursive: true });
  writeFileSync(
    join(ours, 'SKILL.md'),
    '---\nname: ponytail-update\ndescription: 立刻拉上游最新版\n---\nupdate',
    'utf8',
  );
  const provider = createProvider(root, BUNDLED_SKILL_RANK);
  assert.deepEqual(provider.candidates.map((c) => c.name), ['ponytail-update']);
});

test('host 适配器只是装配：声明了注册技能所需的东西', () => {
  assert.match(hostSource, /const inject = \["skills"\]/u, '必须声明 inject = ["skills"]');
  assert.match(hostSource, /const PACKAGE_ID = "xlink-ponytail"/u);
  assert.match(hostSource, /registerProvider\(/u, '必须调用 ctx.skills.registerProvider');
  assert.match(
    hostSource,
    /createProvider\(ROOT, BUNDLED_SKILL_RANK\)/u,
    'provider 必须用内核给的 rank、并以**插件根**构造（上游内容在 skills/，自带技能在 adapter/skills/）',
  );
  assert.match(
    hostSource,
    /from "@deepseek-ai\/dsh-hooks-claude-code"/u,
    '必须 import 内核的 Claude Code hook 桥——上游的档位切换与会话级注入靠它',
  );
  assert.match(hostSource, /ctx\.plugin\(/u, '桥要由这里显式挂上去（它不扫插件目录）');
  // 600 这个数字**不许**在代码里再抄一份：抄一份的话，内核改优先级时本仓会
  // 安静地按旧值注册。所以判据钉的是「它从包 import 来」，而不是「它等于 600」。
  assert.match(
    hostSource,
    /import \{ BUNDLED_SKILL_RANK \} from "@deepseek-ai\/dsh-skill";/u,
    'BUNDLED_SKILL_RANK 必须从内核包 import，不能抄成字面量',
  );
  assert.doesNotMatch(hostSource, /BUNDLED_SKILL_RANK\s*=\s*\d/u);
  // 候选字段不许在 index.js 里再拼一份：第一版就是这么把 list() 拼漏的
  // （只填了 name/description），而当时判据读源码看到 rank 在文件里就放行了。
  assert.doesNotMatch(
    hostSource,
    /source:\s*['"]bundled['"]|rank:\s*BUNDLED_SKILL_RANK|invocation:\s*\{/u,
    '候选字段只在 skills.js 里构造一次：index.js 拼第二份就是第一版那个 bug 的形状',
  );
});

test('host 适配器不引第三方依赖（物化目录只链了两个 peer 包）', () => {
  // 这条对着 `BuiltinSpec::peers` 写：物化目录的 node_modules 里只有壳链过来的
  // `@deepseek-ai/dsh-skill` 与 `@deepseek-ai/dsh-hooks-claude-code`。任何别的
  // import 都会在运行时解析失败，而失败发生在内核启动阶段——表现为整个工作台
  // 起不来，而不是「插件没生效」。
  const imports = [...hostSource.matchAll(/^import\s+[\s\S]*?from\s+"([^"]+)";/gmu)].map(
    (m) => m[1],
  );
  const foreign = imports.filter(
    (spec) => !spec.startsWith('node:') && !spec.startsWith('@deepseek-ai/') && !spec.startsWith('.'),
  );
  assert.deepEqual(foreign, [], `host/index.js 只能 import node: 内置模块、相对路径与 @deepseek-ai/* —— ${foreign.join(', ')}`);
});

test('解析器 fail-closed：认不出的一律不注册', () => {
  for (const raw of [
    '',
    '没有 frontmatter 的一段正文',
    '---\n---\nbody',
    '---\nname: Ponytail\ndescription: 大写名\n---\nbody',
    '---\nname: ponytail\ndescription:\n---\nbody',
    '---\nname: pon ytail\ndescription: 名字带空格\n---\nbody',
    '---\ndescription: 没有 name\n---\nbody',
  ]) {
    assert.equal(parseSkill(raw), null, `这本该被拒绝：${JSON.stringify(raw.slice(0, 40))}`);
  }
});

test('块正文里的字段名不会被当成顶层键', () => {
  const parsed = parseSkill(
    '---\nname: real-name\ndescription: >\n  这里提到 name: fake-name 与 description: 别的\n---\nbody',
  );
  assert.equal(parsed.name, 'real-name', '块正文里的 name: 不得覆盖顶层 name');
  // 块正文被**折进 description**（那行本来就该出现），而不是变成新的键。
  // 判据钉的是折叠后的内容本身：写成「不含 fake-name」是错的——正文里本来
  // 就有这几个字符，解析器没有义务把它们藏起来。
  assert.ok(
    parsed.description.includes('这里提到 name: fake-name'),
    `块正文应折进 description，实际拿到 ${JSON.stringify(parsed.description)}`,
  );
  assert.ok(parsed.description.includes('别的'));
});
