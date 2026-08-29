# dsh-xlink 图标

全仓库图标的双母版、bundle 套板规则与增量构建触发。约定性约束见 [AGENTS.md](../AGENTS.md)。

## 双母版

```
assets/
├── whale-icon.svg          # ≥128px（完整红眼细节）
├── whale-icon-small.svg    # ≤64px、favicon、ui/public/whale-icon.png
└── whale-icon-512.png      # 512px 位图（脚本从 whale-icon.svg 渲染）
```

面板顶栏只按 60 CSS px 显示 `ui/public/whale-icon.png`（由 `whale-icon-small.svg` 渲染 128px）；小尺寸下细节是亚像素，必须简化。

改设计只改两个 SVG 母版，然后跑 `scripts/build-icons.sh`（需 rsvg-convert + ImageMagick + macOS iconutil）一次性再生成：

- `src-tauri/icons` 全套（按尺寸选母版合成 ico/icns）
- `assets/whale-icon-512.png`
- `ui/public/whale-icon.png`（小母版渲染 128px）

不要再用 `tauri icon` 单母版再生成——它会把小尺寸帧覆盖回细节版。

眼睛射线必须用 `<polygon>` 而非 `<path>`，避免后续通用 CSS 对 `path` 的规则影响眼睛细节。

`src-tauri/icons` 只提交被 `tauri.conf.json` 引用的文件（`icon.icns` / `icon.ico` / `32x32.png` / `128x128.png` / `128x128@2x.png`）。改图标后重启应用，Dock 图标缓存才会刷新。

## 桌面 bundle 图标套板

macOS Dock 不给图标加任何背景或蒙版（圆角是 artwork 自带的约定），且按 Apple 图标网格，可见圆角矩形只占画布的 824/1024、四周留透明边距。`build-icons.sh` 在渲染 desktop bundle（`icon.icns` / `icon.ico` / `icon.png` / `32x32.png` / `128x128.png` / `128x128@2x.png` / `assets/whale-icon-512.png`）时：

1. 在透明画布中央画一个 824/1024 大小的白色圆角矩形（圆角 = 画布的 18% ≈ 瓦片的 22.4%，Big Sur squircle 比例）
2. 把鲸鱼缩到瓦片的 75%（≈画布的 60%）居中叠上

三个反面教材：瓦片铺满画布 → 视觉上比其他 Dock 图标大一圈；角落压成白色 → 读作硬白方块；没有瓦片全透明 → 只剩黑色鲸鱼剪影。

输出必须保持 RGBA（`png:color-type=6`；`tauri::generate_context!` 编译期拒绝 RGB 图）。`ui/public/whale-icon.png` 保持全透明，叠加在深色管理面板上。

再生成只能走 macOS 上的 `build-icons.sh`（rsvg-convert + ImageMagick + iconutil，release runner 即 macOS）；仓库不保留 Windows 再生成脚本——提交在仓库里的 PNG 已是套板成品，从它们二次合成会得到双重缩小的错误结果。

## 改了图标但 dev exe 没 rebuild

`tauri-build` 通过 `tauri-winres` → `embed-resource` 把 `icons/icon.ico` 编进 Windows dev exe，但**没有发 `cargo:rerun-if-changed=` 声明**，cargo 的增量构建只看 Rust 源码变化。所以重写 `icon.ico` 之后，光 `pnpm run dev` / `cargo build` 不会触发 rebuild，exe 里嵌入的还是上一次 build 时的图标——磁盘上 PNG 是新的，任务栏却显示旧的。

`build.rs` 已经在调用 `tauri_build::build()` 之前显式声明了 6 个 `rerun-if-changed`（`icon.ico` / `icon.icns` / `icon.png` / `32x32.png` / `128x128.png` / `128x128@2x.png`），任何其中一个变化都强制 rerun build script → 重新生成 `.rc` → 重新 link。

运行中的 dev exe 锁住文件的话，Tauri dev 会先关掉再重启；如果不是 dev 模式就 `Stop-Process` 一下 `dsh-xlink` 再 build。macOS Dock 那边是缓存问题，杀掉 Dock / 重启应用就刷新；Windows taskbar 缓存比 macOS 更粘，可能要重启 Explorer（`ie4uinit.exe -show` 或任务管理器重启 explorer.exe）才能让任务栏读出新图标。