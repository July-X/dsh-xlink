// 测试/探针代码**不得**以用户真实数据目录下的浏览器 profile 起 WebView2。
//
// 事故（2026-10-20 official-chat 首站探测）：探针用
//   --profile "<家目录>\.dsh-xlink\dsh\desktop-dev\webview-official-chat"
// 起 WebView2，浏览器在**用户真实数据目录**里连续写了两分钟——实测
// `EBWebView\Default\Network\Cookies` 与 `Preferences` 的 mtime 都是 12:46:53，
// 另有 Local Storage / History / IndexedDB / Secure Preferences / Favicons。
// Cookies 是活文件，**写坏了回滚不了**；而「测试永远不许碰用户的真实数据目录」
// 是 AGENTS.md「开发规则」里的红线——口头的「我会注意」管不住，必须能机检。
//
// 判据抓两类写法：
//   ① 出现产品 profile 目录名 `webview-official-chat`——那是官网对话窗口真实的
//      user-data 目录，测试一旦指向它，就是在写用户的浏览器档案；
//   ② 同一行里同时出现「真实家目录标记」与「浏览器 profile 标记」
//      （`.dsh-xlink` + `webview-` / `EBWebView` / `--user-data-dir` /
//      `data_directory`）——路径是拼出来的也算。
//
// **只读路径解析放行**：`join(HOME, '.dsh-xlink', mode, 'desktop', 'active.txt')`
// 是读内核安装树的位置，不是浏览器 profile（memory 插件的探针早就这么干，
// 见 `plugins/memory/test/probe.test.mjs` 里同名的那条纪律）。
//
// **生产代码不在扫描范围**：壳自己的官网对话窗口本来就用那个 profile
// （`src-tauri/src/commands.rs` 里的 `state.data_dir.join("webview-official-chat")`）。
//
// 逃生阀：确有必要引用真实路径（例如断言「我们没有碰它」）时，
//   · 该行写 `invariant-allow: real-data-dir`  → 放行这一行；
//   · 某一行**只有** `invariant-allow: real-data-dir` → 放行整个文件。
// 放行必须留痕，不许匿名。
//
// **为什么扫描范围不含 `scripts/`**：规则本体与它的自测**必然**含有被禁字面量
// （那正是规则的定义与夹具）。实测把 `scripts/` 纳入范围会立刻得到 14 条命中，
// 全部来自本文件、`scripts/probe-profile-isolation.test.mjs` 与
// `scripts/check-invariants.mjs` 里的规则说明——**规则会开始抓自己**。
// 真要在 `scripts/` 里放探针，请另起一个 `scripts/probe/` 目录并把它单独纳入范围，
// 别把规则的地盘也扫进来。

/** 真实家目录标记。Windows 上是 `C:\Users\<name>\.dsh-xlink`，也可能写 `~/.dsh-xlink`。 */
export const REAL_HOME_MARKER = /\.dsh-xlink/;

/** 浏览器 profile 标记。 */
export const PROFILE_MARKER =
  /webview-|EBWebView|--user-data-dir|user_data_dir|data_directory|dataDirectory/;

/** 产品官网对话窗口的真实 profile 目录名。 */
export const PRODUCT_PROFILE_NAME = 'webview-official-chat';

/** 显式放行标记：写在行尾放行这一行；独占一行放行整个文件。 */
export const ALLOW_MARKER = 'invariant-allow: real-data-dir';

/**
 * 剥掉一行的注释前缀（`//` / `#` / `/*` / `*`），只剩正文。
 * 放行标记写在注释里是常态，所以判定「这一行只有放行标记」时要先剥前缀。
 */
function stripCommentPrefix(line) {
  return String(line).trim().replace(/^(?:\/\/+|#+|\/\*+|\*+)\s*/, '').trim();
}

/**
 * 在一段文本里找出「指向用户真实数据目录下的浏览器 profile」的行。
 *
 * @param {string} text 文件内容
 * @param {string} file 用于报错的相对路径
 * @returns {{file: string, line: number, text: string}[]} 违规行（行号从 1 起）
 */
export function findProfileViolations(text, file = '<text>') {
  const lines = String(text).split('\n');
  // 独占一行的放行标记 = 放行整个文件。
  if (lines.some((line) => stripCommentPrefix(line) === ALLOW_MARKER)) return [];
  const out = [];
  lines.forEach((line, index) => {
    if (line.includes(ALLOW_MARKER)) return;
    const isProductProfile = line.includes(PRODUCT_PROFILE_NAME);
    const mixesRealHomeAndProfile =
      REAL_HOME_MARKER.test(line) && PROFILE_MARKER.test(line);
    if (!isProductProfile && !mixesRealHomeAndProfile) return;
    out.push({ file, line: index + 1, text: line.trim() });
  });
  return out;
}

/**
 * 在一批文件里找违规行。
 *
 * @param {{file: string, text: string}[]} files
 * @returns {{file: string, line: number, text: string}[]}
 */
export function findProfileViolationsInFiles(files) {
  return files.flatMap((entry) => findProfileViolations(entry.text, entry.file));
}
