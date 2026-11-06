#!/usr/bin/env bash
# Compiles the EIP-8141 Yul accounts to yul/build/<Name>.bin (hex initcode, no trailing newline).
#
#   script/build-yul.sh           write yul/build/*.bin
#   script/build-yul.sh --check   compile and fail if any output differs from yul/build/*.bin
#
# The committed bins are the deployed initcode, so they must stay byte-identical. They are produced by
#   solc 0.8.28 --strict-assembly --evm-version prague --optimize --bin yul/<Name>.yul
# (0.8.27 and 0.8.30 give the same bytes; 0.8.28 is pinned to match foundry.toml).
#
# solc is found, in order, at: $SOLC; `solc` on PATH if it reports 0.8.28; the svm cache that
# `forge build` fills (~/.svm, $XDG_DATA_HOME/svm, %APPDATA%/svm). Run `forge build` once to install it.
set -euo pipefail

VERSION="0.8.28"
NAMES=(PoolAccount PQPaymaster ClassicPaymaster)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="write"
case "${1:-}" in
  "") ;;
  --check) MODE="check" ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

find_solc() {
  if [[ -n "${SOLC:-}" ]]; then echo "$SOLC"; return; fi
  if command -v solc >/dev/null 2>&1 && solc --version 2>/dev/null | grep -q "Version: ${VERSION}+"; then
    command -v solc; return
  fi
  local dir
  for dir in "$HOME/.svm" "${XDG_DATA_HOME:-$HOME/.local/share}/svm" "${APPDATA:-}/svm"; do
    for exe in "$dir/$VERSION/solc-$VERSION" "$dir/$VERSION/solc-$VERSION.exe"; do
      if [[ -x "$exe" ]]; then echo "$exe"; return; fi
    done
  done
  echo "solc $VERSION not found: run 'forge build' (installs it via svm) or set SOLC" >&2
  exit 1
}

SOLC_BIN="$(find_solc)"
if ! "$SOLC_BIN" --version | grep -q "Version: ${VERSION}+"; then
  echo "expected solc $VERSION, got: $("$SOLC_BIN" --version | tail -1)" >&2
  exit 1
fi

mkdir -p "$ROOT/yul/build"
status=0
for name in "${NAMES[@]}"; do
  bin="$("$SOLC_BIN" --strict-assembly --evm-version prague --optimize --bin "$ROOT/yul/$name.yul" \
    | awk '/^Binary representation:/ { getline; print; exit }' | tr -d '\r\n')"
  if [[ -z "$bin" ]]; then
    echo "$name: no bytecode in solc output" >&2
    exit 1
  fi
  out="$ROOT/yul/build/$name.bin"
  if [[ "$MODE" == "check" ]]; then
    if [[ -f "$out" && "$(tr -d '\r\n' < "$out")" == "$bin" ]]; then
      echo "ok       $name ($(( ${#bin} / 2 )) bytes)"
    else
      echo "MISMATCH $name: yul/build/$name.bin differs from the compiled yul/$name.yul" >&2
      status=1
    fi
  else
    printf '%s' "$bin" > "$out"
    echo "wrote    yul/build/$name.bin ($(( ${#bin} / 2 )) bytes)"
  fi
done
exit $status
