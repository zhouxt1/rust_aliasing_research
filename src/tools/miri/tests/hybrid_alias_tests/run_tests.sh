#!/usr/bin/env bash
# Batch-runs one or more .rs files through HB-enabled Miri and reports a PASS/FAIL summary.
#
# Usage:
#   run_tests.sh [--std] [--flags "..."] file1.rs [file2.rs ...]
#   run_tests.sh [--std] [--flags "..."] --glob 'pattern/**/*.rs'
#
# By default runs with MIRI_NO_STD=1 (matches the hybrid_alias_tests/ corpus convention).
# Pass --std to run against the std sysroot instead (needed for upstream SB/TB test files
# that use Box, std::alloc, etc.). --flags lets you pass extra -Z flags (e.g.
# "-Zmiri-permissive-provenance"); never pass -Zmiri-tree-borrows / -Zmiri-disable-stacked-borrows
# here, since those override HB as the active borrow tracker.
#
# "PASS" means the program ran with no Undefined Behavior detected (build completed
# successfully). "FAIL" means Miri reported UB. This says nothing about whether that is the
# *expected* outcome for a given test file — check the file's own pass/ vs fail/ directory or
# doc comment for that.

set -u
cd "$(git rev-parse --show-toplevel)" || exit 1

NO_STD=1
EXTRA_FLAGS=""
FILES=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --std) NO_STD=0; shift ;;
    --flags) EXTRA_FLAGS="$2"; shift 2 ;;
    --glob) shift; while IFS= read -r f; do FILES+=("$f"); done < <(eval "ls -1 $1" 2>/dev/null); shift ;;
    *) FILES+=("$1"); shift ;;
  esac
done

if [[ ${#FILES[@]} -eq 0 ]]; then
  echo "No files given." >&2
  exit 1
fi

printf "%-70s %-12s %s\n" "TEST" "RESULT" "DETAIL"
printf "%-70s %-12s %s\n" "----" "------" "------"

for f in "${FILES[@]}"; do
  # x.py's --args takes exactly one token, so flags and the file path must be combined into
  # a single quoted string (a bare `--args "$FLAGS" "$f"` silently drops $f as an unmatched
  # bootstrap "run" path instead of forwarding it to miri).
  combined_args="$EXTRA_FLAGS $f"
  if [[ "$NO_STD" -eq 1 ]]; then
    out=$(MIRI_NO_STD=1 python3 x.py run miri --stage 1 --args "$combined_args" 2>&1)
  else
    out=$(python3 x.py run miri --stage 1 --args "$combined_args" 2>&1)
  fi

  if echo "$out" | grep -q "Build completed successfully"; then
    result="PASS"
    detail="-"
  else
    result="FAIL"
    detail=$(echo "$out" | grep -m1 "error: Undefined Behavior:\|error: unsupported operation:\|error\[" | sed 's/^error[^:]*: //' | cut -c1-80)
    [[ -z "$detail" ]] && detail="(build/compile error — see full output)"
  fi

  printf "%-70s %-12s %s\n" "$(basename "$f")" "$result" "$detail"
done
