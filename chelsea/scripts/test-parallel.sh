#!/usr/bin/env bash
#
# test-parallel.sh — Run Eagle E2E tests in parallel by file.
#
# Each .eagle test file is independent: creates its own VMs, cleans up after
# itself, and has no cross-file state. We run them as separate processes with
# controlled concurrency to avoid overwhelming the single-node environment.
#
# Usage:
#   ./scripts/test-parallel.sh              # default: 3 concurrent jobs
#   ./scripts/test-parallel.sh --jobs 4     # custom concurrency
#   ./scripts/test-parallel.sh --full       # run all tests (no skips)
#
set -uo pipefail
cd "$(dirname "$0")/../"

# ─── Configuration ────────────────────────────────────────────────────────────

MAX_JOBS=3
QUICK_MODE=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --jobs) MAX_JOBS="$2"; shift 2 ;;
    --full) QUICK_MODE=false; shift ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

EAGLE_SHELL="./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll"

if [[ ! -f "$EAGLE_SHELL" ]]; then
  echo "ERROR: EagleShell.dll not found. Run 'make clone-and-build-eagle' first."
  exit 1
fi

# ─── Skip patterns (matching test-only-quick) ────────────────────────────────
#
# These tests are skipped in quick mode:
#   chelsea-103.0, chelsea-104.0, chelsea-105.0  (slow internal VM tests)
#   chelsea-2??.0                                  (public API proxy variants)
#   chelsea-407.0                                  (SSH key via public API)
#   domains-1??.0                                  (all domain tests)

# Files to skip entirely in quick mode
SKIP_FILES_QUICK=" domains.eagle "

# Per-file skip patterns in quick mode
skip_args_for() {
  local file="$1"
  if ! $QUICK_MODE; then
    return
  fi
  case "$file" in
    main.eagle)
      echo '-skip "chelsea-103.0 chelsea-104.0 chelsea-105.0 chelsea-2??.0"'
      ;;
    api_extra.eagle)
      echo '-skip "chelsea-407.0"'
      ;;
  esac
}

# ─── Collect test files ──────────────────────────────────────────────────────
# validate.eagle first (pure utility, no VMs — finishes fast)

ALL_FILES=(
  validate.eagle
  main.eagle
  exec.eagle
  commits.eagle
  commit_tags.eagle
  list_parent_commits.eagle
  api_extra.eagle
  images.eagle
  domains.eagle
)

RUN_FILES=()
for f in "${ALL_FILES[@]}"; do
  if $QUICK_MODE && [[ "$SKIP_FILES_QUICK" == *" $f "* ]]; then
    echo "SKIP  $f (quick mode)"
    continue
  fi
  RUN_FILES+=("$f")
done

# ─── Run tests in parallel ───────────────────────────────────────────────────

LOG_DIR=$(mktemp -d /tmp/eagle-parallel.XXXXXX)
declare -A PID_TO_FILE
ACTIVE_PIDS=()
FAILED=0
PASSED=0

echo ""
echo "Running ${#RUN_FILES[@]} test files (max $MAX_JOBS concurrent)"
echo "Logs: $LOG_DIR"
echo ""

# Clean up test environment
rm -rf ./target/release/.env 2>/dev/null || true
rm -rf ./target/debug/.env 2>/dev/null || true

run_file() {
  local file="$1"
  local log_file="$LOG_DIR/${file%.eagle}.log"
  local extra_args
  extra_args=$(skip_args_for "$file")

  echo "  ▶ $file"
  eval EAGLE_TEST_TEMP=/tmp dotnet exec --roll-forward Major \
    "$EAGLE_SHELL" -file "./tests/$file" \
    -stopOnFailure true \
    $extra_args \
    > "$log_file" 2>&1 &

  local pid=$!
  PID_TO_FILE[$pid]="$file"
  ACTIVE_PIDS+=("$pid")
}

# Wait for any one job to finish, report its result
reap_one() {
  while true; do
    for i in "${!ACTIVE_PIDS[@]}"; do
      local pid="${ACTIVE_PIDS[$i]}"
      if ! kill -0 "$pid" 2>/dev/null; then
        local status=0
        wait "$pid" || status=$?
        local file="${PID_TO_FILE[$pid]}"

        if [[ $status -eq 0 ]]; then
          echo "  ✓ $file"
          ((PASSED++)) || true
        else
          echo "  ✗ $file (exit $status) — see $LOG_DIR/${file%.eagle}.log"
          ((FAILED++)) || true
        fi

        unset 'ACTIVE_PIDS[i]'
        unset 'PID_TO_FILE[$pid]'
        return
      fi
    done
    sleep 0.5
  done
}

START_TIME=$SECONDS

for f in "${RUN_FILES[@]}"; do
  # Wait for a slot if at max concurrency
  while (( ${#ACTIVE_PIDS[@]} >= MAX_JOBS )); do
    reap_one
  done
  run_file "$f"
done

# Wait for all remaining jobs
while (( ${#ACTIVE_PIDS[@]} > 0 )); do
  reap_one
done

ELAPSED=$(( SECONDS - START_TIME ))

# ─── Summary ─────────────────────────────────────────────────────────────────

echo ""
echo "════════════════════════════════════════════════"
echo "  ${PASSED} passed, ${FAILED} failed  (${ELAPSED}s)"
echo "════════════════════════════════════════════════"

if (( FAILED > 0 )); then
  echo ""
  echo "Failed test logs:"
  for log in "$LOG_DIR"/*.log; do
    if grep -ql "FAILED\|ERRORS WERE REPORTED" "$log" 2>/dev/null; then
      echo "  $log"
    fi
  done
  exit 1
fi
