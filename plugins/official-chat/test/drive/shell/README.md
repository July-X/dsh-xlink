# 壳内复验（真实 Tauri 壳的工作台窗口）

这套脚本在**壳自己起的工作台窗口**里复验 G1 的四条断言——与同目录上一层的
`run-g1.mjs`（内核工作台直起）互补：前者证明的是**产品路径**（壳的物化链接线 +
壳打开的工作台），后者证明的是**内核编辑器本身**。

## 文件

| 文件 | 作用 |
|---|---|
| `paths.mjs` | 共享路径与端口（全部脱敏：用户目录从环境变量推导，版本号从 `active.txt` 读） |
| `cdp.mjs` | 最小 CDP 客户端（零依赖，Node 22+ 自带全局 `WebSocket`） |
| `stub-model.mjs` | OpenAI 形状的桩模型（不接真实厂商、不带凭据） |
| `shell-dev.mjs` | 起**隔离真壳**（`tauri dev`，三处隔离，见文件头） |
| `shell-release.mjs` | 起**隔离 release 壳**（自编 release 二进制，release 模式的路径/实例） |
| `copy-kernel.mjs` | 把现役内核树**只读拷贝**一份到隔离 shell home（壳起内核需要它） |
| `shell-verify.mjs` | 复验驱动（四条断言 + 物化产物核对 + 三条前置条件机检） |

## 怎么跑

```powershell
# ① 一次性准备：把内核树拷进隔离 shell home（约 528MB，只读拷贝源）
node plugins/official-chat/test/drive/shell/copy-kernel.mjs

# ② 终端 A：起隔离真壳（一直开着）
node plugins/official-chat/test/drive/shell/shell-dev.mjs

# ③ 终端 B：跑复验
node plugins/official-chat/test/drive/shell/shell-verify.mjs
```

### release 壳复验（实例 id `default`）

```powershell
# ① 准备（注意 G1_SHELL_MODE=release：路径、实例 id、端口全套换一套）
$env:G1_SHELL_MODE = 'release'
node plugins/official-chat/test/drive/shell/copy-kernel.mjs

# ② 终端 A：起隔离 release 壳（会先编一次 release 二进制，几分钟）
node plugins/official-chat/test/drive/shell/shell-release.mjs

# ③ 终端 B：跑复验
node plugins/official-chat/test/drive/shell/shell-verify.mjs
```

release 与 dev 的差别是**构建产物**层面的（`ShellMode::current()` 由
`cfg!(debug_assertions)` 决定），连带三件事：运行时子目录 `desktop`（dev 是
`desktop-dev`）、默认实例 id `default`（dev 是 `default-dev`）、面板走**内嵌
资源**（`tauri://localhost`）而不是 vite dev server。

**两处只有 release 才踩得到、且都不是「靠人记」能解决的**：

1. **WebView2 的浏览器档案只能编译期搬走**。`app_local_data_dir` 在 Windows 上
   走 `SHGetKnownFolderPath`，读 `LOCALAPPDATA` 环境变量**没用**
   （`dirs-7.0.0/src/win.rs:11` → `dirs-sys-0.5.0` 的
   `known_folder_local_app_data`）。唯一能覆盖的是配置项
   `app.appDirectoriesOverride`，而配置是**编译期**烘焙进二进制的——所以
   `shell-release.mjs` 用 `TAURI_CONFIG` 环境变量把覆盖项注入 `cargo build`
   （`tauri-build-2.7.2/src/lib.rs:682-685`）。
2. **单实例守卫会静默吃掉第二个 release 壳**。守卫按**通道名**判「是不是第一个」，
   而通道名只按壳模式分；用户机器上正跑着安装版 release 壳时，本实例会被
   「交接后退出」——用户看不到任何提示。`shell-release.mjs` 用
   `DSH_XLINK_CHANNEL_SUFFIX` 让隔离实例走自己的通道
   （`src-tauri/src/notify/activate.rs` 的 `channel_name()`）。

证据写在 `%TEMP%\dsh-xlink-g1\evidence-<轮次>/`（**轮次独立命名**，不复用同名
文件）。**它落在会被删的临时 home 之外**：探针那轮跑的临时 home
（`%TEMP%\dsh-xlink-g1\probe-<轮次>/`）跑完整个被删，截图与请求体日志写在
它里面就一起没了——AGENTS.md L33 的原话就是「**证据写在会被删的临时 home 之外**」，
这里的 `evidence-<轮次>/` 是那棵临时 home 的**兄弟目录**，所以清理跑完它还在。

## 隔离面（三处，全在临时目录）

1. `DSH_XLINK_HOME` → 临时目录：壳的实例、内核树、设置、插件物化都在这里；
2. `app.appDirectoriesOverride` → 临时目录：覆盖 `app_local_data_dir`，
   于是 **WebView2 的浏览器档案**也落在这里（Windows 上它默认在用户真实安装
   目录里；`WEBVIEW2_USER_DATA_FOLDER` 环境变量管不住，Tauri 会显式覆盖）；
3. WebView2 的 CDP 端口：让壳自己的 webview 能被驱动。

**绝不起任何指向用户真实 profile 的浏览器**——浏览器一启动就写
Cookies/Preferences/History，而 Cookies 是活文件，写坏了回滚不了。

## 踩过的坑（都在脚本里标注了位置）

1. **只能 attach 主窗口**：壳里有两个同源 webview（主窗口 + 官方对话条
   `…?chatstrip=1`）。壳的能力检查只允许主窗口跑本地命令，连错窗口时
   `builtin_set_enabled` 被拒（2026-10-20 14:28 那轮就是这么失败的）。
2. **过期的 `kernel.pid`** 会让 `start_kernel` 不起内核（端口不通、脚本死等）。
   复验前先清掉它。
3. **等「可编辑」的编辑器**，不是 composer 卡片：`[data-composer-card]` 页面一开
   就有，但内核的 input 状态没建好时它是 `contenteditable="false"` +
   `data-phase="inert"`。
4. **弹窗是延迟出现的**，且要用真实鼠标事件关，否则弹窗盖住 composer，
   `@` 打不进去。
5. **`@` 前面必须是行首或空白**（内核触发词正则 `/(?:^|\s)(@([^\s]*))$/`）。
6. **错误提示是瞬态 Toast**（`level === "error"` 走 `showToast`，带 `onDone`
   自动消失），判「可见错误出口」必须**密集采样**，单点快照会踩在消失之后。
