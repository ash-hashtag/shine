#!/usr/bin/env bash
# Builds the Python wheel and the npm package of Shine's library.
#   bindings/pack.sh [--release] [python|wasm|all]
# Needs: the boltffi CLI (cargo install boltffi_cli --version 0.32.1), the
# wasm32-unknown-unknown Rust target, Python with setuptools and wheel, and
# pnpm (for the TypeScript compiler and the BoltFFI runtime).
# Writes dist/python/wheelhouse/*.whl and dist/wasm/pkg (the npm package).
set -euo pipefail
cd "$(dirname "$0")"

release=()
what=all
for arg in "$@"; do
  case $arg in
    --release) release=(--release) ;;
    python|wasm|all) what=$arg ;;
    *) echo "unknown argument $arg" >&2; exit 2 ;;
  esac
done

# Errors carry their message: BoltFFI names the exception after its type
# ("ShineError") and keeps the message in a field; put it in the exception's
# own message, which is what people read.
fix_errors() {
  local file=$1 old=$2 new=$3
  python3 - "$file" "$old" "$new" <<'EOF'
import sys
path, old, new = sys.argv[1:]
text = open(path).read()
if text.count(old) != 1:
    sys.exit(f"{path}: expected one “{old}”, found {text.count(old)}: has the generated code changed?")
open(path, "w").write(text.replace(old, new))
EOF
}

if [[ $what == python || $what == all ]]; then
  boltffi pack python "${release[@]}" --deny-skipped
  fix_errors dist/python/shine_photonics/__init__.py \
    "        self.error = error
        super().__init__(error)" \
    "        self.error = error
        super().__init__(error.message)"
  boltffi pack python "${release[@]}" --regenerate false
fi

if [[ $what == wasm || $what == all ]]; then
  pnpm install --frozen-lockfile --silent
  export PATH="$PWD/node_modules/.bin:$PATH"
  boltffi pack wasm "${release[@]}" --deny-skipped
  for f in dist/wasm/pkg/shine_photonics.ts dist/wasm/pkg/shine_photonics_node.ts; do
    fix_errors "$f" 'super("ShineError");' 'super(value.message);'
  done
  boltffi pack wasm "${release[@]}" --regenerate false
fi
