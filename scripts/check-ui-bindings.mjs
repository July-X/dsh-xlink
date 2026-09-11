// 校验所有 SFC 模板里的标识符都能解析到真实绑定。
//
// 背景：模板里写了一个既不在 `<script setup>` 绑定、也没有全局注册的标识符时，
// Vue 只会把它求值成 `undefined`——`:disabled="globalBusy"` 会静默变成"永不
// 禁用"，`:loading` 变成"永不转圈"，而 272 个 Rust 测试与全部 UI 测试都不会红，
// 生产构建也不报错（dev 构建才有一条控制台警告）。
//
// 判据：用 `@vue/compiler-sfc` 逐文件 `parse` → `compileScript`（取其
// `bindingMetadata`）→ `compileTemplate`。命中 setup 绑定的标识符会编译成
// `$setup.x` / 直接引用；**没有命中任何绑定的才会编译成 `_ctx.x`**。因此
// "渲染结果里出现 `_ctx.<name>`"就是"这个标识符无从解析"的可靠信号。
//
// 两个必须注意的点，否则这个方法本身会产生误报：
//   1. 必须把 `compileScript` 的 `bindingMetadata` 传给 `compileTemplate`。
//      不传时**所有**标识符都渲染成 `_ctx.*`，整个文件看起来全是未定义。
//   2. `$`/`_` 开头的名字是 Vue 实例内置（`$slots` / `$attrs` / `$event` …），
//      运行时由渲染上下文提供，不属于未定义。
//
// 只检查 `<script setup>` 的组件：选项式 API 的模板标识符来自 `this`，静态无法
// 枚举，跳过并汇报数量。
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const DEFAULT_ROOT = 'ui/src';

/** 解析 @vue/compiler-sfc：它是 vue 的依赖，可能被 pnpm 放在 .pnpm 下。 */
async function loadCompiler() {
  const candidates = [
    '@vue/compiler-sfc',
    'vue/compiler-sfc',
  ];
  for (const specifier of candidates) {
    try {
      return await import(specifier);
    } catch {
      // 试下一个
    }
  }
  // pnpm 的严格布局下顶层不一定能解析到，回退到 .pnpm 里的实体路径。
  const pnpmDir = 'node_modules/.pnpm';
  let entries = [];
  try {
    entries = readdirSync(pnpmDir).filter((name) => name.startsWith('@vue+compiler-sfc@'));
  } catch {
    entries = [];
  }
  for (const entry of entries) {
    const path = join(
      process.cwd(),
      pnpmDir,
      entry,
      'node_modules/@vue/compiler-sfc/dist/compiler-sfc.cjs.js',
    );
    try {
      if (statSync(path).isFile()) {
        return await import(path);
      }
    } catch {
      // 试下一个
    }
  }
  throw new Error(
    '无法解析 @vue/compiler-sfc。请先在仓库根目录执行 `npm run deps`（或 pnpm install）后重试',
  );
}

function walkVueFiles(dir, out = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === 'node_modules' || entry.name === 'dist') continue;
      walkVueFiles(path, out);
    } else if (entry.name.endsWith('.vue')) {
      out.push(path);
    }
  }
  return out;
}

/** 从渲染产物里抽出未解析到任何绑定的标识符。 */
export function unresolvedIdentifiers(renderCode) {
  const found = new Set();
  const pattern = /_ctx\.([A-Za-z_$][\w$]*)/g;
  let match;
  while ((match = pattern.exec(renderCode)) !== null) {
    const name = match[1];
    if (name.startsWith('$') || name.startsWith('_')) continue; // Vue 实例内置
    found.add(name);
  }
  return [...found];
}

export async function checkProject(root = DEFAULT_ROOT) {
  const compiler = await loadCompiler();
  const failures = [];
  let checked = 0;
  let skipped = 0;
  for (const file of walkVueFiles(root)) {
    const source = readFileSync(file, 'utf8');
    const { descriptor, errors } = compiler.parse(source, { filename: file });
    if (errors.length > 0) {
      failures.push({ file, names: ['<SFC 解析失败>'], detail: errors[0].message });
      continue;
    }
    if (!descriptor.scriptSetup || !descriptor.template) {
      skipped += 1;
      continue;
    }
    const script = compiler.compileScript(descriptor, { id: file });
    const bindings = script.bindings || {};
    const compiled = compiler.compileTemplate({
      source: descriptor.template.content,
      filename: file,
      id: file,
      // 必须传：否则所有标识符都会渲染成 _ctx.*（见文件头注释）。
      compilerOptions: { bindingMetadata: bindings, prefixIdentifiers: true },
    });
    if (compiled.errors && compiled.errors.length > 0) {
      failures.push({
        file,
        names: ['<模板编译失败>'],
        detail: String(compiled.errors[0].message || compiled.errors[0]),
      });
      continue;
    }
    checked += 1;
    const unresolved = unresolvedIdentifiers(compiled.code);
    if (unresolved.length > 0) {
      failures.push({ file, names: unresolved, detail: '' });
    }
  }
  return { checked, skipped, failures };
}

async function main() {
  const root = process.argv[2] || DEFAULT_ROOT;
  const { checked, skipped, failures } = await checkProject(root);
  if (failures.length === 0) {
    console.log(
      `✓ UI 模板绑定检查通过：${checked} 个 <script setup> 组件（跳过 ${skipped} 个非 setup / 无模板文件）`,
    );
    return;
  }
  for (const failure of failures) {
    const suffix = failure.detail ? `（${failure.detail}）` : '';
    console.error(
      `✗ ${relative(process.cwd(), failure.file)}：模板引用了未定义的标识符 ${failure.names.join('、')}${suffix}`,
    );
  }
  console.error(
    `\n共 ${failures.length} 个文件未通过。请从 '../loading.js' 等模块导入这些名字，` +
      '或确认它们已在 app.config.globalProperties 上注册；生产构建不会报错，' +
      '只会把绑定静默求值成 undefined',
  );
  process.exitCode = 1;
}

// 入口守卫用 `pathToFileURL`：手拼 `file://${argv[1]}` 在 Windows 上永不相等
// （`D:\a\…` vs `file:///D:/a/…`），脚本会静默变成一个什么都不做的成功步骤
// ——正是 P1-5 在 check-signing-keys.mjs 里修掉的那种坑。
const invokedPath = process.argv[1] && pathToFileURL(resolve(process.argv[1])).href;
if (invokedPath === import.meta.url) {
  await main();
}
