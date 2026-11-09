#!/usr/bin/env bash
# Screenshot examples in the built web app with headless Chromium.
#   scripts/screenshots.sh <built site dir> <output dir>
set -euo pipefail
site=$1
out=$2
mkdir -p "$out"
port=$((20000 + RANDOM % 20000))
python3 -m http.server "$port" --directory "$site" >/dev/null 2>&1 &
server=$!
sleep 1

# name|query (see Workspace::apply_url_query)
shots=(
  "link|example=4-channel-wdm-optical-link"
  "adder|example=8-bit-photonic-adder"
  "full-adder|example=8-bit-photonic-adder&tab=full-adder"
  "accumulator|example=photonic-accumulator"
  "components|example=cascaded-interferometers"
  "free-space|example=mach-zehnder-interferometer"
  "dft|examples=chips&chip=optical-fourier-transform"
  "wdm|examples=chips&chip=wdm-demultiplexer"
)
tmp=$(mktemp -d)
trap 'kill $server; rm -rf "$tmp"' EXIT

# Lossless WebP: pixel-exact UI text, roughly half the size of PNG.
webp() {
  magick "$1" -define webp:lossless=true -define webp:method=6 "$2"
}

for entry in "${shots[@]}"; do
  name=${entry%%|*}
  query=${entry#*|}
  chromium --headless=new --no-sandbox --hide-scrollbars \
    --use-angle=swiftshader --enable-unsafe-swiftshader \
    --force-color-profile=srgb --window-size=1400,900 --virtual-time-budget=10000 \
    --screenshot="$tmp/$name.png" "http://localhost:$port/app/?$query" 2>/dev/null
  webp "$tmp/$name.png" "$out/$name.webp"
  echo "  $out/$name.webp"
done

# Social preview card for Open Graph / Twitter.
sed "s|SCREENSHOT|file://$(realpath "$out/link.webp")|" "$(dirname "$0")/og-card.html" > "$tmp/og-card.html"
chromium --headless=new --no-sandbox --hide-scrollbars --force-color-profile=srgb \
  --allow-file-access-from-files --window-size=1200,630 \
  --screenshot="$tmp/og.png" "file://$tmp/og-card.html" 2>/dev/null
webp "$tmp/og.png" "$out/og.webp"
echo "  $out/og.webp"
