#!/usr/bin/env bash
# 由两张 SVG 主图重新生成这个独立应用中的全部图标资源：
#   assets/whale-icon.svg        → 大小 ≥ 128（保留完整眼睛细节：光晕、星光、射线）
#   assets/whale-icon-small.svg  → 大小 ≤ 64 以及管理面板标志
#                                 （夸张的眼睛，否则细节将落在亚像素尺度）
#
# 产物：
#   src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png, icon.ico, icon.icns
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

# ui/public/whale-icon.png 仍由 SMALL 主图以 128 渲染：面板把它绘制在
# 60 CSS px 上，因此 SMALL 主图的几何更合适；但我们依旧做超采样，以避免
# 白色高光点和闪光射线被 cairo 模糊。
magick "$TMP/small-super.png" -filter LanczosSharp -resize 128x128 \
  -define png:color-type=6 "$TMP/small-128.png"

# 为桌面打包版本盖上白色圆角底板。
# 所有最终进入 icon.ico / icon.icns / icon.png 的帧都经过这一步，
# 让图标读作 OS 风格的圆角底板：macOS Dock 自身不应用任何遮罩或背景
# —— 完全透明的图标会以裸剪影漂浮，把拐角拍平成白色则会读作硬方块。
# 底板遵循 Apple 的 macOS 图标网格：可见的圆角矩形占据画布的
# 824/1024（≈80%），居中放置，周围留白透明 —— 铺满整张画布的底板
# 会比其他 Dock 图标显著偏大。圆角半径为画布的 18%（约为底板的 22.4%，
# 即 Big Sur squircle 的比例）；鲸鱼占底板的 75%（约为画布的 60%），
# 与相邻应用图标的字形粗细匹配。面板品牌标志（`ui/public/whale-icon.png`）
# 保持完全透明，便于叠在深色管理表面上。输出保持 RGBA
# （png:color-type=6）：tauri::generate_context! 在编译期拒绝 RGB PNG。
plate_white_rounded() {
  local src="$1" size="$2" dst="$3"
  local tile=$(( size * 824 / 1024 ))
  local radius=$(( size * 18 / 100 ))
  local inset=$(( tile * 75 / 100 ))
  magick -size "${tile}x${tile}" xc:none \
    -fill white -draw "roundrectangle 0,0 $((tile - 1)),$((tile - 1)) ${radius},${radius}" \
    \( "$src" -resize "${inset}x${inset}" \) \
    -gravity center -compose Over -composite \
    -background none -gravity center -extent "${size}x${size}" \
    -depth 8 -define png:color-type=6 \
    "$dst"
}
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
