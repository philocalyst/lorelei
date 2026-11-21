#!/bin/bash
# What one row type costs to compile: user seconds and __text bytes of an example that lays out one row type,
# against one that lays out none. Run from the repository root; profile dist. The library is built first.
#   bench/r8/lay_compile.sh [profile]
set -euo pipefail
profile=${1:-dist}
log=$(mktemp)
trap 'rm -f "$log"' EXIT
cargo build --locked --profile "$profile" -p bough-lay --example one_none
for example in one_none one_sheet one_ide one_chat; do
  touch "crates/bough-lay/examples/$example.rs"
  if /usr/bin/time -p cargo build --locked --profile "$profile" -p bough-lay --example "$example" > "$log" 2>&1; then
    user_seconds=$(awk '$1 == "user" { print $2 }' "$log")
  else
    status=$?
    cat "$log" >&2
    exit "$status"
  fi
  binary="${CARGO_TARGET_DIR:-target}/$profile/examples/$example"
  if sections=$(size -m "$binary" 2>/dev/null); then
    text_bytes=$(awk '/__text/ { print $3; exit }' <<< "$sections")
  else
    sections=$(size -A "$binary")
    text_bytes=$(awk '$1 == ".text" { print $2; exit }' <<< "$sections")
  fi
  if [[ ! "$user_seconds" =~ ^[0-9]+[.][0-9]+$ || ! "$text_bytes" =~ ^[1-9][0-9]*$ ]]; then
    printf 'lay_compile: missing counter for %s\n' "$example" >&2
    exit 2
  fi
  file_bytes=$(wc -c < "$binary" | tr -d ' ')
  printf '%s\tuser_s=%s\ttext=%s\tfile=%s\n' "$example" "$user_seconds" "$text_bytes" "$file_bytes"
done
