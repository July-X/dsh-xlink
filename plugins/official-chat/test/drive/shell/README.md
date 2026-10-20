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
| `copy-kernel.mjs` | 把现役内核树**只读拷贝**一份到隔离 shell home（壳起内核需要它） |
| `shell-verify.mjs` | 复验驱动（四条断言 + 物化产物核对） |

## 怎么跑

```powershell
# ① 一次性准备：把内核树拷进隔离 shell home（约 528MB，只读拷贝源）
node plugins/official-chat/test/drive/shell/copy-kernel.mjs

# ② 终端 A：起隔离真壳（一直开着）
node plugins/official-chat/test/drive/shell/shell-dev.mjs

# ③ 终端 B：跑复验
node plugins/official-chat/test/drive/shell/shell-verify.mjs
```

证据写在 `%TEMP%\dsh-xlink-g1\evidence-<轮次>/`（**轮次独立命名**，不复用同名
文件；且**不在会被删的临时 home 之外**——见 AGENTS.md 的证据约定）。

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
