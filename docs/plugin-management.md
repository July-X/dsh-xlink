# dsh-desktop 插件管理设计

> 本文档描述桌面外壳的社区插件管理功能：集中存储、按内核物化、管理与更新界面。
> 用户文档见 [README.md](../README.md) 的「插件管理」一节；本文档记录布局、流程与取舍。

## 目标

用户可以把社区 dsh 插件（GitHub `dsh-plugin` topic、npm 上的 bundle 包）安装到本地，
由桌面外壳统一管理，并且：

1. **集中管理**：所有插件源存放在 dsh home 下自定义目录，绝不直接写入某个内核安装目录内部。
2. **按内核物化**：每个内核通过「复制」或「链接」的方式从中央库读取插件；切换内核后插件无需重新下载、重新安装。
3. **更新提醒**：管理界面在有新版本时提醒用户一键更新。

## 术语

| 词 | 含义 |
| --- | --- |
| 插件（plugin） | 一个 npm 包或 git 仓库，其 package.json 声明 `dsh.bundle`（profile 层）和/或 `dsh.client`（Web 客户端代码），或仅作为普通依赖供其他插件使用 |
| 中央库（store） | `~/.dsh/plugins/`，插件源的唯一权威副本 |
| 物化（materialize） | 把一个中央库插件以链接或复制的方式落到某个内核安装目录 |
| 接线（wiring） | 把插件以依赖 + bundle 层的形式登记进 profile，使内核启动时真正加载它 |

## 目录布局

```text
~/.dsh/                          # dsh home（DSH_HOME 可重定向）
├── plugins/                     # 中央插件库（外壳与所有内核共享，不属任何内核）
│   ├── store.json               # 已安装插件清单（来源/版本/模式/profile/更新时间）
│   └── <plugin-id>/             # 一个插件的源（npm 包解包或 git checkout）
│       ├── package.json         # 插件自身清单（dsh.bundle / dsh.client 声明）
│       ├── .dsh-source.json     # 外壳写入：id/来源/版本/拉取时间
│       └── node_modules/        # 仅 link 模式需要：插件自身依赖（hoisted）
├── profiles/<profile>/          # 内核共享的 profile（切换内核后原样保留）
│   ├── package.json             # dependencies 指向活动内核的物化目录；dsh.profile.bundles 含插件层
│   └── cordis.patch.yml         # 用户自己的 patch 层，外壳不改写
└── desktop/
    ├── active.txt               # 活动内核版本
    ├── plugins-catalog.json     # 社区目录缓存（TTL 6 小时）
    └── kernels/<version>/
        ├── plugins/             # 该内核内部读取插件的目录（物化目标）
        │   ├── <plugin-id>/     # = 指向中央库的符号链接（link 模式）或真实副本（copy 模式）
        │   └── .meta/<plugin-id>.json   # 物化记录 {mode, version, syncedAt}
        └── node_modules/        # 内核本体（不写入任何插件）
```

插件 id 由包名/仓库名映射：`/` 替换为 `__`（如 `@ace-zone/dsh-market` → `@ace-zone__dsh-market`）。

## 安装流程

以「安装一个 npm 插件」为例（git 插件同理，仓库 URL 作来源）：

1. **拉取进中央库**：查询 npm registry 文档，取 `dist-tags.latest`（或用户指定版本）与 `dist.tarball`，下载 tarball，用系统 tar 解包到 `~/.dsh/plugins/<id>/`，写入 `.dsh-source.json` 与 `store.json`。
2. **安装自身依赖**：仅 link 模式需要。中央库里的插件目录不是 pnpm 工作区的一部分，链接后 Node 会从该目录的 `node_modules` 解析插件依赖，所以在库内执行一次 `pnpm install`（hoisted，日志落盘 `logs/plugin-<id>.log`）。插件声明的 `peerDependencies`（如 `cordis`、`@deepseek-ai/dsh-*` 服务定义）从活动内核的 `node_modules` 里链接（或复制）进库内 `node_modules`，记录在 `.dsh-peers.json`（按内核版本），切换内核时自动重解析——保证与内核共享同一份 cordis 实例；copy 模式不需要这些：profile 的 pnpm 会负责插件的传递依赖，peer 走内核目录的父级查找天然可达。
3. **按内核物化**：对每个已安装内核执行物化（见下节）。新安装的内核在安装完成后同样物化。
4. **接线**：把插件登记进 `~/.dsh/profiles/<profile>/package.json`：
   - `dependencies["<包名>"] = "link:../../desktop/kernels/<活动版本>/plugins/<id>"`（link 模式）
   - `dependencies["<包名>"] = "file:../../desktop/kernels/<活动版本>/plugins/<id>"`（copy 模式）
   - dev 壳（`tauri dev`）的物化目标在 `desktop-dev/` 下，写出的 spec 相应含 `desktop-dev/kernels/`。依赖是否由壳接管（卸载/隔离时清退）按 spec 的尾部路径结构 `kernels/<version>/plugins/<id>` 判定，与数据目录名无关；其余 spec（版本号、指向任意目录的 link/file）视为用户/CLI 管理，接线校正不动。
   - 若插件清单声明 `dsh.bundle`，把包名追加进 `dsh.profile.bundles`（去重、保留模板层）。
   - 在 profile 目录运行 `pnpm install`（profile 自带 pnpm-workspace.yaml，hoisted/peers 语义与 `dsh plugin` 一致），使 `node_modules/<包名>` 指向物化目录。内核启动时 Loader 按 bundle 名从 profile 解析并应用其 patch 层，与 `dsh plugin add` 行为一致。
5. **卸载**：反向执行——移除依赖与 bundle 层、profile pnpm install 清理、删除所有内核的物化产物与中央库目录、更新 store.json。

### 物化双模式（复制 or 链接）

| 模式 | 内核侧产物 | 优点 | 缺点 |
| --- | --- | --- | --- |
| link（默认，优先尝试） | `kernels/<v>/plugins/<id>` 是指向中央库的符号链接（Windows 上 junction） | 省空间；更新直达；切换内核零拷贝 | 依赖符号链接支持；内核目录不自包含 |
| copy（链接失败/用户选择） | 真实目录副本（跳过未变化的文件） | 内核自包含，中央库移动/删除后照常运行；Windows 无链接权限问题 | 更新需重新同步并重跑 profile pnpm install；占空间 |

每个插件记录期望模式；Windows 上链接尝试失败时自动降级为 copy 并在 UI 明示。物化元数据记录实际模式与版本，供「待同步」状态与更新提醒判断。

## 切换内核

活动内核由 `active.txt` 决定。因为**所有已安装内核在安装/更新时都已物化**，切换动作只是：

1. 若新活动内核缺少某插件的物化产物，从中央库即时物化（link 模式为建链接，copy 模式为复制）。
2. 重写 profile 依赖里的内核版本路径段，仅在变化时在 profile 里重跑 `pnpm install`（纯链接，离线、亚秒级）。
3. 全程不访问网络、不重新解析依赖——「切换内核后插件无需重新安装」。

`activate_version` 与 `start_kernel` 都会触发接线校正（`ensure_wiring`），保证任意路径进入的启动都拿到一致状态；卸载内核时其 `plugins/` 目录随目录删除。

## 更新提醒

三种来源的「最新版本」判定：

| 来源 | 最新版本来源 | 说明 |
| --- | --- | --- |
| npm 包 | `registry.npmjs.org/<包名>` 的 `dist-tags.latest` | 与内核更新菜单同一 HTTP 模式（ureq + rustls） |
| git 仓库（锁定 tag） | `git ls-remote --tags` 中语义化最新的 tag | 无需 API token，任意 git 宿主可用 |
| git 仓库（跟随分支） | `git ls-remote <url> HEAD` 与本地 sha 比较 | 无 tag 插件退化为分支跟踪 |

一次「检查更新」遍历中央库所有插件，把有新版的项目写回 `store.json`。UI 行为：

- 管理面板「插件管理」卡片头部显示 `N 个更新可用` 徽标；每行显示 `有更新` 徽标与「更新」按钮。
- 应用启动/状态刷新时若存在上次检查结果，自动提示；「检查更新」按钮手动触发全量检查。
- 更新 = 重新拉取进中央库（同源同版本约束）→ 重新物化（link 模式即时生效；copy 模式差异复制）→ copy 模式下重跑 profile pnpm install，全程进度走进度面板。

## 社区目录（浏览）

外壳从参考实现 `losebird/dsh-plugin-market` 的 `registry/all.json` 拉取社区目录（缓存 6 小时），管理面板支持按名称/描述/分类搜索并一键安装：

- 条目 `package` 字段存在 → 按 npm 包安装；
- 否则按 `repo` 的 git URL 安装（使用条目里的 `spec`/tag 锁定版本，无 tag 则分支跟踪）；
- 每个条目展示类型/star/下载量/验证标记，「详情」跳转 GitHub。官方 [dsh-plugin topic 页](https://github.com/topics/dsh-plugin) 作为浏览入口链接常驻卡片。

手动安装输入接受 npm 包名（含 `@scope` 与 `@version`）或 git URL（支持 `#tag`）。

## 安全边界

- 插件是任意代码，安装动作需要用户逐次确认；目录条目来自社区，未验证标记原样展示。
- 只信任两个来源解析元数据：npm registry（官方域名）与 GitHub（含 raw.githubusercontent 的目录缓存）；与内核更新菜单的信任边界一致。
- pnpm 构建脚本按 `pnpm approve-builds` 语义处理：非零退出码但产物完整时降级为警告，与内核安装一致；日志始终落盘 `logs/plugin-<id>.log`。
- 插件 id 映射只做 `/` → `__` 替换并拒绝 `..`/空段，避免路径穿越。

## 已知取舍

- link 模式的更新即时到达内核读取路径，但插件在工作内核里热替换仍需重启内核（与 `dsh plugin` 一致：profile 层在启动时快照）。
- link 模式依赖插件库目录内已有自身依赖与 peer 解析（见「安装流程」）；持怀疑态度的插件可切换为 copy 模式获得 profile 级依赖解析。
- copy 模式内核自包含，但中央库被删除后再次更新会丢失比较基准；重新安装即可恢复。
- 物化复制用「大小 + 修改时间」判断跳过未变化文件，不逐字节哈希（性能取舍）。
