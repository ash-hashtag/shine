#!/usr/bin/env bash
# Build the WebAssembly editor and the Astro site (with the docs) into web/dist.
#
#   scripts/build-web.sh                 build everything
#   scripts/build-web.sh --screenshots   also refresh web/public/screenshots
set -euo pipefail
cd "$(dirname "$0")/.."

target_dir=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')

# wasm-bindgen's CLI must match the crate version in Cargo.lock exactly.
version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print }' Cargo.lock)
if ! command -v wasm-bindgen >/dev/null; then
  echo "wasm-bindgen not found; install it with: cargo install wasm-bindgen-cli --version $version" >&2
  exit 1
fi
installed=$(wasm-bindgen --version | awk '{ print $2 }')
if [[ "$installed" != "$version" ]]; then
  echo "wasm-bindgen $installed is installed but Cargo.lock uses $version." >&2
  echo "Run: cargo install wasm-bindgen-cli --version $version   (or: cargo update -p wasm-bindgen --precise $installed)" >&2
  exit 1
fi

echo "Building the WebAssembly app"
cargo build --profile web --lib --target wasm32-unknown-unknown
rm -rf web/public/pkg
wasm-bindgen --target web --no-typescript --out-dir web/public/pkg "$target_dir/wasm32-unknown-unknown/web/shine.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    web/public/pkg/shine_bg.wasm -o web/public/pkg/shine_bg.wasm
fi
ls -lh web/public/pkg/shine_bg.wasm

echo "Building the site"
(cd web && pnpm install --frozen-lockfile && pnpm run build)

if [[ "${1:-}" == "--screenshots" ]]; then
  scripts/screenshots.sh web/dist
  (cd web && pnpm run build)
fi
echo "Site ready in web/dist"
