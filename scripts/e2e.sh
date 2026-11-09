#!/usr/bin/env bash
# End-to-end tests of the built web app in headless Chromium.
#   scripts/e2e.sh [built site dir, default web/dist]
# Build first with scripts/build-web.sh. Screenshots of failures go to
# target/e2e-failures.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
site=${1:-$root/web/dist}
browser=$(command -v chromium || command -v chromium-browser || command -v google-chrome || true)
if [[ -z $browser ]]; then
  echo "Chromium or Chrome is needed for the end-to-end tests" >&2
  exit 1
fi
port=$((20000 + RANDOM % 20000))
devtools=$((port + 1))
profile=$(mktemp -d)
python3 -m http.server "$port" --directory "$site" >/dev/null 2>&1 &
server=$!
"$browser" --headless=new --no-sandbox --hide-scrollbars --user-data-dir="$profile" \
  --remote-debugging-port="$devtools" --window-size=1400,900 about:blank >/dev/null 2>&1 &
chrome=$!
trap 'kill $server $chrome 2>/dev/null; wait $chrome 2>/dev/null; rm -rf "$profile" 2>/dev/null || true' EXIT
node "$root/scripts/e2e.mjs" "$devtools" "http://localhost:$port" "$root/target/e2e-failures"
