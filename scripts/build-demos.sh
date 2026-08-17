#!/usr/bin/env bash
# Build every demo to WebAssembly for the docs site. A demo is discovered by its
# Trunk.toml, so `demos/` is the only place a demo is registered: nothing here
# lists demo names, and a new demo cannot be left out of the docs build by
# forgetting to add it. Any demo that fails to build fails the whole run.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

configs=(demos/*/Trunk.toml)
if [ ! -e "${configs[0]}" ]; then
  echo "no demos/*/Trunk.toml found: nothing to build" >&2
  exit 1
fi

total=${#configs[@]}
index=0
for config in "${configs[@]}"; do
  demo=$(basename "$(dirname "$config")")
  index=$((index + 1))
  echo "==> demo $index/$total: $demo"
  trunk build --config "$config" --release
done

echo "built $total demos"
