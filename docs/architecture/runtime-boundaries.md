# 运行时边界与共享实现

本文承接根 `AGENTS.md` 的内核、取源和共享服务细节。总体模块关系见[当前架构](architecture.md)，路径与隔离见[磁盘布局](kernel-data-layout.md)。

## 内核归属

内核由用户从 npm registry 安装，壳相当于代用户执行 `dsh web`，不携带或重新发布内核代码。已安装的内核目录保持只读：安装 stub（`package.json` / `pnpm-workspace.yaml`，含 `overrides`）统一由 `kernel_deps.rs` 写入，依赖树由 pnpm 生成，用户显式卸载时才删除整目录。备份、哈希校验或可逆性都不构成修改内核代码的例外。

内核缺陷按已知未解决处理：在壳侧减少触发、提供页面内一次性自愈和明确的 `cause` 归因，并登记缺口、上报维护者。不改 `deepseek-ai/deepseek-harness` 或任何 fork，不提上游 PR、不推分支，也不把未来上游修复写成实现或验收前提。工作台运行期间禁止装删本壳内核；官网对话通道使用内核 `connection` 服务的 exact fetch route，不能依赖对第三方插件不可用的 `rpc.handle`。

### 已移除的补丁机制

补丁机制于 2026-10-11 整体删除。`dsh-file-perf` 自内核 `0.1.2-alpha.2` 起已被采纳；[旧补丁设计](../features/extensions/patch-management.md)只作历史资料。

`plugins/patches.rs::applied_patch_ids` 保留只读兼容入口：老用户磁盘上的 `state.json` 仍需计入环境指纹，不能因删除实现而改变回退点基准。更新只覆盖与新增文件，不会清除用户机器上的旧补丁；已打补丁的用户升级后无法通过 UI 撤销，发布说明须明确提醒。事故见[复盘 §1–2](../reviews/agent-lessons.md)。

## 取源与信任边界

| 路径 | 已强制的边界 |
| --- | --- |
| 内核 | `kernel.rs::DSH_NPM_PACKAGE` 硬编码包名 |
| 托管 Node | `node_install.rs` 硬编码版本并校验 SHA-256 |
| 插件 / 技能 | npm 包名仅校验字符类，不限制命名空间；GitHub 来源不限制仓库归属 |

插件与技能共用 `pkg/` 取源层；不要假设该层已限制 `@deepseek-ai`。版本列表优先 npm registry（与内核安装走同一条候选链），GitHub Releases 只作回退。

npm 默认源为 `registry.rs::OFFICIAL_NPM_REGISTRY`（官方 `https://registry.npmjs.org/`）。`registry::npm_registry_candidates()` 返回有序候选：官方在前，`registry.npmmirror.com` 只在前者**传输层不可达**（连接失败 / DNS / 超时）时兜底；HTTP 错误与「某个版本不存在」不换源——那类问题另一个源不会有不同答案，换源只会把上游发布事故说成镜像延迟。显式设置 `DSH_NPM_REGISTRY` 时候选链只有它一个，既不改默认、也不再兜底。**内核安装与版本列表必须同源**：列表按候选链读到哪个 base，`install_version` 就用 `probe_registry_base` 探测到同一个 base 并通过 `run_with_progress_with_registry` 交给 pnpm（`npm_config_registry`）；否则会出现列表能列出、pnpm 在解析阶段失败的错位（2026-10-28：npmmirror 只同步了主包 `@deepseek-ai/dsh` 与部分子包，`dsh-fs` 等 4 个子包仍停在旧版本线）。镜像/源选择不改变完整性规则：`releases::verify_download_integrity` 对 tarball 逐字节校验，优先取 `dist.integrity` 中最强且支持的 sha512 / sha256；没有可用 SRI 才回退 `dist.shasum`（sha1），两者都没有则拒绝安装。摘要不符时删除下载并拒绝安装。**插件 / 技能安装的 pnpm 调用不做这一探测**：它们沿用首选源（官方，或 `DSH_NPM_REGISTRY` 指定的唯一源），只有壳自身的 registry 取源（版本列表、packument、降级查询）才会在传输层失败时换候选——插件安装没有「版本列表」这条需要保持一致的对照，不为每次安装多付一次探测。

### 已登记的缺口

2026-09-27 审查 M7–M10 登记：`DSH_NPM_REGISTRY` 接受 HTTP、`dist.tarball` 不校验 host，明文 registry 对手可同时伪造摘要；git 来源不校验摘要，却执行归档内的 `prepare` 脚本。它们是已知缺口，不是已有防护。修改相关逻辑时结合[审查记录](../reviews/code-review-2026-09-27.md)和当前实现复核。

## 共享实现入口

新增相同语义的逻辑时复用下列入口，保持职责边界：

| 职责 | 入口与调用约定 |
| --- | --- |
| 包取源 | `pkg/`：插件与技能共用，只返回纯文本原因，由调用方分类错误 |
| JSON 状态 | `shell/state.rs`：容错读与校验读分开，文案由 `StateCtx` 提供 |
| 前端异步 | `ui/src/shell/async.js`：`singleFlight` / `createStatusSource` / `createUpdateChecker` |
| 安装沙盒 | `plugins/sandbox.rs`：启动临时内核、探测与回收，技能预检直接复用 |
| 安装事务 | `plugins/precheck.rs`：快照、沙盒安装、探测、提交或回滚；不写进 `center.rs`，后者只暴露 `store_file` 可见性入口 |
| 摘要十六进制 | `shell::digest::hex(&digest[..])`，文件哈希使用分块 `read` + `hasher.update` |

摘要写法须兼容 sha2 0.10 / 0.11；不依赖 `finalize()` 的 `LowerHex` 或 `Sha256: io::Write`，也不为兼容改动擅自抬高 main 的 sha2 版本。背景见[复盘 §25](../reviews/agent-lessons.md)。
