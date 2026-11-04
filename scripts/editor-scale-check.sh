#!/usr/bin/env bash
# Opens VST3 editors on a private Xvfb display, at scale 1 and again with GDK_SCALE=2, and checks
# that each editor's window doubles: the host passed the scale (IPlugViewContentScaleSupport)
# and sized the window from the plug-in's answer. Uses vst3-host's `editor` example, not the app.
#
# usage: scripts/editor-scale-check.sh [plug-in name ...]
#   default: LSP Compressor Mono, Dragonfly Hall Reverb, CHOWTapeModel and TONE3000; each one
#   that isn't installed is skipped.
# Needs Xvfb and xwininfo, e.g. `nix shell nixpkgs#xorg.xorgserver nixpkgs#xorg.xwininfo`.
set -euo pipefail

cd "$(dirname "$0")/.."
PLUGINS=("$@")
[ ${#PLUGINS[@]} -gt 0 ] || PLUGINS=("Compressor Mono" "Dragonfly Hall Reverb" "CHOWTapeModel" "TONE3000")
for tool in Xvfb xwininfo; do
  command -v "$tool" >/dev/null || { echo "needs $tool (see the comment at the top)" >&2; exit 2; }
done

cargo build -q -p vst3-host --example editor
EXAMPLE="${CARGO_TARGET_DIR:-target}/debug/examples/editor"
SCRATCH=$(mktemp -d)
# A display number nobody uses; -noreset keeps it up between clients.
NUM=$((90 + RANDOM % 400))
Xvfb ":$NUM" -screen 0 3840x2160x24 -nolisten tcp -noreset >"$SCRATCH/xvfb.log" 2>&1 &
XVFB=$!
trap 'kill $XVFB 2>/dev/null; rm -rf "$SCRATCH"' EXIT
export DISPLAY=":$NUM"
for _ in $(seq 50); do xwininfo -root >/dev/null 2>&1 && break; sleep 0.1; done

# The size of the editor window while `editor` runs with the given environment, or nothing.
size() {
  local name=$1
  shift
  env "$@" "$EXAMPLE" "$name" 4 >"$SCRATCH/run.log" 2>&1 &
  local pid=$! geometry=""
  for _ in $(seq 30); do
    sleep 0.1
    geometry=$(xwininfo -root -tree | grep '"marbble-plugin"' | grep -o '[0-9]*x[0-9]*+' | head -1 | tr -d +) || true
    [ -n "$geometry" ] && break
  done
  sleep 1 # let a plug-in that sizes itself after `attached` do so
  geometry=$(xwininfo -root -tree | grep '"marbble-plugin"' | grep -o '[0-9]*x[0-9]*+' | head -1 | tr -d +) || true
  wait $pid || true
  echo "$geometry"
}

failed=0
for name in "${PLUGINS[@]}"; do
  one=$(size "$name" GDK_SCALE=1)
  if [ -z "$one" ]; then
    if grep -q "plugin not found" "$SCRATCH/run.log"; then echo "SKIP  $name (not installed)"; continue; fi
    echo "FAIL  $name: no editor window at scale 1"; failed=1; continue
  fi
  two=$(size "$name" GDK_SCALE=2)
  w1=${one%x*} h1=${one#*x} w2=${two%x*} h2=${two#*x}
  # Within 2%: plug-ins round their own layouts.
  if [ -n "$two" ] && [ $((w2 * 100 / w1)) -ge 198 ] && [ $((w2 * 100 / w1)) -le 202 ] && [ $((h2 * 100 / h1)) -ge 198 ] && [ $((h2 * 100 / h1)) -le 202 ]; then
    echo "ok    $name: $one at scale 1, $two at scale 2"
  else
    echo "FAIL  $name: $one at scale 1, ${two:-no window} at scale 2"
    failed=1
  fi
done
exit $failed
