# 内核磁盘布局与生效链路

本文回答两个问题：**dsh-xlink 把内核相关的东西放在哪**，以及**这些文件怎么变成"正在跑的那个内核"**。
行末标「实测」的数字/内容取自 2026-10-13 本机 `~/.dsh-xlink`（release 壳，family = `dsh`）。

## 1. 三个根，别混

| 环境变量 | 作用域 | 默认值 | 备注 |
| --- | --- | --- | --- |
| `DSH_XLINK_HOME` | **壳**自己的数据根 | `~/.dsh-xlink` | `shell/paths.rs` 的 `xlink_home()`，全仓路径都从它展开；只解析、不创建目录 |
| `DSH_HOME` | **内核进程**的用户数据根 | 内核自己默认 `~/.dsh` | 壳启动实例时注入为**实例 home**；与 `DSH_XLINK_HOME` **不互相回退**（`paths.rs` 设计约束 1） |
| `DSH_DESKTOP_DATA_DIR` | 壳运行时元数据根 | `<xlink_home>/<family>/desktop[-dev]/` | `kernel::lifecycle::data_dir` 的完整覆盖（高级用法）。解析链只有两级，**没有 app-data 兜底** |

`kernel::data_dir(family)` 的 `family` 来自 `instance::default_family()`：本壳当前实例 → 本壳默认实例 → `dsh`。
它一级都不读注册表指针——`data_dir` 装的是内核安装树，让另一壳的写入影响它就是跨壳竞态（`check:invariants` 第 12 项）。

## 2. 完整目录树

```
<DSH_XLINK_HOME>/                                = ~/.dsh-xlink
├── shell/<release|dev>/                         壳自己的状态（两壳各一份）
│   ├── settings.json                            端口 / profile / current_instance_id / 自启偏好
│   ├── codex-usage-consent.json                 Codex 额度读取许可
│   └── logs/<mode>-<kind>-<name>-<date>.log     壳日志 + 内核 stdout/stderr（日志面板只扫这里）
├── dsh/desktop[-dev]/                           = kernel::data_dir("dsh")
│   ├── kernels/<version>/                       内核安装树（**当前唯一实际安装位**）
│   ├── active.txt                               本壳活动版本（一行版本号）实测: 0.2.1-alpha.2
│   ├── kernel.pid                               旧单实例启动路径的 pid（`pid port`）实测: 39000 3090
│   ├── quarantine.json                          启动看护隔离的插件记录
│   ├── disk-usage-cache.json                    磁盘面板的测量缓存
│   ├── webview-official-chat/                   「官方对话」窗口的 WebView2 user-data
│   └── tools/node/<version>/                    托管 Node 运行时（用系统 Node 时不存在）
├── kernels/<family>/instances/<id>/             实例目录（第 3 节展开）
├── kernels/<family>/versions/<version>/         P1 规划位，**当前不存在**
├── plugins/<family>[-dev]/                      插件中央库：<plugin-id>/ + store.json + .npmrc
├── skills/{packages,active}/                    技能中央库 / 活动视图（两壳**有意共享**）
├── state/instances[-dev].json                   实例注册表（按壳模式分文件）
├── backups/<id>/                                迁移 / 恢复前的备份
└── cache/                                       可重建的下载缓存
```

实测体积（MiB，**逻辑大小**；`profiles/` 与中央库里大部分文件是 pnpm store 的硬链接，实际增量见第 6 节）：

| 路径 | 体积 |
| --- | --- |
| `dsh/desktop/kernels/0.2.1-alpha.2` | 477.8 MiB / 27906 文件 |
| `dsh/desktop/kernels/0.2.1-alpha.1` | 476.4 MiB / 26742 文件 |
| `kernels/dsh/instances/default` | 435.0 MiB |
| `kernels/dsh/instances/default-dev` | 3.4 MiB |
| `plugins/dsh` + `plugins/dsh-dev` | 372.4 MiB + 463.0 MiB |

## 3. `kernels/<family>/instances/<id>/` 逐项

| 项 | 内容 | 谁写 | 说明 |
| --- | --- | --- | --- |
| `instance.json` | 实例执行记录：`id` / `kernel_family` / `kernel_version` / `profile` / `port` / `workspace` / `created_at_ms` / `label` | `resolve_instance_record` 每次启动前重写 | **启动的执行依据**。注册表只是列表，不是这里 |
| `home/` | 该实例的 `DSH_HOME`（内核的用户数据，见下） | 内核 + 壳接线 | 唯一被注入给内核进程的数据根 |
| `workspace/` | 内核进程 `cwd`（可被用户改成外部目录） | 壳建、内核用 | 内核写出的 cwd 派生路径从这里展开 |
| `runtime/pid` | `pid port mode` 三段的身份记录（实测 `39000 3090 release`） | `start_instance` | 旧格式（两段）仍可读；判活要过 `pid_is_kernel` 身份校验，pid 会被系统复用 |
| `runtime/status.json` | `InstanceRuntime`：status / pid / port / started_at_ms / failure_reason | start / stop | 给 UI 与 2.5s 状态轮询用，`port` 是"启动时实际绑定的端口"的副本 |
| `snapshots/state.json` | 环境回退点（装/卸/更新插件、切版本前打点） | `diagnostics::snapshot` | 按实例存——"上一个能跑的组合"是某一个实例的事实 |
| `usage/state.json` | 本地 token 账目 | `usage::local` | 跟着实例走，两个壳读同一份 |
| `diagnostics/runs/` | 启动诊断的每次运行记录 | `diagnostics::run` | 「上一次启动停在哪一步」按实例存 |
| `subscription-cache.json` | 云端套餐用量缓存 | `usage::subscription` | 按实例隔离：换实例（换账号）不能把上个账号的余额显示出来 |
| `extensions/plugins/<id>/` | 插件物化目录（`link:` 时为软链目标，`copy` 时为真实拷贝） | `plugins::center` 接线 | 按 `(family, instance_id)` 隔离；`extensions/wiring.json` **没有生产调用方**，见第 7 节 |

`home/` 内部（官方 DSH 的 home 约定 + 壳的接线产物）：

```
home/
├── profiles/<name>/                 内核的 profile 项目（实测只有 web）
│   ├── package.json                 dsh.profile.bundles（内核读）+ dependencies（pnpm 装）
│   ├── pnpm-workspace.yaml          nodeLinker: hoisted / minimumReleaseAge: 0
│   ├── cordis.yml                   空清单模板（改动请走 cordis.patch.yml）
│   ├── cordis.patch.yml             用户层 patch（热重载）
│   └── node_modules/                插件依赖闭包（大量 store 硬链接）
├── sessions/<工作区>/session-<uuid>/ 会话正文（附件在各自的 attachments/）
├── storages/                        工作区清单等（会话列表的来源是 workspace.json）
├── attachments/v1/                  会话附件
├── logs/                            内核自己的日志目录
├── cache/attachments/               附件缓存
├── extensions/builtin/<id>/         内置插件物化：<插件版本>/<内核兼容键>/{host,client,…} + state.json
├── plugins/builtin/<id>/            内置插件的数据区：upstream/<版本>/、peers/<包>@<版本>/、upstream.json
├── cordis.patch.yml                 ★ 壳管理的 loader 行（技能接线等）就追加在这里
├── .credentials.yaml                DSH 的凭据（凭据引用优先取 profile 的 apiKeyEnv）
└── .anonymous-user-id / .dsh-home-migrated / settings.yaml*
```

**`home/` 里哪两处是壳写的、其余别碰**：`profiles/<name>/package.json` 的两个键（`dependencies` 的托管 spec、`dsh.profile.bundles` 的托管行）与 `cordis.patch.yml` 的追加行。`settings.yaml*`、`.credentials.yaml`、`sessions/`、`storages/` 都是内核或用户的地盘。

## 4. 生效链路：从点「启动工作台」到内核跑起来

```
UI 点启动
  └─ kernel::lifecycle::start_instance(family, id, …)
       ① resolve_instance_record          ← 读 instance.json（缺失则按壳默认派生）
                                           然后用壳侧权威值覆盖：
                                             port / profile ← shell/<mode>/settings.json
                                             kernel_version ← <data_dir>/active.txt
                                           再写回 instance.json
       ② 端口检查                          ← port_open + instance_workbench_running（幂等）/ 报"端口被占用"
       ③ adapter.prepare_instance          ← 建实例目录骨架；profile_manifest::seed 写
                                             profiles/<profile>/{package.json,pnpm-workspace.yaml}；
                                             修/建 $DSH_HOME/cordis.patch.yml；写技能接线行
       ④ adapter.resolve_install_dir       ← 先试 kernels/<family>/versions/<version>/（不存在），
                                             再落 <data_dir>/kernels/<version>/（实际生效位）
       ⑤ plugins::center::ensure_wiring    ← 中央库 store.json → 物化到
                                             <instance>/extensions/plugins/<id>（link 或 copy）
                                             → 改写 profile 的 dependencies / dsh.profile.bundles
                                             → 清单变了或缺 node_modules 时跑 pnpm install
       ⑥ adapter.start                     ← node <install_root>/node_modules/@deepseek-ai/dsh/lib/bin.js
                                             web --no-open --port <record.port>
                                             cwd = <instance>/workspace
                                             env: DSH_HOME=<instance>/home、DSH_PROFILE=<profile>、
                                                  DSH_CUSTOM_SKILL_DIRS、CLAUDE_CONFIG_DIR、内置插件桥令牌…
                                            stdout/stderr → shell/<mode>/logs/<mode>-<family>-<instance>-kernel-<date>.log
                                                            （实测 release-dsh-default-kernel-2026-10-13.log）
       ⑦ shell::instance::write_pid        ← runtime/pid = "pid port mode"
```

内核最终加载什么，由**三份文件**共同决定，且优先级如上：

| 维度 | 权威来源（高 → 低） |
| --- | --- |
| 用哪个版本 | `active.txt`（壳级）→ `instance.json.kernel_version` |
| 用哪个端口 / profile | `shell/<mode>/settings.json` → `instance.json` |
| 加载哪些插件 | `$DSH_HOME/profiles/<profile>/package.json` 的 `dsh.profile.bundles` + 同目录 `node_modules/`，用户层再叠 `$DSH_HOME/cordis.patch.yml` |

一个容易误解的点：**bundle 名字不是全部从 profile 装的**。内核对 `dsh.profile.bundles` 里的每一项按「**内核安装树优先、profile 兜底**」解析（`@deepseek-ai/dsh-app-boot` 的 `resolveBundleDir`，注释写明这是 in-box bundle 永远来自运行中那份安装的契约）。所以 `@deepseek-ai/dsh-base` / `dsh-web-app` / 各 experimental bundle 在 profile 里一个字节都没有，只有**插件**（以及插件自己的 `dependencies`）才真的落在 profile 的 `node_modules/`。

## 5. 布局的三代与迁移关系

| 代 | 位置 | 现状 |
| --- | --- | --- |
| v0.2.x 平铺 | `<xlink_home>/desktop[-dev]/` | 启动期由 `resolve_family_runtime_dir` 同卷 `rename` 搬进 `<family>/desktop[-dev]/`；搬迁失败**继续用旧目录**（宁可平铺也不能让用户看到空的新目录） |
| 更早的官方默认 home | `~/.dsh/`（内核不注入 `DSH_HOME` 时的落点） | `instance::migrate_legacy_dsh_home_if_needed` 按清单**递归并入 release 实例**的 home（目标优先、`node_modules` 不动、schema 版本化可补跑）；清单外的壳旧目录原样保留 |
| P1 规划位 | `<xlink_home>/kernels/<family>/versions/<version>/` | **已定义、无写方**：只有 `DshAdapter::resolve_install_dir` 读它；实际安装仍落 `<family>/desktop[-dev]/kernels/<version>/`。排查内核问题不要去 `versions/` 找 |

`instance.json` 与 `state/instances[-dev].json` 的分工也属于同一类历史分层：注册表是**列表**（顶部页签、`is_default` 判定），`instance.json` 是**启动依据**；两者不一致时以"启动前 `resolve_instance_record` 重写后的 `instance.json`"为准。

## 6. 磁盘归属：哪些字节是真的

| 数据 | 真实占用 | 原因 |
| --- | --- | --- |
| `<data_dir>/kernels/<version>/` | **每版本 ~478 MiB，独占** | 安装固定 `--config.package-import-method=copy`，新树不与 pnpm store 共享 inode（`install_isolation.rs`）。这是 2026-09-30 黑屏事故（store 硬链接的 inode 侧信道 → NTFS ChangeTime → 内核 HMR 误判 bundle 重建）的根治手段 |
| 实例 `home/profiles/<name>/node_modules/` | **≈0 新增字节** | 其中 ≥1 MB 的文件全部是指向 `%LOCALAPPDATA%\pnpm\store\v11\files\…` 的硬链接（`fsutil hardlink list` 实测）。逻辑上有 383.7 MiB，实际是链接条目 |
| `plugins/<family>[-dev]/` | 视插件而定 | 中央库存源码 + 插件自己的 node_modules；同样可能大量命中 store |
| `~/.dsh-xlink` 之外的 pnpm store | 全机器共享 | 本机 5.2 GiB；`pnpm store prune` 回收无引用内容 |

**所以「省盘」的第一顺位是卸载不用的内核版本（~478 MiB/版本），而不是去动 profile**。

### 为什么 release / dev 的内核树不共用（`desktop/` vs `desktop-dev/`）

开发机上同一版本会被装两遍（实测：两棵树各持 `0.2.1-alpha.1` + `0.2.1-alpha.2`，合计 ~1.9 GiB，内容逐字节相同、inode 各自独立）。这是**刻意的**，三种「共用」里只有第三种在理论上成立：

1. **合并成一棵目录**——不安全。`uninstall` 是整目录 `remove_dir_all`（`lifecycle.rs::remove_kernel_dir`），而守卫 `ensure_own_shell_stopped` 只停**本壳**内核；跨壳那一侧按设计只提示不阻断（`warn_other_shell_workbench` 返回 `()`）。共用一棵树时，dev 卸载版本会删掉 release 正在跑的树。另外 `kernel_deps.rs` 会在上游缺包时往 stub 写 `overrides` 降级钉版——**目录名是版本号、不是内容摘要**，一棵树被两个壳共写后「0.2.1-alpha.2 到底是哪份 closure」无人能说清。
2. **硬链接去重 inode**——不安全，而且**正是 2026-09-30 黑屏事故的成因**：NTFS 上增删硬链接会更新 inode 的 ChangeTime，内核 `dsh-client-hmr` 每 500 ms stat 轮询客户端 bundle，把这种噪声当成「bundle 重建」推给活页面，页面换模块即崩。`--config.package-import-method=copy` 就是为消灭这条侧信道而加的（`install_isolation.rs` 采样硬链接数把这种状态标成「共享存储」）。**跨壳共享 inode = 把一个已修好的 bug 装回去。**
3. **只读共享（junction / 软链指向一份内容寻址的只读树）**——理论上可行：junction 是 reparse point，不产生硬链接数变化，装新版本写新 inode，也就没有第 2 条的 ctime 噪声。但它要新建一套跨进程协调（共享树上的装/卸必须**两个壳都停**，现在只停本壳），并要先删掉/改掉 `warn_other_shell_workbench` 的"只提示"语义；而收益只落在开发机（打包用户只有 release 壳），上限就是这里的两份重复。

**不要用 `DSH_DESKTOP_DATA_DIR` 把 dev 壳指到 release 的目录**：那不仅共用内核树，还共用 `active.txt` / `kernel.pid` / `quarantine.json` / `webview-official-chat/`。前三个直接破坏"版本与进程归属"的判定（dev 切版本会改掉 release 下次启动用的版本），最后一个的共享约束在代码里只按"同一进程内的多个环境"验证过（见 `commands.rs::open_official_chat` 的注释），跨进程共写同一个 user-data 目录没有设计保证。

## 7. 两处文档 / 代码不一致（2026-10-13 核对）

1. **per-instance `extensions/wiring.json` 不存在**。`paths::instance_wiring_file` 带 `#[allow(dead_code)]`、无生产调用方，本机实例目录下也没有该文件。接线状态的事实源是中央库 `store.json` 里的每条 item（物化模式、指纹）+ 每次接线重算。[architecture.md](architecture.md) 的目录树与 `plugins/precheck.rs`、`plugins/sandbox.rs` 的注释仍按旧描述写。
2. **`<data_dir>/kernel.pid` 与 `<instance>/runtime/pid` 是两代**。前者由 `commands.rs` 的旧单实例启动路径写（本机存在 `39000 3090`），后者由 `start_instance` 写（本机 `39000 3090 release`）。排查看实例那份；`kernel.pid` 只在旧路径与兼容读取里出现。

## 8. 路径解析与隔离契约

本节从根 `AGENTS.md` 移入，集中记录路径相关的实现约束；前文标注日期的磁盘样本仍是历史快照。

### 数据根与默认值

`lib.rs::setup()` 通过 `kernel::data_dir` 解析 `<xlink_home>/<family>/desktop/`（release）或 `desktop-dev/`（debug）。family 由 `instance::default_family()` 按本壳当前实例 → 本壳默认实例 → `dsh` 选择，不读取注册表 `default_instance_id` 指针。路径建不出来时记录原因，仍使用该路径，让后续操作明确报错；不转到平台 app-data 目录。v0.2.x 平铺的 `<xlink_home>/desktop[-dev]/` 启动时整体搬入族目录，失败则继续使用旧目录。

默认端口为 release 3090、debug 3091，用户保存的端口优先于 `Settings::default()`。默认实例分别是 `default` / `default-dev`，生产调用方通过 `instance::resolve_default()` 或 `plugins::default_instance_key()` 取值，不硬编码 `DEFAULT_INSTANCE_ID`。

### 按壳模式分家

| 数据 | 路径 / 归属 |
| --- | --- |
| 设置 / 日志 | `shell/<mode>/` |
| 安装树 / `active.txt` | `<family>/desktop[-dev]/` |
| 实例注册表 | `state/instances.json` / `state/instances-dev.json` |
| 实例 home / 插件物化 | `kernels/<family>/instances/<id>/`，按实例 id 分开 |
| 插件中央库 | `plugins/dsh/` / `plugins/dsh-dev/`，内层名称对应内核族 |
| 技能中央库 / 活动视图 | `skills/packages/` + `skills/active/`，有意共享 |

`registry_split::ensure_scoped` 在 setup 一次性拆分旧共享注册表：认领、让位须顺序无关且幂等，顶部页签只列本壳实例，不把两个列表合回去。`default_instance_id` 只由 `ensure_default_registered` 按本壳默认值认领；其他模块不写具体 id，也不读取该字段作决策。当前选择写 `settings.current_instance_id`，读取时校验本壳注册表成员资格；已删除或已让位则回退默认，注册表读失败时保留选择。

`store_relocate::ensure_ready` 接住旧插件中央库：release 整体搬、dev 复制种子；目标已存在绝不覆盖，失败只记 stderr，不阻塞启动，没有旧目录就不创建空中央库。技能共享不会改变工作台的会话、模型或插件，不能据此合并插件中央库。

`instance::ensure_instance_mutable` 仍作为兜底：两个壳若被指向同一实例，且实例内核存活，拒绝插件和内核版本变更。它不替代默认路径下的物理隔离。

### 技能接线

共享活动视图的接线按实例写：每次 `prepare_instance` 经 `kernel_adapter::ensure_skill_wiring` 向实例 `$DSH_HOME/cordis.patch.yml` 追加 `xlink-skill-filesystem` loader。当前内核从插件配置读取 `customSkillDirs`，不读取 `DSH_CUSTOM_SKILL_DIRS`，不能只注入环境变量就宣称技能可见。

接线只追加，不改写已有 patch；顶层不是列表时不碰，写失败记 `shell_events` 而不阻断启动。追加文本的公共规则复用 `kernel/patch_file.rs`。验证需同时检查配置与 `skills/list`，细节见[技能管理](../features/extensions/skill-management.md#技能接线壳怎么让内核看见活动视图)。

### 历史搬迁与找回会话

历史 `~/.dsh` 只搬入 release 实例。`home_recovery` 只读扫描别的实例中目标缺失的 `sessions/` / `attachments/`；用户确认后复制，不删源、不覆盖目标已有条目。执行前按目标实例 pid 文件确认其内核未运行，不能只看本壳工作台状态。此路径用于接回早期误搬入 dev 的历史数据，事故见[复盘 §7](../reviews/agent-lessons.md)。

数据迁移面板的后续移除计划见[迁移文档](../features/migration/migration-wizard-ui-proposal.md#维护与移除边界)，不要把它与底层目录兼容搬迁一起删除。
