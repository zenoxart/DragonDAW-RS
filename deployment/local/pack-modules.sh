#!/usr/bin/env bash
# Packs every module under modules/ into artifacts/<id>-<version>.zip
set -euo pipefail
cd "$(dirname "$0")/../.."
for dir in modules/*/; do
  [ -f "$dir/manifest.json" ] && cargo run -q -p redhive-cli -- pack "$dir" --out artifacts
done
