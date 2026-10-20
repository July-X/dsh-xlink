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
// 判据抓三类写法：
//   ① 出现产品 profile 目录名 `webview-official-chat`——那是官网对话窗口真实的
//      user-data 目录，测试一旦指向它，就是在写用户的浏览器档案；
//   ② 同一行里同时出现「真实家目录标记」与「浏览器 profile 标记」
//      （`.dsh-xlink` + `webview-` / `EBWebView` / `--user-data-dir` /
//      `data_directory`）——路径是拼出来的也算；
//   ③ **建 webview 却没有无条件给 `data_directory`**（见下方
//      `findDataDirectoryViolations`）——这是 2026-10-20 13:48 那次写入的
//      **真正缺口**：① ② 扫的是「写死的路径字符串」，而那次事故的写法是
//      **压根不写路径**——`profile: Option<&Path>` 传 `None` 时不设
//      `data_directory`，WebView2 于是用**默认**数据目录，也就是
//      `%LOCALAPPDATA%\com.zhongxingxing.dsh-xlink\EBWebView`。
//      判据抓不到「用默认路径」，因为源码里没有路径可抓。
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
// **判据 ③ 复用同一个标记**（2026-10-20）：行尾标记放行**那个建窗点**，
// 独占一行的标记放行**整个文件**。语义 = 「这个建窗点/文件**明确允许**
// 使用默认数据目录」= **明确承认这次要写用户真实安装目录**，是**最后手段**。
// 两条判据**各自独立**判定：第 30 条放行 ≠ 第 31 条放行，反之亦然。
// （实测依据：在 WebView2 上「不设 `data_directory`」**恒等于**「用默认目录」
// = 用户真实安装目录，所以判据 ③ 的每一条命中都是「这次会写用户的活 Cookies」，
// 没有「确实不需要目录」的合法例外——逃生阀就是为这种**明确承认**准备的。）
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

// ---------------------------------------------------------------------------
// 判据 ③：建 webview 必须**无条件**给 data_directory
// ---------------------------------------------------------------------------
//
// 为什么要有这一条：判据 ① ② 抓的是「写死的真实路径」，而 2026-10-20 13:48
// 那次事故**没有任何写死的路径**——探针把 profile 传成 `Option<&Path>`，
// `None` 时**不设** `data_directory`，WebView2 就用**默认**数据目录
// （`%LOCALAPPDATA%\com.zhongxingxing.dsh-xlink\EBWebView`），浏览器一启动
// 就往用户的真实安装目录里写 Cookies / Preferences / Local Storage /
// History / IndexedDB。源码里没有路径字面量，所以 ① ② 抓不到。
//
// 判定口径（按**代码形状**，不按路径字符串）：
//   1. 找到「建 webview」的那一行（`WebviewWindowBuilder::new` /
//      `WebviewBuilder::new`）；
//   2. 在**它所在的那一层块**里（同一个函数体，不含后面的函数）找
//      `.data_directory(`；
//   3. 一次都没有 → 违规「压根没给」；
//   4. 有，但**每一处都在条件块里**（`if` / `if let` / `match` / `while` /
//      `loop` / `for` 内部）→ 违规「有才给」：条件不成立时同样落回默认目录。
//
// 「有才给」怎么才算补齐（**分支链**视角）：
//   · `if / else` **每个分支都**给了 → 每条路径都有目录 → 放行；
//   · `if / else if / else` 每个分支都给了 → 放行；
//   · `if` 没有**最终** `else` → 还有「条件全不成立」那条隐式路径没给 → 违规；
//   · 建窗点本身就在条件块里（那条路径压根不建窗）→ 不算漏 → 放行。
//
// **嵌套条件同样算「有才给」**（2026-10-20 修的真漏判）：分支块里「有
// `data_directory`」≠「这个分支给了目录」——命中点自己可能躺在**更深的
// 条件**里：
//
// ```rust
// if a {
//     if b2 { b = b.data_directory(p); }   // 命中在这个更深的条件里
// } else {
//     b = b.data_directory(q);
// }
// ```
//
// `a` 成立、`b2` 不成立时**没给目录** → 照样落回默认目录。判定必须**递归**：
// 一个分支「被完整覆盖」= 它内部要么有**无条件位置**的命中（命中点不在任何
// **更深的**条件里），要么它自己的更深分支链**全覆盖**（链穷尽 + 链上每个
// 分支都被完整覆盖）。
//
// 为什么第 4 条要单独算：写法 `if let Some(p) = profile { b = b.data_directory(p) }`
// 源码里**看着**给了 profile，实际是「有才给」——`None` 那条路径照样写用户的
// 真实目录。这正是 13:48 那次的写法。

/** 建 webview 的构造点。 */
const WEBVIEW_BUILDER_RE = /\bWebview(?:Window)?Builder::\s*new\b/;

/** 设置 user-data 目录的调用。 */
const DATA_DIRECTORY_RE = /\.data_directory\s*\(/;

/** 条件块的关键字。 */
const CONDITIONAL_HEAD_RE = /\b(?:if|if\s+let|match|while|loop|for)\b/;

/**
 * 剥掉一行里的注释与字符串字面量，只留**代码形状**。
 *
 * 为什么必须剥：判据看的是「这段代码在不在条件块里」，字符串里的 `{`
 * 与注释里的 `if` 都会算错。Rust 的生命周期 `'a` 不是字符字面量，
 * 所以单引号只认 `'\x'` / `'\\'` 这种真正的字符字面量形态。
 */
function stripCodeShape(line) {
  const src = String(line);
  let out = '';
  let inString = false;
  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    if (inString) {
      if (ch === '\\') {
        out += '  ';
        i += 2;
        continue;
      }
      if (ch === '"') {
        inString = false;
        out += ' ';
        i += 1;
        continue;
      }
      out += ch === '"' ? ' ' : ' ';
      i += 1;
      continue;
    }
    if (ch === '"') {
      inString = true;
      out += ' ';
      i += 1;
      continue;
    }
    if (ch === '/' && src[i + 1] === '/') {
      out += ' '.repeat(src.length - i);
      break;
    }
    if (ch === "'" && /'(?:\\.|[^\\'])'/.test(src.slice(i, i + 4))) {
      out += ' '.repeat(4);
      i += 4;
      continue;
    }
    out += ch;
    i += 1;
  }
  return out;
}

/**
 * 逐行算花括号深度，并标出**条件块**的行范围与**分支链**。
 *
 * 分支链（chain）是 `if` / `else if` / `else` 这一组互斥分支：
 *   · 链上**每个分支都**给了 `data_directory` → 每条路径都有目录 → 合法；
 *   · 链上有分支没给 → 有路径落回默认目录 → 违规；
 *   · `if` 没有**最终** `else` → 存在「条件全不成立」那条隐式分支 → 违规
 *     （`else if` 不算最终 else：`if a {} else if b {}` 仍有第三条路径没给）。
 *
 * @param {string[]} codeLines 已剥掉注释与字符串的代码行
 * @returns {{depthBefore: number[], blocks: object[]}}
 */
function scanBlocks(codeLines) {
  const depthBefore = new Array(codeLines.length).fill(0);
  const blocks = [];
  const stack = [];
  let depth = 0;
  let lastPopped = null;
  codeLines.forEach((code, index) => {
    depthBefore[index] = depth;
    let statementStart = 0;
    for (let i = 0; i < code.length; i += 1) {
      const ch = code[i];
      if (ch === '{') {
        const head = code.slice(statementStart, i);
        // `} else {` / `} else if … {`：接在刚弹出的那个条件块后面。
        const isElse = /^\s*else\b/.test(head);
        // 「最终 else」：else 后面直接就是 `{`（`else if` 不算）。
        const isFinalElse = /^\s*else\s*$/.test(head);
        const block = {
          id: blocks.length,
          start: index,
          end: index,
          // 父块 = 栈顶。`} else {` 会先弹出上一分支，此时栈顶仍是**同一层**的
          // 父块——所以 else 分支与它的 if 分支是**兄弟**（两者的行范围可能
          // 重叠，父子关系只能靠 parent 链判，不能靠行范围）。
          parent: stack.length > 0 ? stack[stack.length - 1].id : null,
          // else 分支也是**条件**分支：它只在「条件不成立」时走。
          conditional: CONDITIONAL_HEAD_RE.test(head) || isElse,
          chain: null,
        };
        if (isElse && lastPopped) {
          // else / else if 属于上一条分支链。
          block.chain = lastPopped.chain;
          if (block.chain && isFinalElse) block.chain.closed = true;
        } else if (block.conditional) {
          // if / if let / match / while / loop / for：开一条新的分支链。
          block.chain = { members: [], closed: false };
        }
        if (block.chain) block.chain.members.push(block.id);
        blocks.push(block);
        stack.push(block);
        depth += 1;
        statementStart = i + 1;
      } else if (ch === '}') {
        depth -= 1;
        const top = stack.pop();
        if (top) {
          top.end = index;
          lastPopped = top;
        }
        statementStart = i + 1;
      } else if (ch === ';') {
        statementStart = i + 1;
      }
    }
  });
  return { depthBefore, blocks };
}

/**
 * 找出「建 webview 却没有无条件给 data_directory」的行。
 *
 * 判定模型（**路径覆盖**视角）：浏览器只有在「真的建了窗、而那次建窗没给
 * user-data 目录」时才会落回默认目录。所以要问的是——
 * **存在一条「建了窗但没给目录」的路径吗？**
 *
 *   · 压根没有 `data_directory` → 有（窗建了、目录没给）→ 违规；
 *   · `data_directory` 只在条件分支里给，且**那个条件之外还会建窗** → 有 → 违规；
 *   · `data_directory` 无条件给 → 没有 → 放行；
 *   · `if / else` **两个分支都**给 → 每条路径都给了 → 放行；
 *   · 建窗点本身就在条件块里（那条路径压根不建窗）→ 不算 → 放行。
 *
 * **嵌套条件的判定（2026-10-20 修）**：一个分支块里「有 `data_directory`」
 * ≠「这个分支给了目录」——命中点自己可能躺在**更深的条件**里：
 *
 * ```rust
 * if a {
 *     if b2 { b = b.data_directory(p); }   // 命中在这个更深的条件里
 * } else {
 *     b = b.data_directory(q);
 * }
 * ```
 *
 * `a` 成立、`b2` 不成立时**没给目录** → 照样落回默认目录。所以判定必须递归：
 * 一个分支块「被完整覆盖」= 它内部要么有**无条件位置**的命中（命中点不在
 * 任何**更深的**条件里），要么它自己的更深分支链**全覆盖**（链穷尽 +
 * 链上每个分支都被完整覆盖）。
 *
 * **逃生阀（2026-10-20 修）**：第 31 条**自己**认 `ALLOW_MARKER`，两种形态
 * 与第 30 条一致——行尾标记放行**那个建窗点**，独占一行的标记放行**整个
 * 文件**。语义 = 「这个建窗点/文件**明确允许**使用默认数据目录」=
 * **明确承认这次要写用户真实安装目录**，是**最后手段**。
 * 两条判据**各自独立**判定：第 30 条放行 ≠ 第 31 条放行，反之亦然。
 *
 * @param {string} text 文件内容
 * @param {string} file 用于报错的相对路径
 * @returns {{file: string, line: number, text: string, reason: string, detail: string}[]}
 */
export function findDataDirectoryViolations(text, file = '<text>') {
  const lines = String(text).split('\n');
  // 独占一行的放行标记 = 放行整个文件。
  // 第 31 条**自己**判这个标记（不与第 30 条联动）：两条判据各自独立。
  if (lines.some((line) => stripCommentPrefix(line) === ALLOW_MARKER)) return [];
  const codeLines = lines.map(stripCodeShape);
  const { depthBefore, blocks } = scanBlocks(codeLines);
  const out = [];

  // 某一行落在哪些**条件块**里（全部，不只是最内层）。
  const conditionalBlocksContaining = (line) =>
    blocks.filter((b) => b.conditional && b.start <= line && line <= b.end);

  // 某一行落在哪个**条件块**里（取最内层）。
  const ownerOf = (line) =>
    conditionalBlocksContaining(line).sort((a, b) => b.start - a.start)[0] ?? null;

  // a 是 b 的祖先块或 b 本身（沿 **parent 链**判，不用行范围——同行的兄弟块
  // 行范围会重叠，按行范围判会把兄弟当成父子）。
  const isAncestorOrSelf = (a, b) => {
    if (!a || !b) return false;
    let cur = b;
    while (cur) {
      if (cur.id === a.id) return true;
      cur = cur.parent === null ? null : blocks[cur.parent];
    }
    return false;
  };

  // o 是否是 block 的**严格后代**（更深的条件）。
  const isDescendant = (block, o) => o.id !== block.id && isAncestorOrSelf(block, o);

  /**
   * 块 b 被进入之后，从块入口到块出口的**所有路径**是否都给了 data_directory。
   *
   * @param {number} blockId
   * @param {Set<number>} path 当前递归路径上的块（防环）
   */
  const covered = (blockId, path = new Set()) => {
    if (path.has(blockId)) return false;
    path.add(blockId);
    try {
      const block = blocks[blockId];
      // 1) 块内有没有**无条件位置**的命中：命中点所属的条件块里没有 b 的
      //    严格后代 → 命中不在更深的条件里（对本块而言就是无条件给的）。
      for (let k = block.start; k <= block.end; k += 1) {
        if (!DATA_DIRECTORY_RE.test(codeLines[k])) continue;
        const owners = conditionalBlocksContaining(k);
        if (!owners.some((o) => isDescendant(block, o))) return true;
      }
      // 2) 否则看本块的**直接子条件块**：按分支链分组，每条链都要**穷尽**
      //    （有最终 else，没有「条件全不成立」那条隐式路径）且链上每个分支
      //    都被完整覆盖 → 本块的所有路径才有目录。
      const childChains = new Map();
      for (const child of blocks) {
        if (child.parent !== blockId || !child.conditional || !child.chain) continue;
        if (!childChains.has(child.chain)) childChains.set(child.chain, []);
        childChains.get(child.chain).push(child);
      }
      if (childChains.size === 0) return false;
      for (const [chain] of childChains) {
        if (!chain.closed) return false;
        for (const memberId of chain.members) {
          if (!covered(memberId, path)) return false;
        }
      }
      return true;
    } finally {
      path.delete(blockId);
    }
  };

  // 一条分支链是否「每个分支都给了目录」：
  //   · 链有最终 else（没有隐式空分支）；
  //   · 链上每个分支块被**完整覆盖**（无条件位置，或它自己的更深链全覆盖）。
  const chainFullyCovered = (chain) =>
    chain.closed && chain.members.every((id) => covered(id));

  codeLines.forEach((code, index) => {
    if (!WEBVIEW_BUILDER_RE.test(code)) return;
    // 行尾放行标记 = 放行**这个建窗点**（语义：这个建窗点明确允许用默认
    // 数据目录 = 明确承认要写用户真实安装目录）。标记必须写在建窗那一行。
    if (lines[index].includes(ALLOW_MARKER)) return;
    // 作用域 = 建窗这行所在的**那一层块**：从这行到把这一层关掉为止
    // （后面的函数不算——否则别处的 data_directory 会把这条违规盖掉）。
    const base = depthBefore[index];
    let end = lines.length;
    for (let j = index + 1; j < lines.length; j += 1) {
      if (depthBefore[j] < base) {
        end = j;
        break;
      }
    }

    const hits = [];
    for (let k = index; k < end; k += 1) {
      if (!DATA_DIRECTORY_RE.test(codeLines[k])) continue;
      hits.push({ line: k + 1, owner: ownerOf(k) });
    }

    if (hits.length === 0) {
      out.push({
        file,
        line: index + 1,
        text: lines[index].trim(),
        reason: 'no-data-directory',
        detail: '建 webview 却没有给 data_directory——浏览器会用默认数据目录（用户真实安装目录）',
      });
      return;
    }

    // 无条件给 → 这次建窗一定有目录。
    if (hits.some((hit) => hit.owner === null)) return;

    const buildOwner = ownerOf(index);
    const coveredOk = (() => {
      if (buildOwner) {
        // 建窗点自己就在某个条件块里：那条条件成立时给的目录对这次建窗生效。
        // 命中点必须在**这一块或它的外层**（更深层的条件块只覆盖部分路径）。
        if (
          hits.some((hit) => hit.owner && isAncestorOrSelf(hit.owner, buildOwner))
        ) {
          return true;
        }
      }
      // 其余情况只能靠「分支链全覆盖」：链上每条路径都给了目录。
      const chains = new Map();
      for (const hit of hits) {
        if (hit.owner?.chain) chains.set(hit.owner.chain, hit.owner.chain);
      }
      for (const chain of chains.values()) {
        if (chainFullyCovered(chain)) return true;
      }
      return false;
    })();

    if (coveredOk) return;

    const condLines = hits
      .filter((hit) => hit.owner !== null)
      .map((hit) => hit.line)
      .join('、');
    out.push({
      file,
      line: index + 1,
      text: lines[index].trim(),
      reason: 'conditional-data-directory',
      detail:
        `data_directory 只在条件分支里给（${condLines} 行）——` +
        '条件不成立时浏览器照样用默认数据目录',
    });
  });

  return out;
}

/**
 * 在一批文件里找「建 webview 却没有无条件给 data_directory」的行。
 *
 * @param {{file: string, text: string}[]} files
 * @returns {{file: string, line: number, text: string, reason: string, detail: string}[]}
 */
export function findDataDirectoryViolationsInFiles(files) {
  return files.flatMap((entry) => findDataDirectoryViolations(entry.text, entry.file));
}
