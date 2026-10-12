---
name: ponytail-debt
description: >
  把所有 `shortcut:` 注释（旧写法是 `ponytail:`）列成一份技术债台账。一次性报告，
  不改任何代码。用于「ponytail 欠的债」「ponytail 当时推迟了什么」「把标记出来的列一下」，
  或 /ponytail-debt。
---

每一个刻意留下的 ponytail 捷径都标着 `shortcut:` 注释，写明它的上限与升级路径
（老代码里可能写的是 `ponytail:`）。这条技能把它们收成一份台账，
让一次推迟不会悄悄地变成永久。

## 扫描

在仓库里 grep 注释标记，跳过 `node_modules`、`.git` 与构建产物：

`grep -rnE --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=dist --exclude-dir=build '(#|//|/[*]) ?(shortcut|ponytail):' .`（你的技术栈用别的注释前缀就补上）

用户指定了自己的标记词（`/ponytail-debt TODO`）就 grep 那个词。

每一处命中是台账的一行。注释前缀会把「只是提到这个约定」的散文挡在台账之外。
跳过不是推迟的命中，比如讲快捷键的说明。

## 输出

每个标记一行，按文件分组：

`<文件>:<行号>，<简化掉了什么>。上限：<注释里写的上限>。升级：<该回头看的触发条件>。`

约定是 `shortcut: <上限>, <升级路径>`，所以上限与触发条件直接从注释里取。
每行还要标负责人？加 `git blame -L<行号>,<行号>`。

把腐坏的风险标出来：任何没写升级路径或触发条件的标记注释都打上 `no-trigger` 标签，
它们就是会悄悄烂掉的那一批。

结尾写 `<N> 个标记，其中 <M> 个没有触发条件。`
什么都没找到：`没有捷径债。台账是干净的。`

## 边界

只读只报，不改任何东西。要把它固化下来，问过用户再把台账写进文件
（例如 `PONYTAIL-DEBT.md`）。一次性。说「stop ponytail-debt」或「normal mode」撤销。