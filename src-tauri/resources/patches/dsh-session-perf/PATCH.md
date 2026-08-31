# dsh-session-perf v1.1.0：dsh 历史会话列表加载提速

这个补丁优化 `session.list` 以及所有复用持久层 artifact 列表的调用方。它覆盖
`@deepseek-ai/dsh-session-persistence-jsonl` 的 `listArtifacts()`，将短时间内重复的
Zstandard header 扫描合并为一次共享读取，并把结果缓存 1 秒。

v1.1.0 基于 `@deepseek-ai/dsh-session-persistence-jsonl@0.1.2-alpha.2` 重做：在 0.1.2-alpha.2
的新版 `listArtifacts` 上额外加了 16 路并发的 directory 探测（`mapSessionArtifactDirs`），
保留目录顺序；其它缓存 / TTL / 事件失效 / 共享 in-flight / 浅拷贝 / AbortSignal 边界与
v1.0.1 保持一致。

## 目标

```text
node_modules/@deepseek-ai/dsh-session-persistence-jsonl/lib/index.js
```

这个入口同时被以下路径使用：

- `dsh-host-apiproxy` 的 `session.list`；
- `dsh-workspace` 启动时的 workspace header bootstrap；
- `dsh-session-reference` 的后台候选列表刷新；
- `listSnapshots()` 的 header/artifact 枚举阶段。

补丁只缓存 artifact 的 `header` 和物理 `path`。原有的 header-only 语义不变，不会因为
缓存 `listArtifacts()` 而解压完整的会话日志；`session.inspect()`、`readFrom()` 和
`session.history` 不在补丁范围内。

## 实现

- 每个 `JsonlSessionPersistence` 实例使用独立的 `WeakMap` 缓存，避免不同 root 或
  compression 配置互相污染；
- 相同 revision 的并发调用共享一个 in-flight 扫描，避免 workspace、session-reference
  和首个 `session.list` 同时启动多次目录遍历；
- 已完成结果只保留 1000 ms，外部进程对 `~/.dsh/sessions` 的变化最多延迟一个 TTL；
- `session/created` 与 `session/disposed` 事件立即使缓存失效；
- 调用方拿到 header/path 的浅拷贝，避免调用方修改缓存内部数组或对象；
- 带 `AbortSignal` 的调用可以取消自己的等待，但不会取消其它调用正在共享的扫描；
- 扫描失败不会写入缓存，下一次调用会重新尝试。
- 单个项目的 header 探测最多 16 路并发（`SESSION_ARTIFACT_LIST_SCAN_CONCURRENCY`），
  但保留目录顺序，确保 duplicate id 检测与最终结果顺序与原始串行实现一致。

## 来源与安全闸

载荷基于 npm 包 `@deepseek-ai/dsh-session-persistence-jsonl@0.1.2-alpha.2`，MIT © 2026
DeepSeek。当前目标文件原始 SHA-256：

```text
d5ae2c7d6f6fbca6b2d4d8c6fc7ffb1342d4ed6484ec9cd309ee5c7bf88e9a00
```

根据当前原始 dist 文件加入缓存实现生成的补丁后 SHA-256：

```text
f9985512945738f32a29a6c34a3cda2e64ec1d051482a371c634e3fadaffb6ff
```

这是补丁版本 `1.1.0`，是一个带 `expectSha256` 的 `copy` 补丁。载荷保存在
`files/dsh-session-persistence-jsonl/index.js`，manifest 的 `expectSha256` 与 0.1.2-alpha.2
dist 一致；`minKernelVersion` 为 `0.1.1-rc.2`、`maxKernelVersion` 为 `null`，允许 0.1.2-alpha.2
直接应用。内核版本或 dist 内容漂移时会明确失败，不会覆盖未知文件。补丁系统在写入前备份原文件，
并以原子写入方式落盘。应用前必须关闭工作台。

## 验证

补丁载荷的静态、语法和行为验证：

```sh
node scripts/verify-dsh-session-perf.mjs <内核根目录>
```

```sh
node scripts/verify-dsh-session-perf.mjs --require-applied
```

验证结果应显示目标文件为补丁后哈希，并通过 28 项行为断言（并发合并、TTL 命中、事件失效、
调用方拷贝、缺失 artifact / 损坏 header 的 fail-soft、失败重试、并发探测 16 路并发上限、
目录顺序稳定、调用方 abort 只取消自身等待、所有等待者退出后才取消共享扫描等）。
`verify-dsh-session-perf.mjs` 通过临时目录加载 `cordis`、`dsh-session-persistence`
等依赖，不修改激活内核。

## 预期收益

对当前约 133 个持久化会话的数据集：

- workspace 初始化、session-reference 后台刷新和首个 `session.list` 发生在 1 秒窗口内时，
  只保留一次 header 目录扫描；
- 多个重连/重复列表请求在缓存有效期内不再重复打开并解码每个日志的第一帧；
- 单次冷扫描耗时不会因补丁变成零，首次调用仍需读取现有会话 header；
- 选中历史会话后的完整 `session.history` 解压不由本补丁优化。

## 已知限制

- TTL 是针对外部文件变化的最终一致性边界，不是跨进程实时索引；
- 仅有 `session/created`、`session/disposed` 会主动失效。正常会话事件不会改变 header，
  因此不触发无意义的重复目录扫描；header/title 的展示仍由现有 live projection 和
  session projection cache 负责；
- 内核重新安装或 dist 文件漂移后，补丁状态会变为 `dirty`，应通过设置页撤销旧记录或
  重新应用，不要直接覆盖内核文件；
- `expectSha256` 与 0.1.2-alpha.2 dist 一致；v1.0.1 之前的载荷（0.1.1-rc.2）已被
  v1.1.0 覆盖，verify 脚本会识别旧版应用记录并提示先撤销。
- 16 路并发探测对小数据集（<20 个 session）收益有限，但防止大项目首次扫描成为瓶颈。
