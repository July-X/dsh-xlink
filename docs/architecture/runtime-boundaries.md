# 运行时边界与共享实现

本文承接根 `AGENTS.md` 的内核、取源和共享服务细节。总体模块关系见[当前架构](architecture.md)，路径与隔离见[磁盘布局](kernel-data-layout.md)。

## 内核归属

内核由用户从 npm registry 安装，壳相当于代用户执行 `dsh web`，不携带或重新发布内核代码。已安装的内核目录保持只读：安装 stub（`package.json` / `pnpm-workspace.yaml`，含 `overrides`）统一由 `kernel_deps.rs` 写入，依赖树由 pnpm 生成，用户显式卸载时才删除整目录。备份、哈希校验或可逆性都不构成修改内核代码的例外。

内核缺陷按已知未解决处理：在壳侧减少触发、提供页面内一次性自愈和明确的 `cause` 归因，并登记缺口、上报维护者。不改 `deepseek-ai/deepseek-harness` 或任何 fork，不提上游 PR、不推分支，也不把未来上游修复写成实现或验收前提。每个工作台运行期间禁止装删其所属的本壳内核；另一内核的运行不应成为全局互斥条件。官网对话通道使用内核 `connection` 服务的 exact fetch route，不能依赖对第三方插件不可用的 `rpc.handle`。

### 已移除的补丁机制

补丁机制于 2026-10-11 整体删除。`dsh-file-perf` 自内核 `0.1.2-alpha.2` 起已被采纳；[旧补丁设计](../features/extensions/patch-management.md)只作历史资料。

`plugins/patches.rs::applied_patch_ids` 保留只读兼容入口：老用户磁盘上的 `state.json` 仍需计入环境指纹，不能因删除实现而改变回退点基准。更新只覆盖与新增文件，不会清除用户机器上的旧补丁；已打补丁的用户升级后无法通过 UI 撤销，发布说明须明确提醒。事故见[复盘 §1–2](../reviews/agent-lessons.md)。

## 取源与信任边界

| 路径 | 已强制的边界 |
| --- | --- |
| 内核 | DSH 使用 `kernel.rs::DSH_NPM_PACKAGE`；MiniMax 实验安装固定 `@minimax-webui/minimax-code@0.1.7` |
| 托管 Node | `node_install.rs` 硬编码版本并校验 SHA-256 |
| 插件 / 技能 | npm 包名仅校验字符类，不限制命名空间；GitHub 来源不限制仓库归属 |

插件与技能共用 `pkg/` 取源层；不要假设该层已限制 `@deepseek-ai`。版本列表优先 npm registry（与内核安装走同一条候选链），GitHub Releases 只作回退。**插件来源不互相隐式替代**：显式 GitHub URL / SSH URL / `owner/repo` 简写固定按 Git 处理（GitHub Release tarball 优先、clone 回退），不会按仓库名先去试同名 npm 包；npm 包名仍走 npm 路径（2026-11-01 OPT-01）。

npm 默认源为 `registry.rs::OFFICIAL_NPM_REGISTRY`（官方 `https://registry.npmjs.org/`）。`registry::npm_registry_candidates()` 返回有序候选：官方在前，`registry.npmmirror.com` 只在前者**传输层不可达**（连接失败 / DNS / 超时）时兜底；HTTP 错误与「某个版本不存在」不换源——那类问题另一个源不会有不同答案，换源只会把上游发布事故说成镜像延迟。显式设置 `DSH_NPM_REGISTRY` 时候选链只有它一个，既不改默认、也不再兜底。**取源 URL 策略（2026-11-01 OPT-02）**：显式 registry 只接受 HTTPS（带 userinfo / query / fragment 会在请求前拒绝）；packument 的 `dist.tarball` 与每次重定向都必须 HTTPS、无 userinfo，tarball 主机必须属于服务该 packument 的 registry 主机，或由 `DSH_NPM_TARBALL_HOSTS` 显式声明；重定向最多 5 跳。Git 来源只允许 HTTPS / SSH / git+ssh，`git://` 已拒绝。**内核安装与版本列表必须同源**：列表按候选链读到哪个 base，就写进按用途隔离的进程记录（内核 key 与插件 / profile 的 shell key 分开，记录 10 分钟后过期）；`install_version` 只复用内核 key 并通过 `run_with_progress_with_registry` 交给 pnpm（`npm_config_registry`），否则会出现列表能列出、pnpm 在解析阶段失败的错位（2026-10-28：npmmirror 只同步了主包 `@deepseek-ai/dsh` 与部分子包，`dsh-fs` 等 4 个子包仍停在旧版本线）。插件取源成功不会改写内核 key。镜像/源选择不改变完整性规则：`releases::verify_download_integrity` 对 tarball 逐字节校验，优先取 `dist.integrity` 中最强且支持的 sha512 / sha256；没有可用 SRI 才回退 `dist.shasum`（sha1），两者都没有则拒绝安装。摘要不符时删除下载并拒绝安装。**插件 / 技能 / profile 安装的 pnpm 调用使用 shell key**：有记录就复用，没有才探测一次；它们不读内核 key，也不为每次安装改写内核安装源。

MiniMax 的实验性受管安装复用相同的 npm 元数据、下载域和摘要校验；校验后的包在本壳独立暂存并通过 pnpm 安装，发布时不覆盖已有版本目录，发布前后检查包身份及原生依赖。**只用壳内受管安装**（2026-11-08 用户要求）：不再自动探测全局 npm / Node 前缀下的同包，因此「已安装」判定与「内核版本」页同口径，`configured_package_path` 之外的安装不参与启动。native 依赖的 install 脚本按包名显式放行（`--allow-build`），否则 pnpm 10 起会静默跳过、better-sqlite3 / node-pty 装出空壳。首轮固定组合、安装失败清理和 Windows / Intel macOS 实机结果以[接入计划](multi-kernel/minimax-webui-integration-plan.md)为准，代码存在不代表验收完成。

### 已登记的缺口

2026-09-27 审查 M7–M10 登记过的两个缺口已在 2026-11-01 OPT-02 收口：`DSH_NPM_REGISTRY` 只接受 HTTPS 并校验 URL 结构；tarball 下载域与逐跳重定向绑定到同一策略。**仍有意保留**：git 来源按 Git 语义不校验内容摘要，且会执行仓库内的 `prepare` 脚本（OPT-07 负责把这个信任边界讲清楚）；修改相关逻辑时结合[审查记录](../reviews/code-review-2026-09-27.md)和当前实现复核。

## 共享实现入口

新增相同语义的逻辑时复用下列入口，保持职责边界：

| 职责 | 入口与调用约定 |
| --- | --- |
| 包取源 | `pkg/`：插件与技能共用，只返回纯文本原因，由调用方分类错误 |
| JSON 状态 | `shell/state.rs`：容错读与校验读分开，文案由 `StateCtx` 提供 |
| 前端异步 | `ui/src/shell/async.js`：`singleFlight` / `createStatusSource` / `createUpdateChecker` |
| 安装沙盒 | `plugins/sandbox.rs`：启动临时内核、探测与回收，技能预检直接复用 |
| 安装预检与发布 | `plugins/precheck.rs` 管沙盒、计划与应用编排；`plugins/prepared.rs` 管物料缓存、可搬迁复制和原件备份；`plugins/center.rs` 的真实安装、发布与更新入口复用备份事务，负责中央库与实例接线 |
| 摘要十六进制 | `shell::digest::hex(&digest[..])`，文件哈希使用分块 `read` + `hasher.update` |

摘要写法须兼容 sha2 0.10 / 0.11；不依赖 `finalize()` 的 `LowerHex` 或 `Sha256: io::Write`，也不为兼容改动擅自抬高 main 的 sha2 版本。背景见[复盘 §25](../reviews/agent-lessons.md)。
