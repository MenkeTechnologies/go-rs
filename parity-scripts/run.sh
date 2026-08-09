#!/bin/bash
# Differential byte-parity harness: run every parity-scripts/**/*.go through the
# reference `go run` (oracle) and the freshly-built go-rs `go`, and assert their
# stdout is byte-identical (and success/failure agrees). Dev tool — needs the
# real `go` toolchain on PATH. Prints the byte-parity rate and every divergence.
#
#   Usage: bash parity-scripts/run.sh [-v]     (-v shows the diff for each miss)
#
# NOTE: go-rs runs single-file `package main` programs against its built-in
# stdlib subset (fmt/strings/strconv/math/sort/os) with real goroutines/
# channels/select/closures. It has no module system, so `go get`/third-party
# imports are out of scope; the corpus is curated to the supported surface.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OURS="$ROOT/target/debug/go"
CORPUS="$ROOT/parity-scripts"
ORACLE="${GORS_PARITY_GO:-go}"
VERBOSE="${1:-}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

command -v "$ORACLE" >/dev/null || { echo "parity: no reference '$ORACLE' on PATH"; exit 2; }
[ -x "$OURS" ] || { echo "parity: $OURS not built (cargo build)"; exit 2; }

pass=0; fail=0
declare -a misses
while IFS= read -r f; do
  rel="${f#"$CORPUS"/}"
  timeout 30 "$ORACLE" run "$f" >"$TMP/g.out" 2>/dev/null; grc=$?
  timeout 30 "$OURS"   run "$f" >"$TMP/r.out" 2>/dev/null; rrc=$?
  ok_rc=0; { [ $grc -eq 0 ] && [ $rrc -eq 0 ]; } || { [ $grc -ne 0 ] && [ $rrc -ne 0 ]; } || ok_rc=1
  if cmp -s "$TMP/g.out" "$TMP/r.out" && [ $ok_rc -eq 0 ]; then
    pass=$((pass+1))
  else
    fail=$((fail+1)); misses+=("$rel|$grc|$rrc")
    if [ "$VERBOSE" = "-v" ]; then
      echo "=== DIFF $rel  (go rc=$grc, go-rs rc=$rrc) ==="
      diff "$TMP/g.out" "$TMP/r.out" | head -20
    fi
  fi
done < <(find "$CORPUS" -name '*.go' | sort)

total=$((pass+fail))
echo ""
echo "════════════════════════════════════════════"
echo "BYTE PARITY: $pass / $total match  (oracle: $ORACLE)"
echo "════════════════════════════════════════════"
if [ $fail -gt 0 ]; then
  echo "Divergences:"
  for m in "${misses[@]}"; do
    IFS='|' read -r rel grc rrc <<<"$m"
    echo "  DIFF  $rel  (go rc=$grc, go-rs rc=$rrc)"
  done
fi
# A run that compared nothing is not a pass. Without this, an empty or
# unreadable corpus prints "0 / 0 match" and exits 0 — a wiped tree reading as a
# clean gate.
if [ $total -eq 0 ]; then
  echo "parity: no cases ran — the corpus under $CORPUS is empty or unreadable"
  exit 2
fi
[ $fail -eq 0 ]
