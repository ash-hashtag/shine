#!/usr/bin/env bash
# Installs wasm-opt from binaryen's own release into $RUNNER_TEMP and puts it
# on the PATH of later GitHub Actions steps (CI and the release). Ubuntu's
# binaryen (108) is too old for the options BoltFFI passes
# (--enable-bulk-memory-opt).
set -euo pipefail
binaryen=version_132
name=binaryen-$binaryen-x86_64-linux.tar.gz
base=https://github.com/WebAssembly/binaryen/releases/download/$binaryen
cd "${RUNNER_TEMP:?run this in GitHub Actions}"
curl -sSfL -O "$base/$name" -O "$base/$name.sha256"
sha256sum -c "$name.sha256"
tar xzf "$name"
echo "$RUNNER_TEMP/binaryen-$binaryen/bin" >> "$GITHUB_PATH"
"$RUNNER_TEMP/binaryen-$binaryen/bin/wasm-opt" --version
