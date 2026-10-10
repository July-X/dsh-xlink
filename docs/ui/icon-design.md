# dsh-xlink 图标

全仓库图标的三张母版、bundle 套板规则与增量构建触发。约定性约束见 [AGENTS.md](../../AGENTS.md)。

## 面板里的第三方标志

**面板里出现的任何标志都必须来自 `ui/public/` 下的本地文件，不许写远端 `http(s)://` 地址。**

版本面板「npm 发布」那一行的标志过去是
`<img src="https://avatars.githubusercontent.com/u/6078720?s=200&v=4">`：一行代码，构建、单测、CSS 预算全绿，而它每次渲染都要出网取一次。`tauri.conf.json` 的 `csp` 是 `null`，没有任何东西拦这个请求，于是**出不出网完全取决于用户那台机器**——断网、墙内、或公司网关拉黑 `avatars.githubusercontent.com` 时，那 16px 就是一个空图，而 `.brand-logo` 的 `background: #fff` 正好把它垫成一块看得见的白砖。取不到图时页面上没有任何错误信息。

规则：

- 标志一律进 `ui/public/`，矢量优先（SVG），构建时由 Vite 原样拷进 `ui/dist/`，运行时从 `tauri://` / `http://127.0.0.1` 本地协议取。
- **保留来源与许可声明**。第三方标志若换成许可更严格的来源（CC-BY-SA 之类），在同一条注释里写清署名要求。当前在册的是：

  | 文件 | 图形 | 来源与许可 |
  | --- | --- | --- |
  | `npm-logo.svg` | npm 标志 | Devicon 的 `icons/npm/npm-original.svg`，MIT, © 2015 konpa；标志本身是 npm, Inc. 的商标，这里只用它指代 npm 这个包来源 |
  | `deepseek-logo.svg` | DeepSeek 鲸鱼（`#4D6BFE`） | LobeHub Icons 的 `deepseek-color.svg`，MIT, © 2024 LobeHub；标志本身是 DeepSeek 的商标 |
  | `minimax-logo.svg` | MiniMax 声波（渐变 `#E2167E`→`#FE603C`） | LobeHub Icons 的 `minimax-color.svg`，同上；国内站与国际站共用 |
  | `openai-logo.svg` / `openai-logo-dark.svg` | OpenAI 结 | LobeHub Icons 的 `openai.svg`，同上。**路径未改，只把 `currentColor` 换成写死颜色**（浅色 `#0B1526` / 暗色 `#FFFFFF`）：经 `<img src>` 引用的外部 SVG 里的 `currentColor` 没有任何宿主元素可继承，会解析成黑色。暗色那份由 `OverviewPanel.vue` 的 `html.dark` 规则经 CSS `content` 换图 |
  | `zhipu-logo.svg` | 智谱原子（`#3859FF`） | LobeHub Icons 的 `zhipu-color.svg`，同上；标志本身是北京智谱华章的商标 |

  厂商标志按 `provider.id` 在 `ui/src/subscription/subscription.js` 的 `PROVIDER_LOGOS` 里登记；圆形底的底色色相在 `OverviewPanel.vue` 的 `.plan-provider-logo--<id>` 修饰类里。**新增 provider 要同时补这两处**，只补一处就是「有图没底色」或「有底色没图」。
- 图形数据要与现用的那张**同一个标志**。Simple Icons 的 `npm` 图标虽然也是 CC0 且只有 317 字节，但它是另一个图形（方块里一个抽象的方框，不是 npm 的标志），换它等于顺手改了品牌；比对方法是把它与线上那张头像并排渲染，不是看文件名像不像。
- 门禁：`check:invariants` 第 15 项同时查「不许出现远端地址」与「以 `/` 开头的 `src` 在 `ui/public` 里必须存在」。写错路径与指向远端是同一种故障（页面渲染成空图），所以两条都要拦。
- 面板上其余图标仍走 `@element-plus/icons-vue`（那是本地 npm 依赖，不出网）；需要品牌标志时 Element Plus 里没有，才落到上面这条规则。

## 母版

```
assets/
├── whale-icon.svg          # ≥128px（完整红眼细节）
├── whale-icon-small.svg    # ≤64px、favicon、ui/public/whale-icon.png
├── whale-head.svg          # Windows 通知区域（托盘）图标：整条鲸鱼 + 放大 2.4 倍的红眼，内容缩到 88%
└── whale-icon-512.png      # 512px 位图（脚本从 whale-icon.svg 渲染）
```

面板顶栏只按 60 CSS px 显示 `ui/public/whale-icon.png`（由 `whale-icon-small.svg` 渲染 128px）；小尺寸下细节是亚像素，必须简化。

`whale-head.svg` 供**两个常驻入口**共用（Windows 托盘与 macOS 菜单栏）：**整条鲸鱼**（几何与 `whale-icon-small.svg` 同源）+ 在眼心放大 2.4 倍的红眼，整个内容再缩到 88% 居中。`build-icons.sh` 由它渲染 Windows 的**两套**帧，`tray.rs` 按任务栏主题选一套、按显示缩放选一档：

| 帧 | 用在 | 做法 | 为什么 |
| --- | --- | --- | --- |
| `tray-light-{16…48}.png` | 浅色任务栏（`SystemUsesLightTheme = 1`） | 套板：与桌面图标同一套板规则（824/1024 瓦片、圆角 18%、鲸鱼占底板 75%），母版换成托盘母版，所以那步传 85%（0.85 × 母版自带的 0.88 ≈ 0.75，逐档实测与 `32x32.png` 的鲸鱼对齐到 ±1px） | 浅色任务栏上白瓦片的边缘几乎不可见，效果是给鲸鱼留了内边距；与任务栏按钮、桌面看到的是同一个图标 |
| `tray-dark-{16…48}.png` | 深色任务栏（含读不到键时的兜底） | 透明底 + 白描边（在目标尺寸上做的 1px/档栅格膨胀） | 白瓦片在深色任务栏上是一块刺眼的白方块，读作"徽标"而不是系统图标；近黑的鲸体又必须靠白边才认得出轮廓 |

- 取景用整条鲸鱼，不用鱼头特写。首版是特写（红眼放大 3 倍），但 16px 档上唯一还认得出的线索是剪影，特写把剪影换成了一颗占画幅 47.6% 的红球（实测：16px 档红眼直径 7.6px），在托盘里读作"红色状态点"而不是这条鲸鱼。整条鲸鱼缩到 88% 后红眼仍有 1–2 个像素，品牌色和剪影都保住了。
- 88% 居中而不是满帧：首版内容顶满整帧（墨迹包围盒 = 整帧，覆盖率 78%，70% 的帧周长上有内容），比 Windows 自带的单色字形大一圈，浅色任务栏上读作一块黑方块。深色那套因此四周各有约 1px 透明边；浅色那套的"内边距"由套板自带。
- 深色那套的白描边**不做在 SVG 里**，由 `build-icons.sh` 的 `tray_frame_dark()` 在每个目标尺寸上对 alpha 做形态学膨胀（16/20 档 1px、24/32 档 2px、40/48 档 3px），再叠原图。矢量描边的外侧宽度随帧尺寸一起缩，16px 档只剩 0.45px，渲染出来是 alpha 40–59/255 的不连续半透明点（实测 24 个白像素里只有 3 个完全不透明，扫描线上还夹着全透明像素）——"深色任务栏下靠白边认轮廓"的意图恰恰在最小档失效。栅格膨胀每档都是实打实的整数像素白边，也不会把鲸腹的白色一起加粗。两套的衬底/画布都必须显式 `-type TrueColorAlpha`：`xc:white` 是灰度图，灰度会把整个合成结果拉成灰度，红眼会静默变成灰点。
- 路径里的白色衬底与深色鲸体是同一个 `d`：鲸腹/嘴斑在该 path 里是洞，只画深色时洞会透出任务栏底色（深色主题下白斑消失）。先画一层全白再盖上深色，洞就恒为白色。
- 套板那套的代价要认：鲸鱼只占底板的 75%，16px 档里只有 6px 宽，红眼落到 1px 以下就没了（与桌面 16px 帧同病——`tray-light-16.png` 实测 0 个红眼像素，20px 档起恢复 1–8 个）。这是"与桌面图标一致"的必然结果，不是渲染 bug；想让 16px 也保住红点，只能给浅色那套单独放大眼睛，等于第四张母版。

改设计只改这几个 SVG 母版，然后跑 `scripts/build-icons.sh`（需 rsvg-convert + ImageMagick + macOS iconutil）一次性再生成：

- `src-tauri/icons` 全套（按尺寸选母版合成 ico/icns）
- `src-tauri/icons/tray-{dark,light}-{16,20,24,32,40,48}.png`（由 `whale-head.svg`：深色任务栏那套是透明底 + 按档栅格白描边，浅色那套是套板）
- `src-tauri/icons/menubar-{22,44}.png`（macOS 菜单栏模板图，见下节）
- `assets/whale-icon-512.png`
- `ui/public/whale-icon.png`（小母版渲染 128px，**明暗两套主题共用这一枚**）

管理面板的品牌标在两套主题下都是同一枚「红眼黑鲸」，不按主题换图。设计稿暗色那张侧栏画的是品牌蓝剪影且眼窝镂空，2026-10-10 曾据此派生过一张 `ui/public/whale-mark.png` 供 `html.dark .brand img` 换图；用户 12:44 明确否掉——「这里的 logo 应该是我们设计的红眼黑鲸的品牌 logo」。**品牌标认的是我们自己的图形，不是稿子上的配色**，改配色等于把品牌换掉。深色侧栏底（`#142239`）与纯黑鲸身对比度低，那是这个选择的代价，不是另做一张图的理由；要改就改母版后重新渲染。

顺带记一条踩过的坑：曾试过给 `<img>` 加 `mask` 上色。`mask` 裁的是"元素画出来的所有东西"（连 `outline` 一起裁），**不换掉 `<img>` 自己的像素**——黑鲸仍旧画在最上面，看上去还是黑鲸，只在抗锯齿的半透明边缘透出一圈背景色的蓝边。

不要再用 `tauri icon` 单母版再生成——它会把小尺寸帧覆盖回细节版。

眼睛射线必须用 `<polygon>` 而非 `<path>`，避免后续通用 CSS 对 `path` 的规则影响眼睛细节。

`src-tauri/icons` 只提交被引用的文件（`tauri.conf.json` 的 `icon.icns` / `icon.ico` / `32x32.png` / `128x128.png` / `128x128@2x.png`，以及 `tray.rs` 的 `tray-*.png`）。改图标后重启应用，Dock 图标缓存才会刷新。

## 桌面 bundle 图标套板

macOS Dock 不给图标加任何背景或蒙版（圆角是 artwork 自带的约定），且按 Apple 图标网格，可见圆角矩形只占画布的 824/1024、四周留透明边距。`build-icons.sh` 在渲染 desktop bundle（`icon.icns` / `icon.ico` / `icon.png` / `32x32.png` / `128x128.png` / `128x128@2x.png` / `assets/whale-icon-512.png`）时：

1. 在透明画布中央画一个 824/1024 大小的白色圆角矩形（圆角 = 画布的 18% ≈ 瓦片的 22.4%，Big Sur squircle 比例）
2. 把鲸鱼缩到瓦片的 75%（≈画布的 60%）居中叠上

三个反面教材：瓦片铺满画布 → 视觉上比其他 Dock 图标大一圈；角落压成白色 → 读作硬白方块；没有瓦片全透明 → 只剩黑色鲸鱼剪影。

## macOS 菜单栏图标

`src-tauri/icons/menubar-{22,44}.png`（1x / 2x），由 `whale-head.svg` 渲染后**转成模板图**：取原图 alpha 作遮罩，整体填黑。`menu_bar.rs` 建托盘项时必须带 `icon_as_template(true)`，否则图标会带着固定颜色被原样画在菜单栏里。

**为什么必须是模板图而不是像托盘那样准备两套**：菜单栏的明暗由系统决定（浅色桌面 = 菜单栏深色，反之亦然），而 macOS **既不广播主题变化、我们也无法预知当前值**。托盘那套「读 `SystemUsesLightTheme` + 后台线程等注册表变化换帧」在 macOS 上根本无处施展——`watch_theme` 只存在于 `tray.rs`，且 `tray.rs` 是 `cfg(windows)`。模板图把这件事交给系统：它按当前菜单栏底色自动反色，我们只需要交出一张黑图。

代价是红眼这处彩色细节随模板化丢失，菜单栏图标因此只有鲸鱼剪影。22pt 的位高上彩色细节本就读不出来，这个损失是划算的。

**为什么只出 22 / 44 两档**：菜单栏高度固定 22pt，不存在通知区域那套「按 `SM_CXSMICON` 在 6 档里选」的问题（`tray.rs` 需要自己选帧是因为 `tray-icon` 走 `CreateIcon(w, h)` 按图片自身尺寸建 HICON，尺寸不对只能被系统缩放；菜单栏由 AppKit 托管，系统会自行取合适的那一档，44 那档在 Retina 上就是 2x 资源）。

核对方法：22px 档应有约 27% 的不透明像素（22×22=484，实测 133）且**只有黑 + 透明两种颜色**——出现第三种颜色说明 `-fill black -colorize 100` 那步没生效，那样的图被当成模板图时不会被反色，会以深色画在浅色菜单栏上等于看不见。

Windows 通知区域（托盘）图标 `src-tauri/icons/tray-{dark,light}-{16,20,24,32,40,48}.png` 是上面唯一的例外：浅色任务栏那套**就是要套板**（与桌面图标同一套板规则，见上一节），深色那套保持透明底 + 栅格白描边。理由是通知区域的底色不归我们管，随"Windows 模式"在浅色（约 `#F3F3F3`）与深色（约 `#202020`）之间切：白瓦片在浅色任务栏上几乎看不见、等于内边距，在深色任务栏上却是一块刺眼的白方块；透明底 + 白边则相反。十二档都由 `tray.rs` 用 `include_image!` 在编译期解码成 RGBA，运行时按 `Personalize\SystemUsesLightTheme`（`taskbar_is_light`）选一套、按 `GetSystemMetricsForDpi(SM_CXSMICON, …)` 选不小于槽位边长的最小一档；主题变化由 `watch_theme` 的后台线程等注册表变化（切换后立刻换帧），显示缩放变化由 `WindowEvent::ScaleFactorChanged` 触发重取。为什么必须自己选帧：`tray-icon` 把 RGBA 交给 Windows 时走的是 `CreateIcon(w, h)`（按图片自身尺寸建 HICON），尺寸不对就只能由 shell 缩放——那样自带的 16px 帧永远显示不出来，最小档还要多挨一次缩放。

托盘档的核对方法（改完母版跑过 `build-icons.sh` 之后）：① 深色那套六档都要有红眼像素，若红眼变灰说明衬底那步丢了 TrueColorAlpha；② 深色那套 16px 档深色墨迹的包围盒不应贴到帧边（应有 1px 以上留白）；③ 浅色那套要有白瓦片（16px 档约 79 个白像素）且鲸鱼落在底板的 75% 上，与 `32x32.png` 叠着看应基本重合；④ 两套都放到 `#F3F3F3` 与 `#202020` 两种底上放大对比，确认"浅色用套板、深色用白边"这个配对没有反。

已知边界：`SystemUsesLightTheme` 描述的是"Windows 模式"，不是任务栏的实际像素，有两处会对不上——**高对比度主题**（任务栏可能被强制成黑底，但该值仍为 1）与"在任务栏上显示强调色"（底色被染色，可能落在中间调）。这两种情况下会显示套板那一套：宁可局部不贴底色，也不为此再猜一次系统状态。

输出必须保持 RGBA（`png:color-type=6`；`tauri::generate_context!` 编译期拒绝 RGB 图）。`ui/public/whale-icon.png` 保持全透明，叠加在深色管理面板上。

再生成只能走 macOS 上的 `build-icons.sh`（rsvg-convert + ImageMagick + iconutil，release runner 即 macOS）；仓库不保留 Windows 再生成脚本——提交在仓库里的 PNG 已是套板成品，从它们二次合成会得到双重缩小的错误结果。

## 改了图标但 dev exe 没 rebuild

`tauri-build` 通过 `tauri-winres` → `embed-resource` 把 `icons/icon.ico` 编进 Windows dev exe，但**没有发 `cargo:rerun-if-changed=` 声明**，cargo 的增量构建只看 Rust 源码变化。所以重写 `icon.ico` 之后，光 `pnpm run dev` / `cargo build` 不会触发 rebuild，exe 里嵌入的还是上一次 build 时的图标——磁盘上 PNG 是新的，任务栏却显示旧的。

`build.rs` 已经在调用 `tauri_build::build()` 之前显式声明了 18 个 `rerun-if-changed`（6 个桌面图标 `icon.ico` / `icon.icns` / `icon.png` / `32x32.png` / `128x128.png` / `128x128@2x.png`，以及 12 档托盘图标 `tray-{dark,light}-{16,20,24,32,40,48}.png`——十二档都会被 `include_image!` 编进二进制，漏声明哪一档，换掉它之后那一档会一直停在旧图），任何其中一个变化都强制 rerun build script → 重新生成 `.rc` → 重新 link。

运行中的 dev exe 锁住文件的话，Tauri dev 会先关掉再重启；如果不是 dev 模式就 `Stop-Process` 一下 `dsh-xlink` 再 build。macOS Dock 那边是缓存问题，杀掉 Dock / 重启应用就刷新；Windows taskbar 缓存比 macOS 更粘，可能要重启 Explorer（`ie4uinit.exe -show` 或任务管理器重启 explorer.exe）才能让任务栏读出新图标。
