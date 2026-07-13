#!/usr/bin/env bash
#
# mayhem/test.sh — RUN this repo's OWN functional test suite (already built by mayhem/build.sh).
# exit 0 = pass. EDIT per repo. PATCH-grade oracle: after an agent patches the source, the grader
# rebuilds (build.sh) then runs this. DELETE this file if the repo has no meaningful tests.
#
# IMPORTANT:
#  * Must assert BEHAVIOR/OUTPUT, not just exit status. The oracle has to check asserted values /
#    golden-output diffs / known-answer results — so a PATCH that "fixes" a bug by making the program
#    exit(0) (or any no-op) FAILS here. Running inputs and checking only "exit 0 / didn't crash" is
#    NOT a functional test (it's trivially reward-hackable) — use the project's real assertion suite.
#  * Do NOT build here — mayhem/build.sh already compiled the test suite (with the project's normal
#    flags). This script only RUNS the pre-built tests and reports counts. If the test runner is
#    missing, that's a build.sh bug — fail loudly rather than silently rebuilding.
#  * REQUIRED OUTPUT — a CTRF (https://ctrf.io) summary so Mayhem/the PATCH grader reads the counts:
#      - writes a CTRF JSON report to ${CTRF_REPORT:-$SRC/ctrf-report.json}, and
#      - prints a one-line `CTRF {...}` marker to stdout (same JSON, compact).
#    Only `results.summary` (with tests/passed/failed/pending/skipped/other) is required.
#    Use the emit_ctrf helper below; it computes tests = passed+failed+skipped and sets the exit
#    code (0 iff failed==0). Map your framework's output to passed/failed/skipped.
set -uo pipefail
[ -n "${SOURCE_DATE_EPOCH:-}" ] || unset SOURCE_DATE_EPOCH
: "${MAYHEM_JOBS:=$(nproc)}"   # build parallelism; env-overridable, falls back to nproc (use -j"$MAYHEM_JOBS")
cd "$SRC"

# emit_ctrf <tool> <passed> <failed> [skipped] [pending] [other]
# Writes a CTRF report (file + stdout `CTRF {...}` marker) and returns non-zero iff failed>0.
emit_ctrf() {
  local tool="$1" passed="$2" failed="$3" skipped="${4:-0}" pending="${5:-0}" other="${6:-0}"
  local tests=$(( passed + failed + skipped + pending + other ))
  cat > "${CTRF_REPORT:-$SRC/ctrf-report.json}" <<JSON
{
  "results": {
    "tool": { "name": "$tool" },
    "summary": {
      "tests": $tests,
      "passed": $passed,
      "failed": $failed,
      "pending": $pending,
      "skipped": $skipped,
      "other": $other
    }
  }
}
JSON
  printf 'CTRF {"results":{"tool":{"name":"%s"},"summary":{"tests":%d,"passed":%d,"failed":%d,"pending":%d,"skipped":%d,"other":%d}}}\n' \
    "$tool" "$tests" "$passed" "$failed" "$pending" "$skipped" "$other"
  [ "$failed" -eq 0 ]
}

# Run the upstream sniffer_parser unit-test suite that mayhem/build.sh pre-built
# (cargo test --no-run into mayhem/test-build). cargo re-uses that cache, so this
# only RUNS the tests. The top-level tauri GUI crate's tests are not runnable in
# this image (webkit2gtk system libs + an unresolvable live git dependency).
[ -d "$SRC/mayhem/test-build" ] || { echo "ERROR: mayhem/test-build missing — build.sh must build the test suite" >&2; emit_ctrf "cargo-test" 0 1 0; exit 1; }

OUT="$(env -u RUSTFLAGS CARGO_NET_OFFLINE=true cargo test \
        --manifest-path "$SRC/mayhem/test-src/sniffer_parser/Cargo.toml" \
        --target-dir "$SRC/mayhem/test-build" 2>&1)" || true
echo "$OUT"

PASSED=$(echo "$OUT" | grep -oP '^test result: \w+\. \K[0-9]+(?= passed)' | awk '{s+=$1} END {print s+0}')
FAILED=$(echo "$OUT" | grep -oP '(?<= passed; )[0-9]+(?= failed)' | awk '{s+=$1} END {print s+0}')
IGNORED=$(echo "$OUT" | grep -oP '(?<= failed; )[0-9]+(?= ignored)' | awk '{s+=$1} END {print s+0}')
PASSED=${PASSED:-0}; FAILED=${FAILED:-0}; IGNORED=${IGNORED:-0}
if [ "$PASSED" -eq 0 ] && [ "$FAILED" -eq 0 ]; then
  echo "ERROR: no test results parsed — suite did not run" >&2
  emit_ctrf "cargo-test" 0 1 0
  exit 1
fi
emit_ctrf "cargo-test" "$PASSED" "$FAILED" "$IGNORED"
