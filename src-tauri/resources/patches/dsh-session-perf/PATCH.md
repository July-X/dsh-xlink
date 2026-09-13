# dsh-session-perf v1.3.0：dsh 历史会话列表加载提速（锚定官方 0.1.5-rc.2）

这个补丁优化 `session.list` 以及所有复用持久层 artifact 列表的调用方。它覆盖
`@deepseek-ai/dsh-session-persistence-jsonl` 的 `listArtifacts()`，将短时间内重复的
header 扫描合并为一次共享读取，并把结果缓存 1 秒。

v1.3.0 锚定 `@deepseek-ai/dsh-session-persistence-jsonl@0.1.5-rc.2` 重新收录：官方
`0.1.5-alpha.2`、`0.1.5-rc.1`、`0.1.5-rc.2` 的 `lib/index.js` 逐字节相同（npm registry
与本机已安装内核实测，SHA-256 均为 `7d0640c9…`）。缓存机制与 v1.2.0 完全一致，但目标
内核线整体上移：官方在 `0.1.2-alpha.4` 起重写了该模块（`listArtifacts` 由
「反压缩探测 + `parseHeaderMeta`」改为「generation 解析 + `readGenerationHeader`」），
因此 v1.2.0 在 0.1.5 线上一律命中 `expectSha256` 安全闸而无法应用。

## 状态：官方未采纳枚举缓存，本补丁继续收录

官方 0.1.5 线重构了会话读取（generation 解析、格式目录、迁移校验），但在
`listArtifacts` 这一层仍然没有缓存：

- 本机实测官方 `0.1.5-rc.2`：并发两次 `list()` 加一次串行 `list()`，
  `listProjectDirs()` 被调用 **3 次**（即每次调用都完整重走目录遍历与逐会话 header 读取，
  无 TTL、无共享 in-flight）；同一脚本对补丁载荷的测量结果是 **1 次**。
- `session.list`、`session-reference.listCandidates`、workspace 启动 init 仍各自触发
  全量枚举。

因此 v1.3.0 在 0.1.5 线上正常提供「应用/撤销」：`minKernelVersion: 0.1.5-alpha.2`、
`maxKernelVersion: 0.1.5-rc.2`（已实测一致的最宽范围）。范围之外的内核在设置页显示
「不适用当前内核」，不会再出现「显示可应用、点下去必然失败」的状态。

## 目标

```text
node_modules/@deepseek-ai/dsh-session-persistence-jsonl/lib/index.js
```

这个入口同时被以下路径使用：

- `dsh-host-apiproxy` 的 `session.list`；
- `dsh-workspace` 启动时的 workspace header bootstrap；
- `dsh-session-reference` 的后台候选列表刷新。

补丁只缓存 artifact 的 `header` 和物理 `path`。原有的 header-only 语义不变，不会因为
缓存 `listArtifacts()` 而解压完整的会话日志；`session.inspect()` 与 `session.history`
不在补丁范围内。

## 实现

- 每个 `JsonlSessionPersistence` 实例使用独立的 `WeakMap` 缓存，避免不同 root 或
  compression 配置互相污染；
- 相同 revision 的并发调用共享一个 in-flight 扫描，避免 workspace、session-reference
  和首个 `session.list` 同时启动多次目录遍历；
- 已完成结果只保留 1000 ms，外部进程对会话目录的变化最多延迟一个 TTL；
- `session/created` 与 `session/disposed` 事件立即使缓存失效；
- 调用方拿到 header/path 的浅拷贝，避免调用方修改缓存内部数组或对象；
- 带 `AbortSignal` 的调用可以取消自己的等待，但不会取消其它调用正在共享的扫描；
- 扫描失败不会写入缓存，下一次调用会重新尝试；
- 单个项目的 header 探测最多 16 路并发（`SESSION_ARTIFACT_LIST_SCAN_CONCURRENCY`），
  但保留目录顺序，确保 duplicate id 检测与最终结果顺序与原始串行实现一致；
- **保持上游的 fail-soft 契约**：`resolveGenerationInDirectory()` 判定为「该目录没有可用
  generation」（返回 `undefined`）时跳过；`readGenerationHeader()` 抛出
  `SessionFormatUnsupportedError`（不支持的既有格式）时同样跳过；其它错误照旧向上抛。
  并发化只改变「哪些目录已经被探测过」，不改变按目录顺序裁决出的结果与错误。

## 来源与安全闸

载荷基于 npm 包 `@deepseek-ai/dsh-session-persistence-jsonl@0.1.5-rc.2`（官方
`0.1.5-alpha.2` / `0.1.5-rc.1` / `0.1.5-rc.2` 的 `lib/index.js` 逐字节相同），
MIT © 2026 DeepSeek。当前目标文件原始 SHA-256：

```text
7d0640c9fc4be6c703b77605fdee6af519c542fae28a6cd4489353309812f062
```

根据当前原始 dist 文件加入缓存实现生成的补丁后 SHA-256（v1.3.0）：

```text
89f0ad6567e791c9a8bf3bd293fe2a7650835bfd146a4adc9b1fcc5c5cedb14a
```

这是补丁版本 `1.3.0`，是一个带 `expectSha256` 的 `copy` 补丁。载荷保存在
`files/dsh-session-persistence-jsonl/index.js`，manifest 的 `expectSha256` 与
`0.1.5-rc.2` dist 一致；`minKernelVersion` 为 `0.1.5-alpha.2`、`maxKernelVersion` 为
`0.1.5-rc.2`。内核版本或 dist 内容漂移时会明确失败，不会覆盖未知文件。补丁系统在写入前
备份原文件，并以原子写入方式落盘。应用前必须关闭工作台。

## 验证

补丁载荷的静态、语法和行为验证：

```sh
node scripts/verify-dsh-session-perf.mjs <内核根目录>
```

```sh
node scripts/verify-dsh-session-perf.mjs --require-applied
```

验证结果应显示目标文件为补丁后哈希，并通过行为断言（并发合并、TTL 命中、事件失效、
调用方拷贝、缺失 artifact / 损坏 header / 不支持格式的 fail-soft、失败重试、
并发探测 16 路并发上限、目录顺序稳定、调用方 abort 只取消自身等待、所有等待者退出后
才取消共享扫描、重复会话 id 仍然报错）。`verify-dsh-session-perf.mjs` 通过临时目录加载
`cordis`、`dsh-session-persistence` 等依赖，不修改激活内核。

## 预期收益

- workspace 初始化、session-reference 后台刷新和首个 `session.list` 发生在 1 秒窗口内时，
  只保留一次 header 目录扫描；
- 多个重连/重复列表请求在缓存有效期内不再重复打开并解码每个日志；
- 单次冷扫描耗时不会因补丁变成零，首次调用仍需读取现有会话 header；
- 选中历史会话后的完整 `session.history` 解压不由本补丁优化。

## 已知限制

- TTL 是针对外部文件变化的最终一致性边界，不是跨进程实时索引；
- 仅有 `session/created`、`session/disposed` 会主动失效；正常会话事件不会改变 header，
  因此不触发无意义的重复目录扫描；
- 内核重新安装或 dist 文件漂移后，补丁状态会变为 `dirty`，应通过设置页撤销旧记录或
  重新应用，不要直接覆盖内核文件；
- **0.1.2 线与 0.1.3-alpha.2 内核不再适用本补丁**：v1.2.0 及其更早载荷锚定的是
  `0.1.2-alpha.2` / `0.1.2-alpha.3`（原始 SHA-256 `d5ae2c7d…`，补丁后 `29d2501e…`），
  官方从 `0.1.2-alpha.4` 起重写了目标文件，这些内核线已经不可能再应用任何版本的载荷。
  它们上面已存在的应用记录仍可按备份正常撤销（撤销走 `state.json` 记录，不受
  `minKernelVersion` / `maxKernelVersion` 影响）——`verify-dsh-session-perf.mjs`
  对这类内核只跳过目标文件状态校验，不会误报成「补丁坏了」。
- 16 路并发探测对小数据集（<20 个 session）收益有限，但防止大项目首次扫描成为瓶颈。
