#!/usr/bin/env bash
# 由三张 SVG 主图重新生成这个独立应用中的全部图标资源：
#   assets/whale-icon.svg        → 大小 ≥ 128（保留完整眼睛细节：光晕、星光、射线）
#   assets/whale-icon-small.svg  → 大小 ≤ 64 以及管理面板标志
#                                 （夸张的眼睛，否则细节将落在亚像素尺度）
#   assets/whale-head.svg        → Windows 通知区域（托盘）图标
#                                 （整条鲸鱼 + 放大的红眼，透明底 + 按档栅格白描边）
#
# 产物：
#   src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png, icon.ico, icon.icns
#   src-tauri/icons/tray-{dark,light}-{16,20,24,32,40,48}.png
#   assets/whale-icon-512.png
#   ui/public/whale-icon.png           （由 SMALL 主图以 128 渲染）
#
# 眼睛射线一律使用 <polygon> 而非 <path>，因此宽泛的 CSS 路径规则无法
# 将其漂白。需要 rsvg-convert、ImageMagick (magick) 以及 macOS iconutil。
#
# 小尺寸（≤64）先在 16× 超采样画布上渲染 SVG，再用 LanczosSharp 缩放。
# rsvg-convert（cairo）在 16/24/32/48/64 直接对 radialGradient 缩放时，
# 会把亚像素细节（白色高光点、闪光射线）压成一块粉色色斑。超采样能保留
# 这些边缘，LanczosSharp 则提供接近图标风格的清晰缩放，避免普通 Lanczos
# 的柔和模糊。
set -euo pipefail
cd "$(dirname "$0")/.."

ICONS=src-tauri/icons
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# 大尺寸：直接用 rsvg-convert —— 无需缩放，cairo 对它们处理得很好，
# 输出即是 retina/主资源的真实来源。
for s in 128 256 512 1024; do
  rsvg-convert -w "$s" -h "$s" assets/whale-icon.svg -o "$TMP/master-$s.png"
done

# 小尺寸：在干净画布上以 16× 渲染，再用 LanczosSharp 缩放。
# 选择 16× 是因为 SVG viewBox 为 50，1024/16 = 64 远低于 rsvg-convert 的
# 数值上限，同时超采样开销可以忽略（约 1 MP × 5 帧）。
SUPER=1024
rsvg-convert -w "$SUPER" -h "$SUPER" assets/whale-icon-small.svg -o "$TMP/small-super.png"
for s in 16 24 32 48 64; do
  magick "$TMP/small-super.png" -filter LanczosSharp -resize "${s}x${s}" \
    -define png:color-type=6 "$TMP/small-$s.png"
done

# 为桌面打包版本盖上白色圆角底板。托盘图标里"浅色任务栏"那一套也走这里，
# 因此函数定义必须在生成托盘帧之前。
# 所有最终进入 icon.ico / icon.icns / icon.png 的帧都经过这一步，
# 让图标读作 OS 风格的圆角底板：macOS Dock 自身不应用任何遮罩或背景
# —— 完全透明的图标会以裸剪影漂浮，把拐角拍平成白色则会读作硬方块。
# 底板遵循 Apple 的 macOS 图标网格：可见的圆角矩形占据画布的
# 824/1024（≈80%），居中放置，周围留白透明 —— 铺满整张画布的底板
# 会比其他 Dock 图标显著偏大。圆角半径为画布的 18%（约为底板的 22.4%，
# 即 Big Sur squircle 的比例）；鲸鱼默认占底板的 75%（约为画布的 60%），
# 与相邻应用图标的字形粗细匹配。面板品牌标志（`ui/public/whale-icon.png`）
# 保持完全透明，便于叠在深色管理表面上。输出保持 RGBA
# （png:color-type=6）：tauri::generate_context! 在编译期拒绝 RGB PNG。
#
# `inset_pct` 是鲸鱼相对**底板**的百分比：桌面母版里鲸鱼本身就是画布的 97.6%，
# 所以 75 就是"鲸鱼占底板 75%"。托盘母版自带 88% 内缩（鲸鱼 = 画布的 86%），
# 浅色任务栏那套要传 85 才落回同一比例（0.85 × 0.86 ≈ 0.75，实测逐档对齐到 ±1px）。
# 衬底/画布都显式标 TrueColorAlpha：`xc:white` 是灰度图，灰度会把合成结果整体
# 拉成灰度，红眼会静默变成灰点。
plate_white_rounded() {
  local src="$1" size="$2" dst="$3" inset_pct="${4:-75}"
  local tile=$(( size * 824 / 1024 ))
  local radius=$(( size * 18 / 100 ))
  local inset=$(( tile * inset_pct / 100 ))
  magick -size "${tile}x${tile}" xc:none \
    -fill white -draw "roundrectangle 0,0 $((tile - 1)),$((tile - 1)) ${radius},${radius}" \
    \( "$src" -resize "${inset}x${inset}" \) \
    -gravity center -compose Over -composite \
    -background none -gravity center -extent "${size}x${size}" \
    -type TrueColorAlpha -depth 8 -define png:color-type=6 \
    "$dst"
}

# 托盘图标：整条鲸鱼 + 放大红眼的专用主图（assets/whale-head.svg），两套帧对应
# 两种任务栏主题——通知区域的底色不归我们管，随"Windows 模式"在浅色（约
# #F3F3F3）与深色（约 #202020）之间切，一套素材同时伺候两种底色必然各丢一半：
#   tray-light-*.png  浅色任务栏：套板（与桌面图标同一套板规则），读作我们自己的
#                     应用图标；浅色下白底的边缘几乎不可见，等于给鲸鱼留了内边距。
#   tray-dark-*.png   深色任务栏：透明底 + 白描边；白底瓦片在深色任务栏上是一块
#                     刺眼的白方块，读作"徽标"而不是系统图标。
# 16/20/24/32 对应 100/125/150/200% 显示缩放，由 tray.rs 按 SM_CXSMICON 在运行时
# 选帧；40/48 留作更高 DPI 的余量。
#
# 深色那套的白描边必须在**目标尺寸**上做，不能在 SVG 里用矢量描边：矢量描边的
# 外侧宽度随帧尺寸一起缩小，16px 档只剩 0.45px，渲染出来是不连续的半透明点
# （实测 alpha 40–59/255，中间还夹一个全透明像素），"深色任务栏下靠白边认轮廓"
# 的设计意图在最小档反而失效。这里对每档的 alpha 做 1px/档的形态学膨胀再叠原图：
# 每档都是实打实的整数像素白边，也不会把鲸腹的白色一起加粗。
tray_frame_dark() {
  local src="$1" size="$2" dst="$3"
  local ring=$(( (size + 8) / 16 ))
  [ "$ring" -lt 1 ] && ring=1
  # 衬底必须显式标成 TrueColorAlpha：`xc:white` 是 Bilevel/灰度图，灰度衬底会把
  # 整个合成结果拉成灰度——红眼（#e01214）会静默变成灰点，肉眼在 16px 上只看到
  # 一个白点，很难归因。两处 `-type TrueColorAlpha` 都是防这个。
  magick -size "${size}x${size}" xc:white \
    \( "$src" -alpha extract -threshold 50% -morphology Dilate Disk:"$ring" \) \
    -alpha off -compose CopyOpacity -composite \
    -type TrueColorAlpha \
    "$src" -compose Over -composite \
    -type TrueColorAlpha -depth 8 -define png:color-type=6 "$dst"
}

rsvg-convert -w "$SUPER" -h "$SUPER" assets/whale-head.svg -o "$TMP/head-super.png"
for s in 16 20 24 32 40 48; do
  magick "$TMP/head-super.png" -filter LanczosSharp -resize "${s}x${s}" \
    -define png:color-type=6 "$TMP/head-$s.png"
  tray_frame_dark "$TMP/head-$s.png" "$s" "$TMP/tray-dark-$s.png"
  plate_white_rounded "$TMP/head-$s.png" "$s" "$TMP/tray-light-$s.png" 85
done

# ui/public/whale-icon.png 仍由 SMALL 主图以 128 渲染：面板把它绘制在
# 60 CSS px 上，因此 SMALL 主图的几何更合适；但我们依旧做超采样，以避免
# 白色高光点和闪光射线被 cairo 模糊。
magick "$TMP/small-super.png" -filter LanczosSharp -resize 128x128 \
  -define png:color-type=6 "$TMP/small-128.png"

# 为桌面打包版本盖上白色圆角底板。
for s in 16 24 32 48 64; do
  plate_white_rounded "$TMP/small-$s.png" "$s" "$TMP/small-$s-plate.png"
done
for s in 128 256 512 1024; do
  plate_white_rounded "$TMP/master-$s.png" "$s" "$TMP/master-$s-plate.png"
done

# 桌面位图 —— 剪影下方放置圆角白底板。
cp "$TMP/small-32-plate.png"  "$ICONS/32x32.png"
cp "$TMP/master-128-plate.png" "$ICONS/128x128.png"
cp "$TMP/master-256-plate.png" "$ICONS/128x128@2x.png"
cp "$TMP/master-512-plate.png" "$ICONS/icon.png"
cp "$TMP/master-512-plate.png" assets/whale-icon-512.png
cp "$TMP/small-128.png" ui/public/whale-icon.png

# Windows 通知区域（托盘）图标：由 whale-head.svg 渲染的两套透明/套板 PNG。
# 十二档都由 src-tauri/src/tray.rs 经 include_image! 在编译期解码成 RGBA，
# 运行时按任务栏主题选一套、按 SM_CXSMICON 选不小于槽位边长的最小一档。
for s in 16 20 24 32 40 48; do
  cp "$TMP/tray-dark-$s.png" "$ICONS/tray-dark-$s.png"
  cp "$TMP/tray-light-$s.png" "$ICONS/tray-light-$s.png"
done

# Windows .ico：按尺寸分别提供帧，128 以下使用小尺寸变体。
magick \
  "$TMP/small-32-plate.png" "$TMP/small-16-plate.png" "$TMP/small-24-plate.png" \
  "$TMP/small-48-plate.png" "$TMP/small-64-plate.png" "$TMP/master-256-plate.png" \
  "$ICONS/icon.ico"

# macOS .icns 通过 iconset 生成；retina @2x 帧复用上一档尺寸。
ICONSET="$TMP/whale.iconset"
mkdir "$ICONSET"
cp "$TMP/small-16-plate.png"   "$ICONSET/icon_16x16.png"
cp "$TMP/small-32-plate.png"   "$ICONSET/icon_16x16@2x.png"
cp "$TMP/small-32-plate.png"   "$ICONSET/icon_32x32.png"
cp "$TMP/small-64-plate.png"   "$ICONSET/icon_32x32@2x.png"
cp "$TMP/master-128-plate.png" "$ICONSET/icon_128x128.png"
cp "$TMP/master-256-plate.png" "$ICONSET/icon_128x128@2x.png"
cp "$TMP/master-256-plate.png" "$ICONSET/icon_256x256.png"
cp "$TMP/master-512-plate.png" "$ICONSET/icon_256x256@2x.png"
cp "$TMP/master-512-plate.png" "$ICONSET/icon_512x512.png"
cp "$TMP/master-1024-plate.png" "$ICONSET/icon_512x512@2x.png"
iconutil -c icns "$ICONSET" -o "$ICONS/icon.icns"
