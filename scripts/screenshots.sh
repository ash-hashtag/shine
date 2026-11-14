#!/usr/bin/env bash
# Screenshots for the website and the docs, taken from the built web app in
# headless Chromium.
#   scripts/screenshots.sh <built site dir>
# Writes web/public/screenshots/*.webp (site), docs/img/*.webp (docs) and the
# Open Graph cards (web/public/og/). ONLY="name …" takes just those shots
# (and skips the interactive ones and the cards).
set -euo pipefail
site=$1
root=$(cd "$(dirname "$0")/.." && pwd)
site_out="$root/web/public/screenshots"
docs_out="$root/docs/img"
mkdir -p "$site_out" "$docs_out"
port=$((20000 + RANDOM % 20000))
devtools=$((port + 1))
python3 -m http.server "$port" --directory "$site" >/dev/null 2>&1 &
server=$!
tmp=$(mktemp -d)
trap 'kill $server 2>/dev/null; [[ -n ${chrome:-} ]] && kill $chrome 2>/dev/null; rm -rf "$tmp"' EXIT
sleep 1

# Lossless WebP: pixel-exact UI text, roughly half the size of PNG.
webp() {
  magick "$1" -define webp:lossless=true -define webp:method=6 "$2"
}

# name|query (see Workspace::apply_url_query)|where: site, docs or both
shots=(
  "link|example=4-channel-wdm-optical-link|site"
  "wdm-link|example=4-channel-wdm-optical-link|docs"
  "adder|example=8-bit-photonic-adder|site docs"
  "full-adder|example=8-bit-photonic-adder&tab=full-adder|site docs"
  "accumulator|example=photonic-accumulator|site docs"
  "components|example=cascaded-interferometers|site docs"
  "free-space|example=mach-zehnder-interferometer|site docs"
  "dft|examples=chips&chip=optical-fourier-transform|site"
  "chips|examples=chips&chip=optical-fourier-transform|docs"
  "sweep|example=4-channel-wdm-optical-link&sweep|docs"
  "wdm|examples=chips&chip=wdm-demultiplexer|site"
  # The reference design and the optical neural network, for their pages.
  "matrix-processor|example=16-16-photonic-matrix-processor|docs"
  "neural-chip|example=digit-recognizer-optical-neural-network|docs"
  "neural-layer|example=digit-recognizer-optical-neural-network&tab=layer-1-16-16|docs"
  "neural-modulator|example=digit-recognizer-optical-neural-network&tab=linear-modulator&simulate|docs"
)
for entry in "${shots[@]}"; do
  IFS='|' read -r name query dests <<<"$entry"
  if [[ -n ${ONLY:-} && " $ONLY " != *" $name "* ]]; then
    continue
  fi
  chromium --headless=new --no-sandbox --hide-scrollbars \
    --use-angle=swiftshader --enable-unsafe-swiftshader \
    --force-color-profile=srgb --window-size=1400,900 --virtual-time-budget=10000 \
    --screenshot="$tmp/$name.png" "http://localhost:$port/app/?$query" 2>/dev/null
  for dest in $dests; do
    out=$([[ $dest == site ]] && echo "$site_out" || echo "$docs_out")
    webp "$tmp/$name.png" "$out/$name.webp"
    echo "  $out/$name.webp"
  done
done

if [[ -n ${ONLY:-} ]]; then
  exit 0
fi

# Interactive shots (menus, the parts browser) for the docs.
chromium --headless=new --no-sandbox --hide-scrollbars \
  --use-angle=swiftshader --enable-unsafe-swiftshader --force-color-profile=srgb \
  --remote-debugging-port="$devtools" --user-data-dir="$tmp/profile" about:blank >/dev/null 2>&1 &
chrome=$!
mkdir -p "$tmp/ui"
node "$root/scripts/ui-screenshots.mjs" "$devtools" "http://localhost:$port/app/?example=4-channel-wdm-optical-link" "$tmp/ui"
for png in "$tmp"/ui/*.png; do
  webp "$png" "$docs_out/$(basename "$png" .png).webp"
done

# Social preview cards for the home page and every docs page.
python3 "$root/scripts/og-cards.py"
