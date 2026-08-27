#!/usr/bin/env bash
# Regenerate every icon asset in this standalone app from the two SVG masters:
#   assets/whale-icon.svg        → sizes ≥ 128 (full eye detail: glow, star sparkle, rays)
#   assets/whale-icon-small.svg  → sizes ≤ 64 and the management-panel mark
#                                 (exaggerated eye; detail would be subpixel)
#
# Outputs:
#   src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png, icon.ico, icon.icns
#   assets/whale-icon-512.png
#   ui/public/whale-icon.png           (rendered at 128 from the SMALL master)
#
# Eye rays are <polygon>, never <path>, so broad CSS path rules cannot bleach
# them. Requires rsvg-convert, ImageMagick (magick) and macOS iconutil.
#
# Small sizes (≤64) render the SVG at a 16× supersampled canvas then downsample
# with LanczosSharp. rsvg-convert (cairo) directly downsampling radialGradient
# at 16/24/32/48/64 collapses sub-pixel detail (white highlight dot, spark rays)
# into a single pink blob. Supersampling preserves those edges, LanczosSharp
# gives crisp icon-style downsampling without the soft blur of plain Lanczos.
set -euo pipefail
cd "$(dirname "$0")/.."

ICONS=src-tauri/icons
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Large sizes: rsvg-convert directly — no downsampling needed, cairo handles
# them well and the result is the source of truth for retina/master assets.
for s in 128 256 512 1024; do
  rsvg-convert -w "$s" -h "$s" assets/whale-icon.svg -o "$TMP/master-$s.png"
done

# Small sizes: render at 16× on a clean canvas, then LanczosSharp downsample.
# 16× is chosen because the SVG viewBox is 50 and 1024/16 = 64 stays well below
# rsvg-convert's numeric limits while keeping supersample overhead trivial
# (~1 MP per frame × 5 frames).
SUPER=1024
rsvg-convert -w "$SUPER" -h "$SUPER" assets/whale-icon-small.svg -o "$TMP/small-super.png"
for s in 16 24 32 48 64; do
  magick "$TMP/small-super.png" -filter LanczosSharp -resize "${s}x${s}" \
    -define png:color-type=6 "$TMP/small-$s.png"
done

# ui/public/whale-icon.png stays at 128 from the SMALL master: the panel renders it
# at 60 CSS px so small-master geometry is correct, but we still supersample
# to keep the white highlight dot and spark rays sharp instead of cairo-blurred.
magick "$TMP/small-super.png" -filter LanczosSharp -resize 128x128 \
  -define png:color-type=6 "$TMP/small-128.png"

# Stamp a rounded white tile onto the desktop bundle variants.
# Every frame that ends up in icon.ico / icon.icns / icon.png runs
# through here so the icon reads as an OS-style rounded tile: macOS
# Dock applies NO mask or background of its own — a fully transparent
# icon floats as a bare silhouette, and corners flattened to white
# read as a hard square. The tile follows Apple's macOS icon grid:
# the visible rounded rect occupies 824/1024 (≈80%) of the canvas,
# centered, with the surrounding margin left TRANSPARENT — a tile
# that fills the whole canvas reads noticeably larger than every
# other Dock icon. Corner radius is 18% of the canvas (≈22.4% of the
# tile, the Big Sur squircle proportion); the whale is 75% of the
# tile (≈60% of the canvas), matching the glyph weight of neighboring
# app icons. The panel brand mark (`ui/public/whale-icon.png`) stays fully
# transparent so it layers onto the dark management surface. Output stays RGBA
# (png:color-type=6): tauri::generate_context! refuses RGB PNGs at
# compile time.
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

# Desktop bitmaps — rounded white plate behind the silhouette.
cp "$TMP/small-32-plate.png"  "$ICONS/32x32.png"
cp "$TMP/master-128-plate.png" "$ICONS/128x128.png"
cp "$TMP/master-256-plate.png" "$ICONS/128x128@2x.png"
cp "$TMP/master-512-plate.png" "$ICONS/icon.png"
cp "$TMP/master-512-plate.png" assets/whale-icon-512.png
cp "$TMP/small-128.png" ui/public/whale-icon.png

# Windows .ico: per-size frames, small variant below 128.
magick \
  "$TMP/small-32-plate.png" "$TMP/small-16-plate.png" "$TMP/small-24-plate.png" \
  "$TMP/small-48-plate.png" "$TMP/small-64-plate.png" "$TMP/master-256-plate.png" \
  "$ICONS/icon.ico"

# macOS .icns via an iconset; retina @2x frames reuse the next size up.
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
