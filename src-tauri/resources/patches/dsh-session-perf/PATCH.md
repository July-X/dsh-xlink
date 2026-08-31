# dsh-session-perf v1.0.1：dsh 历史会话列表加载提速

这个补丁优化 `session.list` 以及所有复用持久层 artifact 列表的调用方。它覆盖
`@deepseek-ai/dsh-session-persistence-jsonl` 的 `listArtifacts()`，将短时间内重复的
Zstandard header 扫描合并为一次共享读取，并把结果缓存 1 秒。

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

## 来源与安全闸

载荷来自 npm 包 `@deepseek-ai/dsh-session-persistence-jsonl@0.1.1-rc.2`，MIT © 2026
DeepSeek。当前目标文件原始 SHA-256：

```text
8b6ebc4509a3e969ab3ad6e0dfb553ae4861e5b101831afed23e593d148d97f3
```

根据当前原始 dist 文件加入缓存实现生成的补丁后 SHA-256：

```text
9ed3fe3cfa3890e8559efd9369efac9866c19c3737c3328b6355c338f0a7f96e
```

这是补丁版本 `1.0.1`，是一个带 `expectSha256` 的 `copy` 补丁。载荷保存在
`files/dsh-session-persistence-jsonl/index.js`，manifest 只允许覆盖上述原始 SHA-256 的目标；
版本范围当前精确限定为 `0.1.1-rc.2`，内核版本或 dist 内容漂移时会明确失败，不会覆盖未知文件。补丁系统在写入前备份原文件，
并以原子写入方式落盘。应用前必须关闭工作台。

## 验证

补丁载荷的静态、语法和行为验证：

```sh
npm run test:session-perf
```

验证脚本不修改激活内核。应用到当前内核后，验证目标文件：

```sh
node scripts/verify-dsh-session-perf.mjs --require-applied
```

验证结果应显示目标文件为补丁后哈希，并通过并发合并、TTL 命中、事件失效、调用方拷贝、
缺失 artifact / 损坏 header 的 fail-soft、失败重试和取消语义断言。用真实工作台复测时，再连续调用 `session.list`，并与 `workspace.list`
和选中大历史会话的 `session.history` 分别比较耗时。

## 预期收益

对当前约 133 个持久化会话的数据集：

- workspace 初始化、session-reference 后台刷新和首个 `session.list` 发生在 1 秒窗口内时，
  只保留一次 header 目录扫描；
- 多个重连/重复列表请求在缓存有效期内不再重复打开并解码每个日志的第一帧；
- 单次冷扫描耗时不会因补丁变成零，首次调用仍需读取现有会话 header；
- 选中历史会话后的完整 `session.history` 解压不由本补丁优化。

## 本机只读基准

使用当前 `~/.dsh/sessions`（133 个会话）运行原始模块和临时加载的补丁载荷，未应用补丁、未重启内核：

| 场景 | 原始 | 补丁载荷 |
| --- | ---: | ---: |
| 连续第 1 次 `list()` | 223.2 ms | 86.7 ms |
| 连续第 2 次 `list()` | 112.4 ms | 0.0 ms |
| 连续第 3 次 `list()` | 113.9 ms | 0.0 ms |
| 同实例 3 个并发 `list()` 总耗时 | 未测 | 82.3 ms，均返回 133 条 |

首扫耗时受 OS 文件缓存和当时进程负载影响，不能作为固定承诺；稳定收益是同一启动窗口
内的重复扫描被合并或直接命中缓存。

## 已知限制

- TTL 是针对外部文件变化的最终一致性边界，不是跨进程实时索引；
- 仅有 `session/created`、`session/disposed` 会主动失效。正常会话事件不会改变 header，
  因此不触发无意义的重复目录扫描；header/title 的展示仍由现有 live projection 和
  session projection cache 负责；
- 内核重新安装或 dist 文件漂移后，补丁状态会变为 `dirty`，应通过设置页撤销旧记录或
  重新应用，不要直接覆盖内核文件；
- 仅适用于已核验的 `0.1.1-rc.2` 内核；其它版本需要先重新检查源文件、依赖布局和数据一致性，
  再生成新的载荷哈希并调整版本范围。
