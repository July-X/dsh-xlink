# dsh-file-perf：dsh 文件引用性能修复

dsh-xlink 收录的第一个真实内核补丁。修改 `@deepseek-ai/dsh-file-reference-local`
（`WorkspaceFileSearch`，内核 `@` 文件引用数据源）的 dist 产物。

## 来源与哈希链

- 原始文件：`node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js`
  （npm 包 `@deepseek-ai/dsh-file-reference-local@0.1.1-rc.2`，MIT © 2026 DeepSeek）
  - SHA-256：`dcf5299bf9a1c8dd33bf7d099f8d7bdfd52d69e516ba90e97cda0cdf402e0a7d`
- 补丁后文件（本目录 `files/index.js`）
  - SHA-256：`207d0e6f7a05cc58625a8d37a2bc8914461721fb330fa8115e56a3b23d32855b`
- manifest 的 `expectSha256` 即原始文件哈希：仅当内核里目标文件与它一致时才
  应用，内核升级 / 文件漂移时安全失败并给出可操作提示。

## 修复内容（4 处）

1. `invalidate()` 只置 `stale = true`，不再 abort 在途查询（`dispose()` 保留
   真 abort）——边跑工具边打 `@` 菜单不再闪空；
2. `ensureIndex()` 增加 stale-while-revalidate：脏了先返回旧索引、后台重扫——
   工具返回后首个 `@` 从 ~200 ms 降到毫秒级；
3. `scanWorkspace()` 由逐目录串行 `await` 改为层序 + 16 并发 `readdir`；
4. 索引期预计算 `lower/base/hidden`，查询期零 `split` / 零 `toLowerCase`；
   `rankCandidates` 用有界 top-K 插入替代「全量数组 + `sort` + `slice`」——
   大仓库每键 15–43 ms 降到 0.05–3 ms。

回归说明：原实现的隐藏文件规则只作用于全局模糊查询；目录列举分支用
`entry.name.startsWith('.') && !fragment.startsWith('.')`。补丁用 `filterHidden`
参数区分两个分支，避免 `@./`、`@.github/` 被误过滤（曾在该补丁开发中踩到）。

## 行为验证（补丁开发期）

- `/tmp/dshbench/verify.mjs`：3 个 root × 2 组排除 × 20 类 query =
  **120/120 组查询结果逐字节一致**（排序、评分、隐藏规则、目录列举与原始实现相同）。
- 性能（A/B，本机）：冷启 205→2 ms、每键 17.7→0.05 ms、tool 后首查 204→0 ms；
  大型真实代码库每键 15.2→3.0 ms、tool 后首查 171→4 ms。

## 已知坑

- 补丁打在 npm dist 上：内核版本升级（重装 node_modules）会覆盖它，状态面板
  会呈现「文件已被改动（dirty）」，此时可「撤销」（丢弃旧记录）或重新应用。
- 本补丁只覆盖 `dsh-file-reference-local` 的代码缺陷。同批次发现的配置修复
  （`excludedDirectories` 排除 `target/dist/.venv` 等 16 个构建目录）写在
  `~/.dsh/profiles/web/cordis.patch.yml`，属用户侧配置，不在本补丁内。
- 配套的会话引用侧性能问题（`sessionReferenceResolver/candidates` 全量加载
  会话日志）不在本补丁范围，需另立补丁。