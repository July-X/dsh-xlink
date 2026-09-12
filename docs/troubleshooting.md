# dsh-xlink 已知坑

环境 / 平台 / 权限相关的常见症状与处理。约定性约束见 [AGENTS.md](../AGENTS.md)。

| 症状 | 处理 |
| --- | --- |
| `pnpm install` 装出其他仓库内容 | 独立项目应在本仓库根目录运行 `pnpm install`，不要从包含它的上级目录启动命令 |
| GUI 启动（Finder/开始菜单）下检测不到 nvm 的 Node，内核安装报「未检测到 Node.js」 | GUI 进程继承的 launchd / Window-Station PATH 不含 `~/.nvm/versions/node/*/bin` 或 `%NVM_HOME%\v*` —— `node.rs` 直接扫描 nvm 根并按 default 别名解析 + 版本降序探测；仍失败时在 `<data_dir>/settings.json` 里手动指定 `node_path`，改完回概览页点 Node.js 行的「重新检测」 |
| 目标机器完全没有 Node（nvm 没装 / 装了但未 install / 系统未装） | `node.rs` 没有任何候选可探测 —— 空结果文案分别给出三类安装路径：① 版本管理器 nvm/fnm/volta；② 系统包管理器 brew/winget/apt；③ 官方安装包；并附手动路径（`settings.json` 的 `node_path`）兜底 |
| GUI 启动下检测不到用户 PATH 里的 pnpm/npm（`%AppData%\npm`），误走自动安装 | 检测扫进程 PATH 只能看到系统 PATH —— `node.rs` 的 `path_dirs()` 一律扫 `env::merged_path()`（合并注册表 `HKCU\Environment\Path`）；新增 PATH 探测点同样必须用 merged_path |
| 首次安装报「无法运行 npm 以自动安装 pnpm：系统找不到指定的路径 (os error 3)」 | `run_with_progress` 开日志时 `<data_dir>/logs/` 尚未创建（`install_version`/`kernel::start` 都在其后）—— 现在开日志前 `create_dir_all` 父目录；排查同类错误先看日志文件是否真的落盘 |
| Tauri 同步命令里创建 webview 卡死 | 用新线程创建（`open_harness` 模式） |
| debug/dev 模式使用 `@deepseek-ai/dsh@0.1.2-alpha.1` 时首次打开工作台白屏，第二次点「打开工作台窗口」才正常 | alpha.1 的工作台 URL 带进程级 launch token；当天内核日志可能残留上一次进程的旧 token，旧 token 会返回 `401`。外壳现在会在打开 WebView 前用当前端口验证 token，并等待新 token；若仍失败，打开概览页「查看日志」并根据日志路径重试，或切换/重装内核版本 |
| 主面板 invoke 全部报 `xxx not allowed. Command not found`、状态卡「加载中…」 | `src-tauri/permissions/` 一旦存在任何应用级权限文件，应用命令就从「本地窗口默认放行」翻转为「必须显式授权」。新增 `tauri::generate_handler!` 命令时必须把命令名同步进 `permissions/app-commands.json` 的 `allow-local-commands` 列表；工作台 webview（远程源）的命令单独走 `allow-focus-main-shell` + `capabilities/harness-remote.json` |
| macOS 访问 `127.0.0.1:3090`（dev 壳为 3091）失败 | WKWebView 默认允许环回，勿加 `NSAppTransportSecurity` 例外 |
| macOS 标题栏黄灯（最小化）点了没反应，`Cmd+M` 也一样（关闭红灯与绿点禁用是另一回事） | 管理窗口在 macOS 上必须**出生即无边框**：`tauri.conf.json` 的 `main.decorations: false`。若改成在 `setup` 里 `set_decorations(false)`，tao 会把 `NSWindowStyleMask` 重算成 `Borderless \| Resizable`，`Miniaturizable` 位随之丢失——`miniaturize:` 在缺这个位时静默失败（`isMiniaturized` 永远 false，不报错），Tauri 的 `minimize()` 还返回 `Ok(())`，所以前端连失败 toast 都弹不出来，看起来就是「点黄灯毫无反应」。另一个陷阱：这次样式重算是 tao **异步**排到主线程队列的，`setup()` 结束时才真正执行，因此在 `setup` 里紧接着补一句 `set_minimizable(true)` 也会被它覆盖（实测无效），必须在事件循环跑起来之后再补。回归由 `scripts/check-invariants.mjs` 的第 6 项与 `lib.rs` 的 `check_main_window_minimizable` 自检兜住。真机验证：`npm run dev` 启动后用临时命令确认 `is_minimizable()`/`is_decorated()`，再点一次黄灯看窗口是否收进 Dock——正常时 stderr 只有 `data_dir = …`，一旦自检发现窗口不可最小化会另打印一条以「管理窗口不是「可最小化」的」开头的告警；`osascript` 的 `AXMinimized` 需要辅助功能授权，未授权时会报 `-1719`，别把它当成窗口状态 |
| 任务跑完没有角标 / 没有通知气泡 | 先看「设置 → 任务通知」的提示：`尚未连接内核事件流` 说明壳没订阅上——内核没启动，或 WebSocket 断了（`lastError` 有原因）。订阅线程按 1→20 s 退避重连并每次重新认证，内核重启后自愈。点「模拟一次任务完成」可把整条链路走一遍：**角标**与构建是否打包无关，`tauri dev` 也能看到；**系统通知气泡**在 macOS 上要求进程位于 `.app` bundle 内，裸二进制会被系统丢弃（日志里是 `NotificationCenter: Unable to find valid bundle with backupPath: …/target/debug/dsh-xlink`，但投递函数仍返回成功），面板会以灰字说明这一点，验证请用 `npm run build -- --debug` 产出的 `.app` 或安装版。Windows 未安装的构建会以 PowerShell 名义显示通知。设计见 [notification-design.md](notification-design.md) |
| 通知开着但角标一直是 0 | 角标记的是**用户没在看工作台时**完成的任务数：工作台在前台时完成的任务既不计数也不提醒（默认策略，可在「设置 → 任务通知」关掉「仅当工作台窗口不在前台时通知」）；子代理会话的完成也不打扰用户，均属预期 |
| 改内核事件流相关代码后怎么验证 | 跑真实内核上的集成测试：`DSH_DESKTOP_DATA_DIR=~/.dsh/desktop DSH_XLINK_LIVE_PORT=<端口> cargo test --lib -- --ignored live_`，它覆盖 launch token → cookie、WebSocket 握手、`$events` 就绪帧与标题快照。**不要**改成轮询 `session/list`：实测一次约 2.1 MB / 0.5 s 内核 CPU |
| 编辑器报 `capabilities/default.json` 缺 `$schema` | schema 由首次 `tauri build` 生成，属正常 |
| updater 显示"已是最新"但实际有新版 | endpoint `/releases/latest/download/latest.json` 拿到 404——发布版本是 draft 或 prerelease。检查 `.github/workflows/desktop-release.yml` 是否被改过或最近一次 GitHub Release 是否被标成 prerelease |
| Windows 更新后仍能从旧目录或旧快捷方式启动 | `tauri-plugin-updater` 的 NSIS `/UPDATE` 会覆盖安装但不执行旧版本卸载。新版本首次状态刷新后，对更新标记记录的不同安装目录调用其 `uninstall.exe /S`，同目录则直接删除历史 exe 和默认快捷方式，同时清理受限范围内的 updater 临时目录；若文件被占用或卸载失败，查看 `release-shell-update-cleanup-<日期>.log`，重启应用会自动重试。没有写入更新标记的用户自定义安装目录不会被扫描或删除 |
| Windows 上点通知区域图标（或托盘菜单「显示主界面」）后进程直接消失，系统弹「dsh-xlink has stopped working / Error launching CrashSender.exe」 | 那是栈溢出的表现（`thread 'main' has overflowed its stack` 被写进 stderr，GUI 启动时看不到）。首版把恢复动作写成了 `tray::show_main_shell → lib::show_main_shell → tray::show_main_shell` 的自我递归。恢复动作现在只写在 `tray::show_main_shell`（Windows）里，`lib::show_main_shell` 只是按平台分发；改动这两处时不要互相回调。注意 GUI 启动的应用没有 stderr，要复现请在终端里跑 `src-tauri/target/debug/dsh-xlink.exe` 才能看到 panic / 栈溢出信息 |
| Windows 任务栏图标不更新 | `tauri-build` 默认不发 `cargo:rerun-if-changed`，需要 `Stop-Process dsh-xlink` 后再 `cargo build`；重启 Explorer（`ie4uinit.exe -show`）清任务栏缓存。详见 [icon-design.md](icon-design.md) |
| macOS Dock 图标不更新 | 杀掉 Dock（`killall Dock`）或重启应用清缓存 |
| 卸载插件后工作台无法启动，内核日志报 `cannot resolve profile bundle "<包名>"` | 托管 spec 曾按目录名子串 `desktop/kernels/` 判定，dev 壳（`desktop-dev/`）接线的插件卸载后依赖与 bundle 层残留在 profile manifest，内核沿悬空符号链接解析失败 —— 现改为按 `kernels/<version>/plugins/<id>` 尾部路径结构判定（与壳的数据目录名无关）；手工恢复：删掉 `~/.dsh/profiles/<profile>/package.json` 里该插件的 dependencies 与 bundles 条目，删 `node_modules/` 下悬空链接后 `pnpm install` |
| 启动容错面板中移除插件后告警仍显示 | 若上次卸载已删除中央库记录但留下 `quarantine.json`，重新点击「移除插件」会按隔离记录完成幂等清理；若正在运行旧版 dev shell，需要重启并重新构建 Rust 端后再操作 |
| 工作台 DevTools 一堆 `Failed to load resource ... 404 ... .js.map`（默认安装约 44 条，仅 debug 构建自动开启的 DevTools 可见） | 内核 `@deepseek-ai/dsh` 的 npm tarball 故意不带 `.js.map` 体积，但构建产物末尾仍带 `//# sourceMappingURL=`，浏览器于是逐个去拉并得到 404。壳在打开工作台前扫描当前内核前端 `dist`，只为已声明但缺失的 map 创建最小合法 sidecar；已有 map 不覆盖，壳也不修改 JS。若前端包目录只读，工作台仍可正常启动，但需忽略这类调试提示；若想真正看到源码，回到 `tauri dev` 之外另装带 source map 的本地内核构建即可 | |
