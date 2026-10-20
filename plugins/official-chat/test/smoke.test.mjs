// 插件的**形状与纪律**判据（与 `client.test.mjs` 的行为判据分工）。
//
// ## 为什么单��这一层
//
// `client.test.mjs` 证的是「插件的行为对不对」；这里证的是「插件的**形状**对不对」——
// 而形状错了，行为测试可能照样绿：
//
//   · `dsh.client.platform` 不是 `"web"`、或 `exports["./client"]` 缺失 →
//     内核的 `dsh-client-modules` **根本不会把这个包放进浏览器模块图**，
//     客户端半边一行都不执行（`K\dsh-client-modules\lib\index.js:105-119`）。
//   · `dsh.client.inject` 里写了一个内核里**不存在**的包名 → 装配期报错，
//     插件整体起不来。
//   · 客户端束出现**顶层 const/let/class** → client-hmr 第二次求值直接
//     `SyntaxError`，entry 当场死掉，而界面照常显示「已启用」。
//
// 这三类事故的共同点：**症状是静默的**。所以形状要有独立门禁。

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (relative) => readFileSync(new URL(`../${relative}`, import.meta.url), 'utf8');
const pkg = JSON.parse(read('package.json'));

/** 剥掉注释再判——文件头的解释性注释里原样写着这些词，判据不许命中自己。 */
const code = (relative) =>
  read(relative)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');

test('package.json 的形状：内核靠它把客户端半边放进浏览器模块图', () => {
  assert.equal(pkg.name, 'xlink-official-chat');
  assert.equal(pkg.dsh?.client?.platform, 'web', 'platform 必须是 web，否则不进浏览器模块图');
  assert.ok(pkg.exports['./client'], 'exports 必须有 "./client"');
  assert.ok(pkg.exports['.'], 'exports 必须有 "."（host 半边）');
  assert.ok(pkg.exports['./package.json'], 'exports 必须有 "./package.json"');
  assert.equal(pkg.main, 'host/index.js');
});

test('dsh.client.inject 里的包名必须是内核里真实存在的包', () => {
  // 这三个包在 release 0.2.1-rc.2 的
  // `<kernel>/node_modules/@deepseek-ai/` 下都存在（逐个 ls 过）。
  // **写错包名的症状是静默的**：装配期报错，插件整体不加载。
  //
  // 注意：`@deepseek-ai/dsh-client-ui-slots` **不在** rc.2 的 @deepseek-ai 目录里
  // （只在浏览器模块图里），所以 inject 列表里不能写它。
  const known = new Set([
    '@deepseek-ai/dsh-api-session-controller',
    '@deepseek-ai/dsh-client-ui-conversation',
    '@deepseek-ai/dsh-client-ui-input-trigger',
  ]);
  for (const name of pkg.dsh?.client?.inject ?? []) {
    assert.ok(name.startsWith('@deepseek-ai/'), `inject 里应写内核包名：${name}`);
    assert.ok(known.has(name), `inject 的包名不在已核对的清单里：${name}`);
  }
});

test('host 半边导出 apply / inject / name，且 name 与包名一致', async () => {
  const mod = await import('../host/index.js');
  assert.equal(typeof mod.apply, 'function');
  assert.ok(Array.isArray(mod.inject), 'inject 是「注入的服务名数组」');
  assert.equal(mod.name, pkg.name, 'settingsNs 与包名必须一致');
  assert.deepEqual(mod.inject, [], 'G1 的 host 半边不消费任何内核服务');
});

test('客户端束没有顶层声明（HMR 重求值会直接 SyntaxError）', () => {
  const topLevel = code('client/client.js')
    .split('\n')
    .filter((line) => /^(const|let|class|var)\s/.test(line));
  assert.deepEqual(topLevel, [], `顶层声明会踩 HMR：${topLevel.join(' | ')}`);
});

test('客户端束自包含：不 import 别的文件（浏览器里同目录文件不在模块图）', () => {
  const source = code('client/client.js');
  assert.ok(!/\bimport\s+.*\bfrom\s+['"]\./.test(source), '客户端束不许 import 相对路径');
  assert.ok(!/\brequire\s*\(/.test(source), '客户端束不许 require');
});

test('不写死内核版本目录（升级后那个目录还在，于是会静默跑错内核）', () => {
  for (const file of ['client/client.js', 'host/index.js']) {
    assert.ok(!/kernels\/[\d.]+-(alpha|beta|rc)/.test(code(file)), `${file} 里写死了内核版本目录`);
  }
});

test('不碰用户真实数据目录，只写临时目录', () => {
  for (const file of ['client/client.js', 'host/index.js']) {
    assert.ok(!/\.dsh-xlink/.test(code(file)), `${file} 里出现了用户真实数据目录`);
  }
});

test('证据里不出现凭据形状的东西（token / cookie / key）', () => {
  for (const file of ['client/client.js', 'host/index.js']) {
    const source = code(file);
    assert.ok(!/sk-[A-Za-z0-9]{8,}/.test(source), `${file} 里出现了像 API key 的字面量`);
    assert.ok(!/Bearer\s+[A-Za-z0-9._-]{8,}/.test(source), `${file} 里出现了 Authorization 头`);
    assert.ok(!/cookie/i.test(source), `${file} 里出现了 cookie`);
  }
});

test('夹具是固定的公开文本，不含真实官网内容与私密正文', () => {
  const source = read('client/client.js');
  assert.match(source, /【G1 固定正文 Alpha】/);
  assert.match(source, /【G1 固定正文 Beta】/);
  assert.ok(!/https?:\/\/(?!127\.0\.0\.1)/.test(code('client/client.js')), '客户端束里不该有真实官网地址');
});

// 2026-10-20 的教训：本机有**两棵**内核数据目录——现役的
// `C:\Users\zxx\.dsh-xlink\dsh\<family>\desktop[-dev]\` 与旧的
// `C:\Users\zxx\.dsh\`（平铺布局）。两边的**行号不一样**，只写行号，下一个人
// 无从判断自己读的是哪棵树——所以内核引用一律写成 `<包>/<文件>@<版本>:<行号>`，
// 并在文件头给出版本号 + SHA-256 + 绝对路径。
test('引内核行号必须带版本号（防止又引到旧树）', () => {
  for (const file of ['client/client.js', 'host/index.js']) {
    for (const match of read(file).matchAll(/`([^`\n]*?):(\d+(?:-\d+)?)`/g)) {
      const path = match[1];
      if (path.startsWith('plugins/')) continue; // 仓库内文件，不受此限
      assert.ok(
        path.includes('@'),
        `${file} 里的引用 \`${path}\` 没带版本号：写成 \`<包>/<文件>@<版本>:<行号>\``,
      );
    }
  }
});

test('文件头给出证据基线：版本号 + SHA-256 + 绝对路径', () => {
  const source = read('client/client.js');
  assert.match(source, /0\.2\.1-rc\.2/, '必须写明内核版本号');
  assert.match(source, /[0-9a-fA-F]{64}/, '必须给出内核文件的 SHA-256');
  assert.match(source, /dsh-xlink\\dsh\\desktop\\kernels/, '必须给出内核树绝对路径');
});

test('客户端束里不再有第二个变体（PM 2026-10-20 拍板：P0 不并行做两个方案）', () => {
  const source = code('client/client.js');
  assert.ok(!/mode\s*[:=]/.test(source), '客户端束里不该再有模式开关');
  assert.ok(
    !/input-insert-reference/.test(source),
    '插件不该自己插 chip（那是在绕开内核的输入事件与 span CAS）',
  );
});

test('文件头写明「互斥」与被违反的上游需求（G1-1），以及将来上游支持后的退让路径', () => {
  // 这三条是**纪律**，不是文档洁癖：
  //   · 哪两条不可兼得（「就绪才插 chip」×「不得漏发」）；
  //   · 违反的是哪一条接口缺口（只有 G1-1 能解）；
  //   · 上游补上之后甲形态怎么退让。
  // 少写一条，下一个改形态的人就会以为「就绪才插 chip」是随手能改回去的，
  // 于是把「不漏发」那条悄悄丢掉——而丢掉的症状是**静默漏发**。
  //
  // 判据读**文件头注释**（`read` 不剥注释），所以它命中的正是解释性文字。
  const source = read('client/client.js');
  assert.match(source, /互斥/, '必须写明哪两条不可兼得');
  assert.match(source, /G1-1/, '必须点名被违反的上游需求（G1-1：需要 onPick 异步化）');
  assert.match(source, /退让/, '必须写明上游支持 onPick 异步化后甲形态怎么退让');
  assert.match(
    source,
    /occurrences\.length===0/,
    '互斥推导的依据（内核在草稿里没有本来源 occurrence 时会把纯文本直接发出去）必须留在文件头',
  );
});
