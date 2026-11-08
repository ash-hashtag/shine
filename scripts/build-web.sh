#!/usr/bin/env bash
# Build the WebAssembly editor and the Astro site into web/dist.
#
#   scripts/build-web.sh                 build everything
#   scripts/build-web.sh --screenshots   also refresh web/public/screenshots
set -euo pipefail
cd "$(dirname "$0")/.."

target_dir=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')

# wasm-bindgen's CLI must match the crate version exactly; keep one locally.
version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print }' Cargo.lock)
bindgen="$PWD/target/tools/bin/wasm-bindgen"
if ! "$bindgen" --version 2>/dev/null | grep -qx "wasm-bindgen $version"; then
  echo "Installing wasm-bindgen-cli $version into target/tools"
  cargo install wasm-bindgen-cli --version "$version" --root target/tools --locked
fi

echo "Building the WebAssembly app"
cargo build --profile web --lib --target wasm32-unknown-unknown
rm -rf web/public/pkg
"$bindgen" --target web --no-typescript --out-dir web/public/pkg "$target_dir/wasm32-unknown-unknown/web/shine.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    web/public/pkg/shine_bg.wasm -o web/public/pkg/shine_bg.wasm
fi
ls -lh web/public/pkg/shine_bg.wasm

echo "Building the site"
(cd web && npm ci --no-audit --no-fund && npm run build)

if [[ "${1:-}" == "--screenshots" ]]; then
  scripts/screenshots.sh web/dist web/public/screenshots
  (cd web && npm run build)
fi
echo "Site ready in web/dist"
