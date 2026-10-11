// release 门禁：内嵌 WebDriver 插件绝不能进发布产物。
//
// ## 为什么需要一道门禁
//
// `tauri-plugin-wdio-webdriver` 在 app 进程内起一个 W3C WebDriver HTTP 服务
// （默认 4445、仅监听 127.0.0.1），能执行脚本、点击元素、读 cookie、截屏。
// 落进 release 产物等于给每位用户装了一个无鉴权的本机控制口。上游 crate 的
// README 是同一句警告："Never include it in production builds."
//
// ## 这道门禁是被一次真实的失败逼出来的
//
// 最初按上游 README 的建议写 `[target.'cfg(debug_assertions)'.dependencies]`。
// **那样写不生效**，cargo 会警告：
//
//   warning: Found `debug_assertions` in `target.'cfg(...)'.dependencies`.
//   This value is not supported for selecting dependencies and will not
//   work as expected.
//
// 后果是 Cargo.toml 长得完全正确、`lib.rs` 门控在位、`cargo check --release`
// 绿、全部单测绿，而 release 产物里**照样带着那个端口**。cargo 解析依赖只看
// 平台三元组与 `cfg(feature = ...)`，不看 `debug_assertions`。
//
// 教训写在这里而不是只写进提交信息：**这份判据早先还逐行核对「每个引用是否
// 被紧贴的 `#[cfg(debug_assertions)]` 覆盖」——形状全对，而事情照样是坏的。**
// 判据必须钉「算出来的结果」，不能钉「声明在哪」。
//
// ## 两层断言，且互为反向
//
// 只查 release 不够：一条「release 里没有它」的判据，在依赖被**整体**弄没了
// 的情况下照样是绿的——而那意味着调试能力悄悄消失，主壳又调不动了，却没任何
// 东西反对。所以同时断言 debug 侧**确实有**它。
//
// 1. **源码层**（默认执行，无需编译，进 `npm run check`）
//    - 依赖必须声明为 `optional = true`；
//    - 必须存在一个把它 `dep:` 进来的 feature，且该 feature **不在 `default`**；
//    - `src-tauri/src/lib.rs` 里的 `#[cfg(feature = "…")]` 必须与该 feature
//      同名。这条不是多余的：feature 名打错时，crate 会照常编译进二进制，
//      只是注册那行被 cfg 掉，**插件静默不启动且零报错**——debug 产物层查的
//      是「链接了谁」，查不出「注册了没有」，只有同名断言能拦。
//    - 不再逐引用核对门控的「紧贴」：依赖是 optional 的，release 不带 feature
//      时 crate 未声明，引用一旦失去门控**编译器直接报错**（`check:rust` 里
//      的 `cargo check --release` 已经在跑）。那属于编译器看得见的事，不该由
//      一个正则再猜一遍。
//
// 2. **产物层**（`--profile <debug|release>`）
//    直接读 `target/<profile>/dsh-xlink` 这份**将要发出去的二进制**，在 Buffer
//    里找插件的标记串。不读 cargo 指纹：指纹分不清 `cargo check` 与
//    `cargo build`（check 不重链二进制），一次违规构建之后紧跟一次干净的
//    check 就能骗绿它，而磁盘上仍是带端口的那份。标记串是字面量，扛得住
//    `[profile.release] strip = true`。

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];
const fail = (message) => failures.push(message);

const CRATE = 'tauri-plugin-wdio-webdriver';
const RUST_PATH = 'tauri_plugin_wdio_webdriver';
const RUST_ENTRY = join('src-tauri', 'src', 'lib.rs');

const cargoToml = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8');

// --- 1. 依赖必须是 optional，且声明在通用的 [dependencies] 表里 ----------------

// **必须连所在表一起看。** 本项目自己踩过：依赖一度落在
// `[target.'cfg(windows)'.dependencies]` 里，于是 macOS 上它根本不在依赖图内，
// `[features]` 里那句 `dep:tauri-plugin-wdio-webdriver` 悬空、cargo 不报错，
// 而「optional = true」与「feature 名字对得上」两条判据全都绿——直到
// `cargo check --features wdio` 才报 `cannot find module or crate`。
// 只看那一行不看它属于哪个表，就会漏掉整类问题。
let depSection = null;
let depLine = null;
let section = '(根)';
for (const line of cargoToml.split('\n')) {
  const header = line.match(/^\s*\[([^\]]+)\]\s*$/);
  if (header) {
    section = header[1].trim();
    continue;
  }
  if (!depLine && new RegExp(`^\\s*${CRATE}\\s*=`).test(line)) {
    depLine = line.trim();
    depSection = section;
  }
}

if (!depLine) {
  fail(`src-tauri/Cargo.toml 里没有 ${CRATE} 的依赖声明。主壳 WebUI 的调试入口靠它。`);
} else {
  if (depSection !== 'dependencies') {
    fail(
      `${CRATE} 声明在 [${depSection}]，必须在 [dependencies]。` +
        '声明在 [target....] 表里时，它只在匹配该 cfg 的平台上进依赖图：' +
        '其余平台上 `[features]` 那句 dep: 引用悬空、cargo 不报错，' +
        '而直到 `cargo check --features wdio` 才炸。'
    );
  }
  if (!/optional\s*=\s*true/.test(depLine)) {
    fail(
      `${CRATE} 必须声明为 optional（当前：${depLine}）。` +
        '非 optional 时它会无条件进入 release 产物。'
    );
  }
}

// --- 2. feature 存在，且不在 default 里 ---------------------------------------

const featuresBlock = cargoToml.match(/^\[features\]\s*$([\s\S]*?)(?=^\[|\Z)/m);
if (!featuresBlock) {
  fail('src-tauri/Cargo.toml 里找不到 [features] 表，无法核对 WebDriver 门控。');
} else {
  const body = featuresBlock[1];
  const featureName = [...body.matchAll(/^([A-Za-z0-9_-]+)\s*=\s*\[([^\]]*)\]/gm)].find(
    ([, name, values]) => values.includes(`dep:${CRATE}`),
  )?.[1];

  if (!featureName) {
    fail(
      `没有任何 feature 通过 dep:${CRATE} 把插件引进来。` +
        '可选依赖必须由某个 feature 启用，否则它永远不参与编译。'
    );
  } else {
    const defaultEntry = body.match(/^default\s*=\s*\[([^\]]*)\]/m);
    const defaults = (defaultEntry?.[1] ?? '')
      .split(',')
      .map((s) => s.trim().replace(/^["']|["']$/g, ''))
      .filter(Boolean);
    if (defaults.includes(featureName)) {
      fail(
        `feature \`${featureName}\` 出现在 \`default\` 里。` +
          '这会让 release 构建默认带上插件，本机 HTTP 自动化端口直接进产物。' +
          '`default` 必须是空的，dev 侧靠 `--features ' + featureName + '` 显式打开。'
      );
    }

    // --- 3. lib.rs 的门控 feature 名必须与之一致 -----------------------------

    const libSource = readFileSync(join(root, RUST_ENTRY), 'utf8');
    if (!libSource.includes(RUST_PATH)) {
      fail(
        `${RUST_ENTRY} 里没有 ${RUST_PATH} 的引用。插件没注册 = 主壳 WebUI 又调不动了，` +
          '而这不会有任何测试变红。'
      );
    }
    const gatedFeatures = [...libSource.matchAll(/#\[cfg\(feature\s*=\s*"([^"]+)"\)\]/g)].map(
      (m) => m[1],
    );
    if (!gatedFeatures.includes(featureName)) {
      fail(
        `${RUST_ENTRY} 里没有 \`#[cfg(feature = "${featureName}")]\`。` +
          (gatedFeatures.length
            ? `现有门控是：${gatedFeatures.join(' / ')}。`
            : '现有代码里没有任何 feature 门控。') +
          `feature 名与 Cargo.toml 的 \`${featureName}\` 不一致时，插件会静默不注册、零报错。`
      );
    }
  }
}

// --- 4. 产物层：这次构建真的链接了谁 ------------------------------------------

const profileArgIndex = process.argv.indexOf('--profile');
const profile = profileArgIndex === -1 ? null : process.argv[profileArgIndex + 1];

// 两条都在插件源码里，且都只可能由该 crate 带进二进制：
//   · 运行时日志字面量（server 启动时打印的那句）
//   · `panic::Location` 展开的 crate 源文件路径（不含版本号，跨升级稳定）
const MARKERS = ['WDIO WebDriver plugin initialized on port', 'tauri-plugin-wdio-webdriver'];
let verified = '';

if (profile && !['debug', 'release'].includes(profile)) {
  fail(`--profile 只接受 debug / release，收到：${profile}`);
} else if (profile) {
  const fingerprintRoot = join(root, 'src-tauri/target', profile, '.fingerprint');
  if (!existsSync(fingerprintRoot)) {
    // 静默跳过等于给出一道「通过」的假门禁（AGENTS.md 对预算脚本的同款要求）。
    fail(
      `找不到 ${fingerprintRoot}：产物层判据没有可读的对象，必须 exit 1。\n` +
        `  先跑：${
          profile === 'release'
            ? 'cargo build --release --manifest-path src-tauri/Cargo.toml'
            : 'cargo build --manifest-path src-tauri/Cargo.toml --features wdio'
        }`
    );
  } else {
    // ## 为什么读产物字节，而不是读 cargo 指纹
    //
    // 指纹（`target/<profile>/.fingerprint/dsh-xlink-*/….json`）里的 `deps`
    // 确实是「这次构建链接了谁」，但它**分不清 `cargo check` 与 `cargo build`**：
    // check 只出 `.rmeta`、不重链二进制，于是一次违规的
    // `cargo build --release --features wdio` 之后，紧跟着跑一次干净的
    // `cargo check --release` 就会产生更新的指纹，判据据此变绿——而
    // `target/release/` 里躺着的**仍然是那份带端口的二进制**。本项目实测踩过。
    //
    // 直接读二进制则没有这个歧义：它就是将要发出去的那份文件。
    //
    // ## 为什么 strip 之后仍然读得到
    //
    // `[profile.release] strip = true` 只剥符号表；标记串是**字面量**，住在
    // `.rodata` 里。插件的两条标记一条来自运行时日志（`WebDriver server
    // listening on http://`），一条来自 `panic::Location` 展开的 crate 源文件
    // 路径——两者都在 strip 之后原样保留（实测：release 二进制里命中 37 处）。
    //
    // 直接用 Buffer 在内存里找字节，不依赖 `strings` 这个外部命令（Windows
    // runner 上不一定有）。
    const binPath = ['dsh-xlink', 'dsh-xlink.exe']
      .map((n) => join(root, 'src-tauri/target', profile, n))
      .find((p) => existsSync(p));

    if (!binPath) {
      fail(
        `找不到 src-tauri/target/${profile}/ 下的产物二进制，判据没有可读的对象。\n` +
          `  先跑：${
            profile === 'release'
              ? 'cargo build --release --manifest-path src-tauri/Cargo.toml'
              : 'cargo build --manifest-path src-tauri/Cargo.toml --features wdio'
          }`
      );
    } else {
      const bytes = readFileSync(binPath);
      const found = MARKERS.filter((m) => bytes.includes(Buffer.from(m, 'latin1')));
      const rel = binPath.slice(root.length + 1);

      if (profile === 'release' && found.length) {
        fail(
          `${rel} 里含有内嵌 WebDriver 插件的标记串（命中 ${found.length}/${MARKERS.length}：` +
            `${found.map((m) => JSON.stringify(m)).join('、')}）。` +
            '这份二进制会在用户机器上开一个本机 HTTP 自动化端口，' +
            '发布前必须不带该 feature 重新构建。'
        );
      } else if (profile === 'debug' && found.length < MARKERS.length) {
        fail(
          `${rel} 里没有插件的全部标记串（只命中 ${found.length}/${MARKERS.length}）。` +
            '两条断言必须互为反向：只查「release 没有」的话，依赖被整体删掉时' +
            '那条判据照样是绿的，而主壳已经调不动了。\n' +
            '  先跑：cargo build --manifest-path src-tauri/Cargo.toml --features wdio'
        );
      } else {
        verified = `${profile} 产物层已核对（${rel}）`;
      }
    }
  }
}

// --- 结果 --------------------------------------------------------------------

if (failures.length) {
  console.error('');
  for (const message of failures) console.error(`✗ ${message}`);
  console.error(`\n${failures.length} 项 WebDriver 门禁检查失败。`);
  process.exit(1);
}
console.log(
  '✓ 内嵌 WebDriver 插件未进 release：依赖为 optional、feature 不在 default、' +
    'lib.rs 门控同名' +
    (profile ? `；${profile} 产物层已核对` : ''),
);