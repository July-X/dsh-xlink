// 装配层（`host/index.js`）的**执行**判据——这一份存在的唯一理由，是补上「只把源码当
// 文本扫锚点」留下的那个洞。
//
// 2026-10-18 的真机故障：三个工具（`memory_search` / `memory_read` / `memory_propose`）
// 里读的是 `apply` 的局部变量 `workspaceRegistry`，而 `registerTools` 在**模块作用域**——
// 那是个自由变量，一调用必抛 `ReferenceError: workspaceRegistry is not defined`。真机会话里
// 这个错误累计几百次（一个会话 149 次），三个工具**从来没成功过一次**；而当时的判据全绿，
// 因为它们只把 `host/index.js` 当文本扫锚点（`smoke.test.mjs` 里那几条），从不执行它。
//
// 做法：临时目录 + 两个 peer 包的最小桩 + 一个真的回环 HTTP 桩桥，然后 `import()` 整个
// 装配层并**调用三个工具**。不碰内核、不碰网络、不碰用户的数据目录。
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import test from 'node:test';

import { TOKEN_ENV, URL_ENV } from '../host/bridge.js';

// `ui/src/memory/memory.js` 顶层 import 了 `bridge.js`，后者在模块求值期就读
// `window.__TAURI__`——下面「三处词表同一套」那条判据要 import 面板那份，所以桩必须
// 在这里就位（与 `smoke.test.mjs` 同一套桩）。
globalThis.window = {
  __TAURI__: { core: { invoke: () => Promise.resolve([]), Channel: class { onmessage = null; } } },
  addEventListener() {},
  removeEventListener() {},
};
Object.defineProperty(globalThis, 'navigator', { value: { userAgent: 'node' }, configurable: true });

const here = dirname(fileURLToPath(import.meta.url));

/** 一个 peer 包的最小桩。装配层只用到 `defineTool` 与两个 LLM 构造器。 */
function stubPackage(root, name, source) {
  const dir = join(root, 'node_modules', ...name.split('/'));
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, 'package.json'),
    JSON.stringify({ name, version: '0.0.0', type: 'module', main: 'index.js' })
  );
  writeFileSync(join(dir, 'index.js'), source);
}

/**
 * 把 `host/` 连同桩装进一个临时目录并求值，返回装配层的入口与几个探针。
 *
 * 注册表桩会记下每次 `resolveByPath` 的实参——「工具真的用了现取的那份注册表」这条
 * 只能靠它证，靠「没抛异常」证不了（抛在别处也一样没抛 ReferenceError）。
 */
async function loadAssembly() {
  const root = mkdtempSync(join(tmpdir(), 'memory-assembly-'));
  stubPackage(root, '@deepseek-ai/dsh-tools', 'export const defineTool = (spec) => spec;\n');
  stubPackage(
    root,
    '@deepseek-ai/dsh-llm',
    'export class BlockAssembler {}\nexport const createUserMessage = (input) => input;\n'
  );
  cpSync(join(here, '..', 'host'), join(root, 'host'), { recursive: true });
  const mod = await import(pathToFileURL(join(root, 'host', 'index.js')).href);

  const tools = [];
  const lookups = [];
  const resolved = [];
  const pluginHandlers = new Map();
  const ctx = {
    logger: { warn() {}, info() {}, error() {} },
    on(event, cb) { pluginHandlers.set(event, cb); },
    inject() {},
    get(name) {
      lookups.push(name);
      return name === 'workspaceRegistry'
        ? {
            async resolveByPath(path) {
              resolved.push(path);
              return { id: 'ws-1' };
            },
          }
        : undefined;
    },
    tools: { register(tool) { tools.push(tool); } },
    systemPrompt: { context() {} },
    workingDirectory: { get: () => '/repo/x' },
  };
  return {
    apply: mod.apply,
    mod,
    tools,
    ctx,
    lookups,
    resolved,
    pluginHandlers,
    cleanup: () => rmSync(root, { recursive: true, force: true }),
  };
}

/**
 * 回环桩桥：按路由回一份形状与真桥一致的 2xx。
 *
 * 凭据是 `apply()` 里**当场**读的（`readCredentials(process.env)`），所以环境变量必须在
 * 装载之前设好；没凭据时三个工具会停在「记忆桥不可用」那一句上，根本走不到注册表——
 * 那样这条判据就测不到病根。
 */
async function withStubBridge(fn, context = () => ({})) {
  const server = createServer((req, res) => {
    const route = String(req.url || '').split('?')[0];
    const payload =
      route === '/v1/memory/context'
        ? {
            enabled: true,
            hits: [
              {
                id: 'm-1',
                title: '提交前跑检查',
                body: '正文',
                scope: 'general',
                verification: 'verified',
                updated_at: '',
                truncated: false,
              },
            ],
            text: '以下是本机保存的、与当前工作相关的记忆。\n\n[m-1] 提交前跑检查（通用偏好）\n提交前跑 npm run check',
            pending: [],
            ...context(new URL(req.url, 'http://127.0.0.1')),
          }
        : route === '/v1/memory/read'
          ? {
              id: 'm-1',
              title: '提交前跑检查',
              body: '正文',
              scope: 'general',
              state: 'accepted',
              verification: 'verified',
              updated_at: '',
              use_count: 1,
            }
          : route === '/v1/memory/propose'
            ? { id: 'c-1', state: 'pending' }
            : { ok: true };
    const body = JSON.stringify(payload);
    res.writeHead(200, {
      'content-type': 'application/json',
      'content-length': Buffer.byteLength(body),
    });
    res.end(body);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const previous = { url: process.env[URL_ENV], token: process.env[TOKEN_ENV] };
  process.env[URL_ENV] = `http://127.0.0.1:${server.address().port}`;
  process.env[TOKEN_ENV] = 'assembly-test-token';
  try {
    return await fn();
  } finally {
    if (previous.url === undefined) delete process.env[URL_ENV];
    else process.env[URL_ENV] = previous.url;
    if (previous.token === undefined) delete process.env[TOKEN_ENV];
    else process.env[TOKEN_ENV] = previous.token;
    await new Promise((resolve) => server.close(resolve));
  }
}

test('四个工具真的跑得起来，用的是用时现取的工作区注册表', async () => {
  await withStubBridge(async () => {
    const { apply, tools, ctx, lookups, resolved, cleanup } = await loadAssembly();
    try {
      apply(ctx);
      const byName = new Map(tools.map((tool) => [tool.name, tool]));
      assert.deepEqual(
        [...byName.keys()].sort(),
        ['memory_context', 'memory_propose', 'memory_read', 'memory_search'],
        '四个工具都要注册上'
      );
      const exec = { agent: { session: { id: 's1', header: { cwd: '/repo/x' } } } };
      assert.ok(!byName.get('memory_propose').parameters.body.required, 'structured experiences do not need a redundant body');

      const search = await byName.get('memory_search').execute({ query: '构建' }, exec);
      assert.equal(search.count, 1);
      assert.equal(search.hits[0].title, '提交前跑检查');

      const read = await byName.get('memory_read').execute({ id: 'm-1' }, exec);
      assert.equal(read.title, '提交前跑检查');
      assert.equal(read.use_count, 1);

      const proposed = await byName
        .get('memory_propose')
        .execute({ title: '标题', body: '正文', scope: 'general' }, exec);
      assert.equal(proposed.state, 'pending');
      const context = await byName.get('memory_context').execute({ kind: 'all', query: '构建' }, exec);
      assert.equal(context.enabled, true);
      for (const tool of tools) await assert.rejects(tool.execute({}, {}), /需要一个会话/);

      // **给模型看的那两段文字也要说中文**：工具输出原样拼 `scope` / `verification`，
      // 于是模型读到「（project · verified）」，而面板与注入块写的是「项目记忆 · 已验证」——
      // 模型可能把 `project` 原样讲给用户（2026-10-18 用户要求统一）。
      const searchText = byName.get('memory_search').output.render({ query: '构建' }, search)[0].text;
      assert.match(searchText, /通用偏好 · 已验证/, `搜索输出要用中文标签：${searchText}`);
      const readText = byName.get('memory_read').output.render({ id: 'm-1' }, read)[0].text;
      assert.match(readText, /通用偏好 · 已验证/, `读取输出要用中文标签：${readText}`);
      assert.ok(!/（general/.test(searchText + readText), '不许再把英文枚举拼给模型');

      // **病根那条**：注册表必须是调用时从 `ctx` 现取的（`ctx.get`）。写成任何作用域里的
      // 变量时，三个工具各自抛 `ReferenceError: workspaceRegistry is not defined`——
      // 而「没抛这个错」本身证不了它用上了注册表，所以这里连 `resolveByPath` 的实参一起钉。
      assert.ok(lookups.includes('workspaceRegistry'), '注册表要 ctx.get 现取');
      assert.deepEqual(resolved, ['/repo/x', '/repo/x', '/repo/x', '/repo/x'], '四个工具都要拿它解析工作区 id');
    } finally {
      cleanup();
    }
  });
});

test('三处标签同一套词：面板 / 注入块（Rust） / 工具输出（插件）', async () => {
  const { mod, cleanup } = await loadAssembly();
  try {
    // ① 插件侧（工具输出）的词表。
    assert.deepEqual(mod.SCOPE_LABELS, { general: '通用偏好', project: '项目记忆' });
    assert.deepEqual(mod.VERIFICATION_LABELS, {
      verified: '已验证',
      unverified: '未验证',
      stale: '已过期',
    });

    // ② 面板侧（Vue）：验证三档必须逐字相同；范围那边面板给通用偏好多一句限定，
    //    所以比的是**词本身**。
    const { scopeText, verificationText } = await import('../../../ui/src/memory/memory.js');
    for (const [raw, label] of Object.entries(mod.VERIFICATION_LABELS)) {
      assert.equal(verificationText(raw), label, `面板的 ${raw} 要与工具输出同词`);
    }
    assert.equal(scopeText({ scope: 'project' }), mod.SCOPE_LABELS.project);
    assert.ok(
      scopeText({ scope: 'general' }).startsWith(mod.SCOPE_LABELS.general),
      '面板的通用偏好以同一个词开头'
    );

    // ③ 注入块（Rust）：那两组字面量在 `query.rs` 里，读源码比对。三处跨语言跨进程，
    //    没有共享模块可放，这条判据就是「同一个词」的唯一保证。
    const rust = readFileSync(new URL('../../../src-tauri/src/memory/query.rs', import.meta.url), 'utf8');
    for (const label of Object.values(mod.SCOPE_LABELS)) {
      assert.ok(rust.includes(`"${label}"`), `注入块里要有「${label}」`);
    }
    for (const label of Object.values(mod.VERIFICATION_LABELS)) {
      assert.ok(rust.includes(`"${label}"`), `注入块里要有「${label}」`);
    }
    assert.ok(!rust.includes('"本项目"'), '注入块不许再写「本项目」——面板叫「项目记忆」');
  } finally {
    cleanup();
  }
});

test('注入那条路也真的跑得起来：agent/created 注册贡献，pre-step 原样返回别人的决定', async () => {
  await withStubBridge(async () => {
    const { apply, ctx, pluginHandlers, cleanup } = await loadAssembly();
    try {
      apply(ctx);
      const contributions = [];
      const preSteps = [];
      const agent = {
        session: { id: 's1', header: { cwd: '/repo/x' } },
        ctx: {
          systemPrompt: { context(spec) { contributions.push(spec); } },
          on(event, cb) { if (event === 'agent/pre-step') preSteps.push(cb); },
        },
      };

      const created = pluginHandlers.get('agent/created');
      assert.equal(typeof created, 'function', 'agent/created 要挂上');
      created({ agent });

      assert.equal(contributions.length, 1, '每个会话注册一条运行上下文贡献');
      assert.equal(contributions[0].name, 'xlink.memory', '贡献名要与接线行同值');
      assert.equal(contributions[0].interpolate, false, '记忆正文里的 {{…}} 不许被当成提示变量');

      assert.equal(preSteps.length, 1, 'pre-step 要挂在 agent 作用域上');
      const decision = { allowed: true };
      const returned = await preSteps[0](
        {
          agent,
          messages: [{ source: { kind: 'user' }, content: [{ type: 'text', text: '构建前先跑检查' }] }],
          turn: 1,
        },
        async () => decision
      );
      // **原样返回**：排序与拒绝归别的插件，记忆这条路只更新自己的缓存。
      assert.equal(returned, decision, 'pre-step 必须原样返回 next() 的决定');
      // 桩桥回了非空正文 → 贡献的 `text()` 这时应当能读出它。**缓存的是桥渲染好的那段
      // 正文**，不是命中标题：标题只用于面板与徽标（`note` 路由），别把两者混起来。
      assert.match(String(contributions[0].text()), /提交前跑检查/, '这一步取到的记忆要进缓存');
    } finally {
      cleanup();
    }
  });
});

test('交接只随自身变化追加，记忆与经验刷新不重发交接，换目录立即清空旧项目', async () => {
  let snapshot = { text: '记忆一', handoff_text: '交接一', experience_text: '\n经验一', generation: 1 };
  await withStubBridge(async () => {
    const { apply, ctx, pluginHandlers, tools, cleanup } = await loadAssembly();
    let cwd = '/repo/x';
    ctx.workingDirectory.get = () => cwd;
    try {
      apply(ctx);
      let contribution, preStep;
      const agent = { session: { id: 'handoff-session', header: { cwd } }, ctx: {
        systemPrompt: { context(spec) { contribution = spec; } },
        on(event, cb) { if (event === 'agent/pre-step') preStep = cb; },
      } };
      pluginHandlers.get('agent/created')({ agent });
      const step = (turn) => preStep({ agent, messages: [{ source: { kind: 'user' }, content: [{ type: 'text', text: '情境' }] }], turn }, async () => true);
      await step(1);
      assert.equal(contribution.text(), '交接一\n\n记忆一\n经验一');
      await step(2);
      assert.equal(contribution.text(), '交接一\n\n记忆一\n经验一', '没变化就不生成另一份快照');
      snapshot = { ...snapshot, text: '记忆二', generation: 2 };
      await step(3);
      assert.equal(contribution.text(), '记忆二\n经验一', '记忆刷新不重发交接');
      for (const [turn, text] of [[4, '交接二'], [5, '交接三']]) {
        snapshot = { ...snapshot, handoff_text: text, generation: turn };
        await step(turn);
        assert.equal(contribution.text(), text);
      }
      snapshot = { ...snapshot, handoff_text: '交接四', generation: 6 };
      await step(6);
      assert.ok(!contribution.text().includes('交接四'), '交接复用首次一次加库变化两次的次数闸');
      const tool = tools.find((tool) => tool.name === 'memory_context');
      const value = await tool.execute({ kind: 'handoff' }, { agent });
      assert.equal(value.text, '交接四', '显式工具读取不受自动注入预算限制');
      cwd = '/repo/y';
      snapshot = { text: '', handoff_text: '', experience_text: '', generation: 7 };
      await step(7);
      assert.equal(contribution.text(), '', '预算用完也不能把旧交接留给新项目');
    } finally { cleanup(); }
  }, () => snapshot);
});
