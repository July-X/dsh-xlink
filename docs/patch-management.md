# Patch（内置补丁 / 小插件）管理

桌面壳随发布携带的自研内核补丁与小插件的设计、开发流程与实现说明。
与社区插件（`docs/plugin-management.md`）的区别：补丁**随 dsh-xlink 发布包内置**（进入
app bundle 资源目录），由本仓库维护、签名打包交付，用户只需选择「应用 / 撤销」，
不存在第三方代码信任问题。

## 目标与原则

- **用户自主**：每个补丁默认不生效；用户在「设置 → 内核补丁」页决定是否应用到
  **当前激活的内核**，随时可一键撤销（还原补丁前的文件）。
- **可逆优先**：应用前先备份被覆盖的原文件，撤销时优先从备份还原；备份丢失时
  以内容哈希校验兜底，绝不盲目删除或覆盖未知内容。
- **文件级手术**：补丁 = 一组针对内核目录（`<data_dir>/kernels/<版本>/`）内相对路径
  的文件操作，支持两种模式：
  - `copy`：把随包携带的文件覆盖为内核内路径（通常是新增文件）；
  - `replace`：在目标文件中做精确字符串替换（`search` → `replacement`，全文替换），
    用于不改整体文件的小手术。
- **范围约束**：所有 `to` 路径必须是内核目录内的相对路径（拒绝绝对路径、`..`、
  符号链接穿透），保证补丁不可能写出内核目录之外。
- **状态持久化**：应用记录落在 `<data_dir>/patches/state.json`，跨 Shell 重启保留；
  状态按「补丁 × 内核版本」记录，切换活动内核后互不串扰。

## 数据布局

```text
src-tauri/resources/patches/<patch-id>/       # 随发布包内置的资源（tauri.conf.json resources）
  manifest.json                               # 补丁清单（schemaVersion 1）
  PATCH.md                                    # 出处、哈希链、修复内容与验证记录
  files/<目标包名>/…                           # copy 模式引用的文件，路径相对本目录；
                                              # 一个补丁覆盖多个包时按包名分目录，
                                              # 避免同名文件（如 index.js）互相冲突

<data_dir>/patches/                          # 运行时状态（data_dir = ~/.dsh/desktop[-dev]/）
  state.json                                 # 应用记录
  backups/<patch-id>/<内核版本>/<相对路径>     # 应用时备份的原文件
```

资源在构建时经 `bundle.resources` 的 Map 形式固定复制到
`<resource_dir>/patches/`；运行时解析顺序为 `resource_dir/patches` 与
`resource_dir/resources/patches` 两个候选，兼容新旧打包语义。

## 清单格式（manifest.json）

以首个真实补丁 `dsh-file-perf` 为例（覆盖 npm dist 既有文件，一个补丁两个目标包）：

```json
{
  "schemaVersion": 1,
  "patches": [
    {
      "id": "dsh-file-perf",
      "name": "dsh @ 引用性能修复（file-reference-local + session-reference）",
      "version": "1.1.0",
      "kind": "patch",
      "description": "修复 @ 补全的两个数据源。",
      "minKernelVersion": "0.1.1-rc.2",
      "maxKernelVersion": null,
      "files": [
        {
          "mode": "copy",
          "from": "files/dsh-file-reference-local/index.js",
          "to": "node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js",
          "expectSha256": "dcf5299bf9a1c8dd33bf7d099f8d7bdfd52d69e516ba90e97cda0cdf402e0a7d",
          "required": true
        },
        {
          "mode": "copy",
          "from": "files/dsh-session-reference/index.js",
          "to": "node_modules/@deepseek-ai/dsh-session-reference/lib/index.js",
          "expectSha256": "e67bda5c8ee2e39437b474a596bc7fa600dfc9eea6821ce0a5c3eb3003c4e106",
          "required": true
        }
      ]
    }
  ]
}
```

字段约定：

| 字段 | 说明 |
| --- | --- |
| `id` | 全局唯一、文件系统安全的标识（kebab-case）。 |
| `name` / `version` | 显示名称与补丁自身版本。 |
| `kind` | `patch`（内核补丁，改动内核既有文件）或 `plugin`（内置小插件，多为新增文件）。仅作展示。 |
| `minKernelVersion` / `maxKernelVersion` | 适用内核版本范围（含端点，`null` 表示不限）。比较复用 `version::cmp_versions`。 |
| `supersededSinceKernelVersion` | 从该内核版本起，补丁功能已被官方内核采纳；`null` 表示一直有效，非 `null` 时对当前激活版本 `>=` 此值的内核，设置页把卡片折叠为「已并入官方内核」并禁用应用按钮（已应用到旧内核的记录仍可正常撤销）。语义区别于 `maxKernelVersion`：后者直接拒绝在更高版本上**安装**；本字段仅表达「该版本后已不需要」，与 `minKernelVersion` / `maxKernelVersion` 共存。 |
| `files[]` | 文件操作序列。`mode` = `copy` 或 `replace`；`required` 默认 `true`，为 `false` 时目标不存在/未命中搜索串只跳过该文件并记录说明，不中断整个补丁。 |
| `expectSha256`（copy 模式） | 预期「原文件」SHA-256（64 位小写十六进制）。给出时目标已存在必须与该哈希一致才会被备份并覆盖——把补丁打在 npm dist 等既有文件上时的安全闸：内核升级 / 文件漂移会明确失败而不是覆盖未知文件。缺省时 copy 保持「新增文件」语义（目标已存在且内容不同则拒绝）。 |
| `replace` 模式 | 需要 `search`（精确字符串，不支持正则）与 `replacement`；全文中所有匹配都会替换。 |

校验规则（应用时执行，而非加载时）：`copy` 必须给出 `from` 且资源内为普通文件；
`replace` 必须同时给出 `search` 与 `replacement`；`to` 必须通过路径约束检查。

## 应用流程（patch_apply）

1. 校验前置：工作台必须已停止（端口空闲，规则与「切换内核版本」一致，避免写入
   运行中的内核目录）；必须存在已激活的内核版本；补丁版本范围必须覆盖该版本。
2. 逐文件处理：
   - `copy`（无 `expectSha256`，新增文件语义）：目标已存在且内容与源一致 → 先备份再记录（视为幂等）；内容不一致 →
     `required` 时中止并报错，否则跳过并记录说明；目标不存在 → 直接写入。
   - `copy`（带 `expectSha256`，覆盖既有文件语义，用于 npm dist 等真实文件）：目标必须与声明哈希一致才备份并覆盖；
     目标已是补丁后状态则不动（幂等）；不一致 / 缺失 → `required` 时中止并给出「内核版本可能已升级」的可操作提示，
     为 `false` 时跳过并记录说明。
   - `replace`：目标不存在或搜索串未命中 → `required` 时中止，否则跳过；命中 →
     先备份原文件，再原子写回替换后的内容。
   - 任何已有备份残留（上一次应用未正常撤销）都会中止并提示先撤销。
3. 每个被修改/新增的文件记录 `hadOriginal`、备份相对路径、修改后内容 SHA-256；应用记录同时保存
   `patchVersion`。旧版记录缺少该字段时按过期记录处理，先撤销后才能重新应用当前定义。
4. 应用记录原子写回 `state.json`（`process::atomic_write`）。

## 撤销流程（patch_revert）

1. 找到（补丁 × 当前激活内核）的应用记录；没有则报「未应用」。
2. 逐文件还原：
   - 备份存在 → 用备份内容原子覆盖目标，删除备份（还原成功）。
   - 备份丢失且目标缺失 → `hadOriginal=false` 时视为已还原（无需动作）；否则记录警告
     「原文件备份已丢失且目标文件不存在（内核可能已重装），无法恢复原文件」。
   - 备份丢失但目标存在 → 校验目标 SHA-256 是否等于记录的应用后哈希：
     - `hadOriginal=false`（纯新增文件）且哈希一致 → 删除该文件（安全撤销）；
     - `hadOriginal=true` 且哈希一致 → 中止并提示「原文件备份丢失（内核可能已重装），
       请重新安装该内核版本后重试」，绝不拿错误内容当原文件；
     - 哈希不一致 → 中止并提示「目标文件已被其他工具修改，请检查后手动处理」。
3. 移除应用记录；还原过程中产生的警告随命令结果返回，UI 逐一提示。

## 状态模型（patch_status）

每个补丁一行，状态按**当前激活内核**计算：

| 状态 | 含义 |
| --- | --- |
| `no_kernel` | 尚未安装/激活内核，无法应用。 |
| `incompatible` | 当前内核版本不在补丁的适用范围内。 |
| `not_applied` | 未应用（或已应用到其他版本，附注说明）。 |
| `applied` | 已应用，应用记录的 `patchVersion` 与当前清单一致，且磁盘文件与记录哈希一致。 |
| `partial` | 已应用但有文件被跳过（非必需文件未命中）。 |
| `dirty` | 记录存在但补丁版本不一致，或磁盘文件缺失/哈希不一致（内核可能被重装、补丁资源已更新或文件被手动修改），提示先撤销再重新应用。 |

## 已被官方内核取代的补丁（supersededSinceKernelVersion）

当官方内核的某个版本开始已经采纳了本补丁的修复，`PatchDef.supersededSinceKernelVersion`
设为该版本号。对当前激活版本 `>=` 此值的内核：

- 状态字段 `row.superseded` 为 `true`；`row.supersededSinceKernelVersion` 透传 manifest 声明；
- `not_applied` 时 `state_text` 变为「已并入官方内核」；`enabled` 强制为 `false`，
  Rust 端 `apply` 会以「补丁已被官方内核 v{bound} 及以上版本取代，无需手动应用」拒绝。
- 撤销不受影响：旧版本上已应用的 `applied` 记录仍可正常 `revert`，新激活内核上看
  不到这条记录（每个 kernel_version 独立）。
- 旧 alpha.1 / rc.2 等内核上的应用记录不会因为官方内核升级而自动撤销；切回旧内核时
  仍按 `applied` 处理。

UI 表现（`ui/src/components/SettingsPanel.vue`）：

- 卡片整体降饱和（`opacity: 0.72`），标题加删除线（`text-decoration: line-through`），
  在标题行追加「已并入官方内核 v{bound} 起」徽标；
- 默认折叠（只显示标题 + 徽标 + 一行说明 + 「展开查看」按钮），不展示描述、
  适用范围、操作按钮；
- 用户点击「展开查看」可手动展开（`obsoleteExpanded` Set 维护展开态）查看完整卡片；
- 折叠态不展示「应用到当前内核」按钮，展开后该按钮也保持禁用（`row.enabled = false`）。

`dsh-file-perf` 是第一个使用此字段的真实补丁（`supersededSinceKernelVersion: 0.1.2-alpha.2`）：
官方 0.1.2-alpha.2 已经采纳了它的全部核心优化，alpha.2 上不需要再手动打补丁，
但 `expectSha256` 仍以原 0.1.1-rc.2 dist 哈希做安全闸——目标文件与声明哈希不一致时
拒绝应用并提示「内核版本可能已升级」。

## 开发流程与计划

新增/修改一个补丁的完整流程：

1. **设计**：在 `src-tauri/resources/patches/<id>/` 下建目录，写 `manifest.json`
   （含 `minKernelVersion` 等约束）与 `files/` 载荷；补丁目标必须是 `@deepseek-ai/dsh`
   官方包内稳定存在的路径，优先 `copy` 新增文件，避免对内核既有文件做脆弱手术。
2. **实现（初版，已完成）**：`patches.rs` 模块 + 三个 Tauri 命令
   （`patch_status` / `patch_apply` / `patch_revert`）+ 设置页「内核补丁」卡片。
3. **验证**：`cargo test patches` 覆盖应用/撤销/备份丢失/路径越界等场景；
   `cargo clippy --all-targets && cargo fmt`；`npm run build:ui`。
   补丁**载荷本身**的行为另写只读校验脚本（如 `scripts/verify-dsh-file-perf.mjs`）：
   既核对目标文件哈希是否为补丁后版本，也复跑该补丁特有的行为断言，
   要求未应用时失败、应用后全绿——否则脚本无法证明补丁真的生效。
4. **发布**：补丁资源随 app bundle 内置（tauri.conf.json 已配置 `resources`），
   无需改动 `.github/workflows/desktop-release.yml`；上架新补丁 = 改 `resources/patches/`
   并随版本发布。替换补丁文件（同 `id`）时必须递增补丁 `version`；已应用过旧版或旧状态
   记录的内核会呈现 `dirty`，由用户先撤销旧记录，再重新应用当前版本。

### 计划拆解

- [x] 初版机制：清单/状态/备份模型、应用/撤销/状态命令、设置页 UI、
      `expectSha256` 覆盖既有文件语义、单元测试。
- [x] 首个真实补丁：`dsh-file-perf`（dsh `@` 引用性能修复，含真实内核文件
      的端到端验证）。v1.1.0 起同时覆盖 `dsh-file-reference-local` 与
      `dsh-session-reference` 两个包。此前用于验证机制的示例补丁（`xlink-hello` /
      `xlink-stub-annotate`）已移除，机制能力由单元测试与 `dsh-file-perf` 覆盖。
- [x] 第二个真实补丁：`dsh-session-perf` v1.0.1（JSONL 持久层会话 header 枚举的短 TTL 缓存、并发扫描合并、生命周期失效和旧载荷状态识别）。
- [x] 第三个真实补丁：`dsh-escalation-same-mode` v1.0.0（`copy + expectSha256` 模式：在 `@deepseek-ai/dsh-sandbox/lib/index.js` 的 `approveEscalation` 顶部插入同模式短路，让模型在已处于 `danger-full-access` 等目标模式时仍可合法送入同模式 `sandbox_permissions`，不再被「not strictly wider」击穿；其它非更宽请求仍按原路径抛错；当前载荷只覆盖 `0.1.1-rc.2`，其它内核版本不能直接套用）。
- [ ] 二期：补丁版本升级（`update` 命令：备份旧应用记录 → 应用新版本，无需先撤销）；
      按内核版本的应用视图（切换内核后对每个已装版本单独管理）。
- [ ] 三期：补丁更新渠道（从发行版拉取最新补丁清单，脱离「随 app 版本捆绑」的节奏）；
      安装日志接入 `logs/` 日轮转规范。

## 首个内置补丁：dsh-file-perf

`src-tauri/resources/patches/dsh-file-perf/`（含 `PATCH.md` 出处与验证记录）：

- 目标（两个包，同一 `id`，应用/撤销整体生效）：
  - `node_modules/@deepseek-ai/dsh-file-reference-local/lib/index.js`
    （`@` 的文件/目录候选源）；
  - `node_modules/@deepseek-ai/dsh-session-reference/lib/index.js`
    （`@` 的历史会话候选源）。
  两者均来自 npm 包 `@0.1.1-rc.2`，MIT © 2026 DeepSeek。
- 内容：
  - file-reference-local — `@` 文件引用的索引失效、后台刷新、目录扫描和排序路径；
    同时保留 `@./` / `@.github/` 目录列举行为；
  - session-reference — 候选发现优先使用 workspace 头信息和 live session，完整列表在
    后台刷新；标题优先从内存投影或 `sessionProjectionCache` 读取；归档会话在标题读取前
    被过滤。服务未挂载或尚未初始化时软降级，不影响显式引用解析。
- 安全闸：两个文件各自的 `expectSha256` = 对应原始 dist 哈希
  (`dcf5299b…2e0a7d` / `e67bda5c…c4e106`)；内核升级后文件漂移会在应用时明确失败。
  补丁后哈希 `593e150a…bd60` / `a4bbe7c0…4a87`。
- 验证：机制级单元测试覆盖 expectSha256 匹配/不匹配/缺失三种路径；补丁行为用
  `node scripts/verify-dsh-file-perf.mjs <内核根目录>` 复跑，脚本只读检查两个目标文件
  的哈希并运行两份载荷的行为断言；另用真实内核原文件跑过临时端到端（应用 → 逐字节一致
  → 撤销 → 逐字节还原）。
- 注意：
  - 同批次发现的配置修复（`excludedDirectories` 16 项，写入
    `~/.dsh/profiles/web/cordis.patch.yml`）属用户侧配置，**不在**本补丁内；
  - 如果同一内核已经应用过旧版 `dsh-file-perf`，更新后的状态会显示 `dirty`；先撤销旧
    记录，再从设置页重新应用，补丁系统会用原备份恢复文件。
  - 本补丁改善两个候选源，但客户端仍会同时等待两者；真实工作区的呼出耗时还需要在
    应用后的目标内核上复测。

## 第二个内置补丁：dsh-session-perf

`src-tauri/resources/patches/dsh-session-perf/`（使用 `copy + expectSha256` 注入共享缓存载荷）：

- 目标：`node_modules/@deepseek-ai/dsh-session-persistence-jsonl/lib/index.js` 的
  `listArtifacts()`；该入口同时服务 host `session.list`、workspace 启动 header bootstrap、
  `session-reference` 后台刷新和 `listSnapshots()` 的 artifact 枚举。
- 根因：每个调用方都重新遍历 project/session 目录，并读取每个 Zstandard artifact 的首帧
  header。这个路径只读 header，但在启动期间会被多个服务重复触发。
- 行为：同一个 persistence 实例在 1000 ms 内复用 header/path 结果；相同 revision 的
  并发调用共享一个 in-flight 扫描；单个项目内的 header 探测最多 16 路并发且保留目录顺序；
  `session/created` 和 `session/disposed` 立即清除缓存；
  带 `AbortSignal` 的调用只取消自己的等待，不会中断其它调用共享的扫描。返回值为浅拷贝，
  不把调用方对数组或 header 的修改写回缓存。
- 边界：不缓存完整会话日志，不改变 `session.inspect()`、`readFrom()` 或
  `session.history` 的解压、校验、分页和错误语义。外部进程直接改写 `~/.dsh/sessions` 时，
  最多有一个 TTL 的最终一致性窗口。
- 来源：目标文件来自 npm `@deepseek-ai/dsh-session-persistence-jsonl@0.1.1-rc.2`，
  原始 SHA-256 为 `8b6ebc45…8d97f3`，补丁后 SHA-256 为 `9ed3fe3c…0a7f96e`。载荷按
  包名保存在 `files/dsh-session-persistence-jsonl/index.js`，manifest 的版本范围和
  `expectSha256` 只允许覆盖已核验的原始文件；补丁系统仍按文件级备份、原子写入和可撤销规则处理。
- 验证：`npm run test:session-perf` 在临时模块树中检查清单、语法、并发合并、TTL 命中、
  事件失效、调用方拷贝、缺失 artifact / 损坏 header 的 fail-soft、失败重试和 abort 语义；应用到当前内核后执行
  `node scripts/verify-dsh-session-perf.mjs --require-applied` 检查目标状态，再用真实
  `session.list` 和选中历史会话的 `session.history` 分开复测。当前载荷只声明适用于
  `0.1.1-rc.2`，其它内核版本不能直接套用。

## 第三个内置补丁：dsh-escalation-same-mode

`src-tauri/resources/patches/dsh-escalation-same-mode/`（使用 `copy + expectSha256` 模式，
载荷整文件保存在 `files/dsh-sandbox/index.js`）：

- 目标：`node_modules/@deepseek-ai/dsh-sandbox/lib/index.js` 的 `approveEscalation`。
  `WIDER_MODES` 严格更宽表里没有 `danger-full-access` 条目，导致
  `effectiveMode === "danger-full-access"` 且模型送入
  `sandbox_permissions: "danger-full-access"` 时被抛
  `sandbox escalation to "danger-full-access" is not strictly wider than this call's current "danger-full-access" mode`；
  `workspace-write` 同模式也有同样问题。
- 行为：补丁在 `const { requestedMode, effectiveMode, ... } = request;` 与
  `if (!(WIDER_MODES[effectiveMode] ?? []).includes(mode)) throw ...` 之间插入
  `if (mode === effectiveMode) return mode;`——同模式请求变成 no-op，不再走严格更宽表、
  也不再触发审批。其他分支完全保持原状：严格更宽的请求仍走审批流，非更宽但模式不同的
  请求仍按原路径抛错，无 `approver` / 无 `agent` 的失败消息文案也不变。
- 来源：目标文件来自 npm `@deepseek-ai/dsh-sandbox@0.1.1-rc.2`，原始 SHA-256 为
  `63ee2a10…e324f`，补丁后 SHA-256 为 `dafc42d2…22c5e`。清单只允许覆盖声明的原始
  `expectSha256`；目标 SHA 不符时拒绝写入。载荷整文件保留在 `files/dsh-sandbox/index.js`。
- 验证：`npm run test:escalation-same-mode` 在临时模块树里检查清单、目标 / 载荷 SHA、
  载荷的同模式短路存在性、`approveEscalation` 的同模式 / 缩小 / 缺失 approver / 缺失 agent /
  审批取消等行为断言。应用到当前内核后追加 `--require-applied` 复测。当前载荷只覆盖
  `0.1.1-rc.2`，其它内核版本不能直接套用。

## 已知限制（初版）

- 应用/撤销只针对**当前激活内核**；切换版本后需要重新应用（状态互不覆盖）。
- `replace` 是整文件读改写：二进制文件或非 UTF-8 文件会被拒绝。
- 内核重新安装（同版本重装或 pnpm 重写文件）后，已应用补丁表现为 `dirty`，
  需用户重新应用或撤销——机制上保证不会静默丢失或误删文件。
