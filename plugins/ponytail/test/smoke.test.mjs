// 内置 ponytail 插件的冒烟测试：随包的六个 SKILL.md 真能被解析成技能。
//
// 为什么要有这个测试：插件是**运行时**被内核加载的 JS，`npm run check` 的任何
// 一环都不执行它——Rust 只负责物化文件，前端只读命令载荷。于是「frontmatter
// 解析写错了」「目录名与 frontmatter 的 name 对不上」这两类错在装上应用之前
// 没有任何东西会响，而症状是工作台里一个技能都不出现（用户分不清是插件没装、
// 没启用，还是正文没注册）。
//
// 解析器住在 `host/skills.js` 而不是 `host/index.js` 里，正是为了这件事：
// index.js 唯一的外部 import 是 `@deepseek-ai/dsh-skill`（物化目录里由壳 peer
// 链接过来，仓库里并不存在），import 它需要先造一个内核。所以测的是**运行时
// 用的那份解析器**，不是测试里抄一遍的规则。
//
// 跑法：`node --test plugins/ponytail/test/`（`npm run test:plugin`）。
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

import { createProvider, parseSkill, PROVIDER_NAME, SKILL_NAME_RE } from '../host/skills.js';

const pluginRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const SKILLS_DIR = join(pluginRoot, 'skills');
const hostSource = readFileSync(join(pluginRoot, 'host', 'index.js'), 'utf8');

/** 与内核 `dsh-skill` 的 `BUNDLED_SKILL_RANK` 同值。上游若改了这个数字、
 *  本仓还按旧数字注册时，本条就是那个看得见的信号。 */
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

test('随包的六个技能都在，目录名与 frontmatter 的 name 一致', () => {
  const dirs = readdirSync(SKILLS_DIR, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name);
  assert.deepEqual(dirs.sort(), [
    'ponytail',
    'ponytail-audit',
    'ponytail-debt',
    'ponytail-gain',
    'ponytail-help',
    'ponytail-review',
  ]);

  const provider = createProvider(SKILLS_DIR, BUNDLED_SKILL_RANK);
  assert.equal(
    provider.candidates.length,
    dirs.length,
    '每一个目录都要被认成一个技能：少认一个的话，界面上的插件说明与工作台里实际能用的对不上',
  );
  for (const dir of dirs) {
    const raw = readFileSync(join(SKILLS_DIR, dir, 'SKILL.md'), 'utf8');
    const parsed = parseSkill(raw);
    assert.ok(parsed !== null, `${dir}: frontmatter 解析失败，启用后工作台里会没有这个技能`);
    assert.equal(
      parsed.name,
      dir,
      `${dir}: frontmatter 的 name 必须与目录名一致——不一致时内核按 name 注册、`
        + '而资源目录在另一个名字下，技能加载时找不到正文',
    );
    assert.ok(parsed.description.length > 0, `${dir}: description 不能为空`);
    assert.ok(parsed.body.length > 0, `${dir}: 正文不能为空`);
  }
});

test('provider 的 list() 每个候选都过得了内核的逐字段校验', async () => {
  const provider = createProvider(SKILLS_DIR, BUNDLED_SKILL_RANK);
  const candidates = await provider.list();
  assert.equal(provider.name, PROVIDER_NAME);
  assert.equal(candidates.length, 6);
  for (const candidate of candidates) {
    validateCandidateLikeKernel(candidate, provider.name);
  }
  // rank 必须是**传进来的那个 bundled 值**，而不是硬编码在 skills.js 里：
  // 抄一份的话，内核改优先级时本仓会安静地按旧值注册。
  assert.ok(candidates.every((c) => c.rank === BUNDLED_SKILL_RANK));
});

test('get() 取得到正文，且与 list() 的那一项是同一个技能', async () => {
  const provider = createProvider(SKILLS_DIR, BUNDLED_SKILL_RANK);
  const [first] = await provider.list();
  const definition = await provider.get(first, {});
  assert.ok(definition !== undefined, 'get() 不该返回 undefined');
  assert.equal(definition.name, first.name);
  assert.equal(definition.provider, PROVIDER_NAME);
  assert.equal(typeof definition.content, 'string');
  assert.ok(definition.content.includes('ponytail') || definition.content.length > 50);
  // rank / locator 是 provider 自己消费的，**不许**漏回定义里：内核会把它们
  // 当作技能元数据往下传。
  assert.equal(definition.rank, undefined);
  assert.equal(definition.locator, undefined);
  // 读不出来的正文按「这个技能没了」处理，而不是抛错炸掉整个发现过程。
  const missing = await provider.get({ ...first, locator: join(SKILLS_DIR, 'nope', 'SKILL.md') }, {});
  assert.equal(missing, undefined);
});

test('host 适配器只是装配：声明了注册技能所需的东西', () => {
  // 缺任何一样，症状都是「插件装上了、工作台里没有技能」，且没有一行报错。
  assert.match(hostSource, /const inject = \["skills"\]/u, '必须声明 inject = ["skills"]');
  assert.match(hostSource, /const PACKAGE_ID = "xlink-ponytail"/u);
  assert.match(hostSource, /registerProvider\(/u, '必须调用 ctx.skills.registerProvider');
  assert.match(
    hostSource,
    /createProvider\(SKILLS_DIR, BUNDLED_SKILL_RANK\)/u,
    'provider 必须用内核给的 rank 构造',
  );
  // 600 这个数字**不许**在代码里再抄一份：抄一份的话，内核改优先级时本仓会
  // 安静地按旧值注册，且没有任何报错。所以判据钉的是「它从包 import 来」，
  // 而不是「它等于 600」——后者恰恰是在允许第二份真值。
  assert.match(
    hostSource,
    /import \{ BUNDLED_SKILL_RANK \} from "@deepseek-ai\/dsh-skill";/u,
    'BUNDLED_SKILL_RANK 必须从内核包 import，不能抄成字面量',
  );
  assert.doesNotMatch(
    hostSource,
    /BUNDLED_SKILL_RANK\s*=\s*\d/u,
    '不许给 BUNDLED_SKILL_RANK 一个本地字面量',
  );
  // **候选字段不许在 index.js 里再拼一份**：第一版就是这么把 list() 拼漏的
  // （只填了 name/description），而当时判据读源码看到 rank 在文件里就放行了。
  assert.doesNotMatch(
    hostSource,
    /source:\s*['"]bundled['"]|rank:\s*BUNDLED_SKILL_RANK|invocation:\s*\{/u,
    '候选字段只在 skills.js 里构造一次：index.js 拼第二份就是第一版那个 bug 的形状',
  );
});

test('host 适配器不引第三方依赖（物化目录只链了一个 peer 包）', () => {
  // 这条对着 `BuiltinSpec::peers` 写：物化目录的 node_modules 里只有壳链过来的
  // `@deepseek-ai/dsh-skill`。任何别的 import 都会在运行时解析失败，而失败发生在
  // 内核启动阶段——表现为整个工作台起不来，而不是「插件没生效」。
  const imports = [...hostSource.matchAll(/^import\s+[\s\S]*?from\s+"([^"]+)";/gmu)].map(
    (m) => m[1],
  );
  const foreign = imports.filter(
    (spec) => !spec.startsWith('node:') && !spec.startsWith('@deepseek-ai/') && !spec.startsWith('.'),
  );
  assert.deepEqual(foreign, [], `host/index.js 只能 import node: 内置模块、相对路径与 @deepseek-ai/* —— ${foreign.join(', ')}`);
});

test('解析器 fail-closed：认不出的一律不注册', () => {
  assert.equal(parseSkill('# 没有 frontmatter'), null, '没有 frontmatter 的文件不注册');
  assert.equal(parseSkill('---\ndescription: 有说明没名字\n---\nbody'), null, '缺 name 不注册');
  assert.equal(parseSkill('---\nname: ok-skill\n---\nbody'), null, '缺 description 不注册');
  assert.equal(parseSkill('---\nname: Ok_Skill\ndescription: x\n---\nbody'), null, '非 kebab-case 不注册');
  assert.equal(parseSkill('---\nname: ok\ndescription: ""\n---\nbody'), null, '空说明不注册');
});

test('块正文里的字段名不会被当成顶层键', () => {
  // description 的折行里提到 `name:` 很常见。第一版没处理缩进时，正文里的
  // `name: fake-name` 会覆盖真正的 name——而那种技能在界面上显示成另一个名字，
  // 调用的却是这份正文，且没有任何报错。
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