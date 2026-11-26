#!/usr/bin/env bash
# Runs the nopray tool, building it on first use or after source changes.
# The build cache lives outside the skill (override with NOPRAY_TARGET_DIR).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export CARGO_TARGET_DIR="${NOPRAY_TARGET_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/nopray-target}"
bin="$CARGO_TARGET_DIR/release/nopray"
if [[ ! -x "$bin" ]] || [[ -n "$(find "$here/nopray/src" "$here/nopray/Cargo.toml" "$here/nopray/Cargo.lock" -newer "$bin" -print -quit 2>/dev/null)" ]]; then
  cargo build --quiet --release --locked --manifest-path "$here/nopray/Cargo.toml" >&2
fi
exec "$bin" "$@"
