#!/usr/bin/env bash
# Serve one demo with Trunk's dev server: scripts/demo-dev.sh <demo>, or
# `npm run demo:dev -- <demo>`. The demo names come from demos/ itself, so the
# list cannot drift from what exists.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

demo=${1-}
if [ -z "$demo" ] || [ ! -f "demos/$demo/Trunk.toml" ]; then
  {
    if [ -n "$demo" ]; then
      echo "no demo named '$demo'."
    fi
    echo "usage: npm run demo:dev -- <demo>"
    echo "demos:"
    for config in demos/*/Trunk.toml; do
      echo "  $(basename "$(dirname "$config")")"
    done
  } >&2
  exit 1
fi

exec trunk serve --config "demos/$demo/Trunk.toml"
