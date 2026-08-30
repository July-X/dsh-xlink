# dsh-file-perf：dsh `@` 引用性能修复

dsh-xlink 收录的第一个真实内核补丁。它覆盖 `@` 补全的两个数据源，仍然只使用一个
补丁条目统一应用和撤销：

| 包 | 角色 | 主要改动 |
| --- | --- | --- |
| `@deepseek-ai/dsh-file-reference-local` | 文件和目录候选 | 缓存索引、目录扫描、查询排序和失效刷新 |
| `@deepseek-ai/dsh-session-reference` | 历史会话候选 | 复用会话头信息和标题投影，跳过归档会话 |

两个包都包含名为 `index.js` 的 dist 文件，载荷保留包目录层级，不把它们合并成一个
文件：

```text
files/
  dsh-file-reference-local/index.js  -> node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js
  dsh-session-reference/index.js     -> node_modules/@deepseek-ai/dsh-session-reference/lib/index.js
```

客户端 `dsh-client-ui-reference` 会同时等待这两个候选源，因此任一源的慢路径都可能
拖住整个 `@` 菜单。

## 来源与哈希链

| 文件 | SHA-256 |
| --- | --- |
| 原始 `dsh-file-reference-local/lib/index.js` | `dcf5299bf9a1c8dd33bf7d099f8d7bdfd52d69e516ba90e97cda0cdf402e0a7d` |
| 补丁后 `files/dsh-file-reference-local/index.js` | `593e150adb4f5db2c16fa76d9980f66ef4190f31c6b73e0b890b76e92dedbd60` |
| 原始 `dsh-session-reference/lib/index.js` | `e67bda5c8ee2e39437b474a596bc7fa600dfc9eea6821ce0a5c3eb3003c4e106` |
| 补丁后 `files/dsh-session-reference/index.js` | `a4bbe7c0010ccd004026a33b41787513bd28182a488339a4590a870a2a294a87` |

两者均取自 npm 包 `@0.1.1-rc.2`（MIT © 2026 DeepSeek）。manifest 的 `expectSha256`
即对应的原始文件哈希：仅当内核里目标文件与它一致时才应用，内核升级 / 文件漂移时
安全失败并给出可操作提示。

## 修复内容

### 一、file-reference-local

1. `invalidate()` 只标记索引过期，不再中止在途查询；`dispose()` 仍会真正中止资源；
2. `ensureIndex()` 先返回旧索引，再在后台扫描并发布新的 generation。刷新期间再次失效
   会保留过期状态，扫描失败也不会丢掉旧索引；
3. `scanWorkspace()` 改为按层级批量扫描，每批最多并发 16 个 `readdir`；
4. 索引阶段预计算 `lower`、`base`、`hidden`，查询阶段不再重复拆路径和转小写；
   `rankCandidates` 使用有界 top-K，避免每次查询创建完整排序数组。

回归说明：原实现的隐藏文件规则只作用于全局模糊查询；目录列举分支用
`entry.name.startsWith('.') && !fragment.startsWith('.')`。补丁用 `filterHidden`
参数区分两个分支，避免 `@./`、`@.github/` 被误过滤（曾在该补丁开发中踩到）。

### 二、session-reference

`listCandidates` 的候选路径改为优先使用已经建立的索引和投影：

- workspace 注册表的会话头信息与 live session 列表直接合并，避免每次按键都等待完整的
  `sessionQuery.listSessions()`；完整列表只在后台刷新，供下一次查询使用；
- 过滤在读取标题之前进行。workspace 已归档的会话不会进入候选，也不会触发标题读取；
- live session 优先读取内存中的 `title` 投影，冷会话读取 `sessionProjectionCache`，缓存
  不可用时退回 session id；
- 会话服务、workspace 服务或投影缓存未挂载时保持软降级，不影响 headless profile 和
  已写入 draft 的显式 `@session:<id>` 引用解析。

## 行为验证（补丁开发期）

- file-reference-local：3 个 root × 2 组排除 × 20 类 query =
  **120/120 组查询结果逐字节一致**，覆盖排序、评分、隐藏规则和目录列举。
- session-reference：使用真实的磁盘会话和归档 id，验证头信息快路径、归档过滤、投影
  缓存标题、显式 id 查询、软降级和取消行为。
- 复跑：`node scripts/verify-dsh-file-perf.mjs <内核根目录>`。脚本只读检查目标文件哈希，
  并运行两份载荷的行为断言。

## 性能（本机实测）

| 场景 | 改前 | 改后 |
| --- | ---: | ---: |
| 文件源冷启动 | 205 ms | 2 ms |
| 文件源每键 | 17.7 ms | 0.05 ms |
| 文件源「工具返回后首查」 | 204 ms | 0 ms |
| 大型代码库每键 | 15.2 ms | 3.0 ms |
| 会话源首次候选查询 | 等待完整列表和标题读取 | 有索引时先返回，完整列表后台刷新 |
| 会话源标题读取 | 读取持久化会话日志 | 优先使用内存投影或 `sessionProjectionCache` |

## 已知坑

- 本补丁改善的是候选源路径；整个 `@` 菜单仍由客户端同时等待文件源和会话源，真实耗时
  还会受 workspace 初始化、内核进程负载和当前数据量影响。应用后应在目标内核上复跑校验
  脚本，再用实际工作区体验呼出和连续输入。
- 补丁打在 npm dist 上：内核版本升级（重装 node_modules）会覆盖它，状态面板
  会呈现「文件已被改动（dirty）」，此时可「撤销」（丢弃旧记录）或重新应用。
- 同一个 `dsh-file-perf` 已应用旧载荷时，更新后的 manifest 会把旧文件显示为 `dirty`。
  请先点击「撤销补丁」还原旧备份，再点击「应用到当前内核」写入两个新载荷，不要手动
  删除备份或直接覆盖内核文件。
- 归档过滤会改变可见行为：归档后该会话在 `@` 里就搜不到了（需先在侧边栏取消归档）。
  两个文件在同一 `id` 下，应用/撤销是整体的，不能只留其一。
- 同批次发现的配置修复（`excludedDirectories` 排除 `target/dist/.venv` 等 16 个构建
  目录）写在 `~/.dsh/profiles/web/cordis.patch.yml`，属用户侧配置，**不在**本补丁内；
  没有它，文件源的候选仍会被构建产物淹没。
