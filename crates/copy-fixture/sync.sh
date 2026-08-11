#!/usr/bin/env bash
# Regenerate this crate's modules from ratcn's source, omitting tests, replacing
# crate:: with ratcn::, and applying the workspace formatter. Run after editing
# a component module in ratcn, then check the fixture crate to confirm every
# copyable module still compiles against ratcn's public API.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

src_dir=../ratcn/src

copy() {
  local src="$1" dst="$2"
  local testmod
  testmod=$(grep -n '^mod tests' "$src_dir/$src" | head -1 | cut -d: -f1)
  if [ -z "$testmod" ]; then
    cp "$src_dir/$src" "src/$dst"
  else
    head -n $((testmod - 2)) "$src_dir/$src" > "src/$dst"
  fi
  perl -pi -e 's/\bcrate::/ratcn::/g' "src/$dst"
}

copy components/barchart.rs barchart.rs
copy components/button.rs button.rs
copy components/dialog.rs dialog.rs
copy components/list.rs list.rs
copy components/select.rs select.rs
copy components/tabs.rs tabs.rs
copy components/toast.rs toaster.rs
copy components/tooltip.rs tooltip.rs

cargo fmt --manifest-path ../../Cargo.toml -p copy-fixture
