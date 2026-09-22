#!/usr/bin/env bash
set -euo pipefail

programs=(task pnpm node cargo)
missing=()
for prog in "${programs[@]}"; do
  command -v "$prog" >/dev/null || missing+=("$prog")
done

if [ ${#missing[@]} -gt 0 ]; then
  echo "Missing: ${missing[*]}" >&2
  exit 1
fi
