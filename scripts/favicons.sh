#!/usr/bin/env bash
# Raster fallbacks of web/public/favicon.svg, for browsers and apps that
# don't take SVG icons: favicon.ico (16, 32, 48 px) and WebP icons (192 and
# 512 px, also used by the web app manifest).
#   scripts/favicons.sh
set -euo pipefail
cd "$(dirname "$0")/../web/public"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for size in 16 32 48 192 512; do
  magick -background none -density 1024 favicon.svg -resize "${size}x${size}" "PNG32:$tmp/$size.png"
done
magick "$tmp/16.png" "$tmp/32.png" "$tmp/48.png" favicon.ico
magick "$tmp/192.png" -define webp:lossless=true favicon.webp
magick "$tmp/512.png" -define webp:lossless=true icon-512.webp
ls -l favicon.ico favicon.webp icon-512.webp
