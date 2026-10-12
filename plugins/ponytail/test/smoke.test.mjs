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

import { parseSkill, readCandidates } from '../host/skills.js';

const pluginRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const SKILLS_DIR = join(pluginRoot, 'skills');
const hostSource = readFileSync(join(pluginRoot, 'host', 'index.js'), 'utf8');

/** 与内核 `dsh-skill` 的 `BUNDLED_SKILL_RANK` 同值。上游若改了这个数字、
 *  本仓还按旧数字注册时，本条就是那个看得见的信号。 */
const BUNDLED_SKILL_RANK = 600;

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

  const candidates = readCandidates(SKILLS_DIR);
  assert.equal(
    candidates.length,
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

test('host 适配器声明了注册技能所需的东西', () => {
  // 缺任何一样，症状都是「插件装上了、工作台里没有技能」，且没有一行报错。
  assert.match(hostSource, /const inject = \["skills"\]/u, '必须声明 inject = ["skills"]');
  assert.match(hostSource, /const PACKAGE_ID = "xlink-ponytail"/u);
  assert.match(
    hostSource,
    /registerProvider\(/u,
    '必须调用 ctx.skills.registerProvider：注册 provider 而不是内存里的一段正文',
  );
  assert.match(hostSource, /rank: BUNDLED_SKILL_RANK/u, '必须是 bundled rank');
  assert.match(
    hostSource,
    /modelInvocable: true,\s*userInvocable: true/u,
    '技能要同时能被模型与用户调用：只给模型的话 /ponytail-review 这类命令用户敲不出来',
  );
  // 600 这个数字**不许**在代码里再抄一份：抄一份的话，内核改优先级时本仓会
  // 安静地按旧值注册，且没有任何报错。所以判据钉的是「它从包 import 来」，
  // 而不是「它等于 600」——后者恰恰是在允许第二份真值。
  assert.match(
    hostSource,
    /import \{ BUNDLED_SKILL_RANK \} from "@deepseek-ai\/dsh-skill";/u,
    'BUNDLED_SKILL_RANK 必须从内核包 import，不能抄成字面量',
  );
  assert.equal(
    BUNDLED_SKILL_RANK,
    600,
    '本仓文档（skill-management.md 的 rank 表）与文档化的内核常量应当一致；变了要一起改',
  );
  assert.doesNotMatch(
    hostSource,
    /BUNDLED_SKILL_RANK\s*=\s*\d/u,
    '不许给 BUNDLED_SKILL_RANK 一个本地字面量',
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