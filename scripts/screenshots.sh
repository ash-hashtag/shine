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
trap 'kill $server' EXIT
sleep 1

# name|query (see Workspace::apply_url_query)
shots=(
  "components|example=reusable-free-space-component"
  "component-definition|example=reusable-free-space-component&tab=free-space-mzi"
  "free-space|example=mach-zehnder-interferometer"
  "dft|example=optical-dft"
  "wdm|example=add-drop-filter&examples=chips&chip=wdm-demultiplexer"
)
for entry in "${shots[@]}"; do
  name=${entry%%|*}
  query=${entry#*|}
  chromium --headless=new --no-sandbox --hide-scrollbars \
    --use-angle=swiftshader --enable-unsafe-swiftshader \
    --force-color-profile=srgb --window-size=1400,900 --virtual-time-budget=10000 \
    --screenshot="$out/$name.png" "http://localhost:$port/app/?$query" 2>/dev/null
  echo "  $out/$name.png"
done
