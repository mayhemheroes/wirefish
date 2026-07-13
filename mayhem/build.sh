#!/usr/bin/env bash
#
# mayhem/build.sh — build this repo's cargo-fuzz target(s) as sanitized libFuzzer
# binaries (OSS-Fuzz Rust path: cargo-fuzz + ASan via RUSTFLAGS). EDIT per repo.
#
# Runs inside the commit image (RUST mayhem/Dockerfile) as `mayhem` in /mayhem.
# The Rust toolchain + cargo registry live at $CARGO_HOME=/opt/toolchains/rust/cargo
# (pinned by the Dockerfile ENV — absolute, $HOME-independent).
#
# AIR-GAPPED CONTRACT (SPEC §6.5): the PATCH tier re-runs THIS script OFFLINE.
#   - This FIRST build (in CI, online) populates the cargo registry under $CARGO_HOME.
#   - The PATCH re-run resolves crates from that cache. The rlenv runtime exports
#     CARGO_NET_OFFLINE=true for the re-run so cargo won't try to refresh the
#     crates.io index over the (absent) network — so do NOT hard-code `--offline`
#     here (it would break this first, online build).
#   - For a FULLY self-contained image (no runtime flag needed) instead vendor:
#       cargo vendor --versioned-dirs vendor   # commit vendor/ + a .cargo/config.toml
#     with [source.crates-io] replace-with = "vendored-sources".
set -euo pipefail

# clang rejects SOURCE_DATE_EPOCH='' — must be unset or a valid integer.
[ -n "${SOURCE_DATE_EPOCH:-}" ] || unset SOURCE_DATE_EPOCH

: "${MAYHEM_JOBS:=$(nproc)}"
# cargo-fuzz has no --jobs flag; cargo reads parallelism from CARGO_BUILD_JOBS.
export CARGO_BUILD_JOBS="$MAYHEM_JOBS"

cd "$SRC"

# OSS-Fuzz Rust libFuzzer+ASan flags. cargo-fuzz sets the ASan flag itself, but we
# pin it explicitly. --cfg fuzzing matches libfuzzer-sys; force-frame-pointers aids
# ASan backtraces. The rlenv PATCH tier prepends `-C debuginfo=2`; we don't fight it.
# Sanitizer off-switch parity with the C $SANITIZER_FLAGS contract: an explicitly
# EMPTY SANITIZER_FLAGS builds with no sanitizers (natural crash). rustc ignores
# clang flags, so map it onto the cargo-fuzz/RUSTFLAGS equivalents.
SANITIZER_FLAGS=${SANITIZER_FLAGS=-fsanitize=address}
if [ -n "$SANITIZER_FLAGS" ]; then
  RUST_SAN_FLAGS="-Zsanitizer=address"
  CARGO_FUZZ_SAN=(-s address)
else
  RUST_SAN_FLAGS=""
  CARGO_FUZZ_SAN=(-s none)
fi

# DWARF must stay < 4 (SPEC §6.2 item 10): rustc and clang-19 emit DWARF-5 by
# default. Pin rustc-compiled code (incl. -Zbuild-std std) and the libfuzzer C++
# runtime (compiled by libfuzzer-sys's cc build script) to DWARF-3.
export RUST_DEBUG_FLAGS="${RUST_DEBUG_FLAGS:--Cdebuginfo=1 -Zdwarf-version=3}"
export RUSTFLAGS="${RUSTFLAGS:-} ${RUST_DEBUG_FLAGS} $RUST_SAN_FLAGS --cfg fuzzing -Cforce-frame-pointers"
export CFLAGS="${CFLAGS:-} -gdwarf-3"
export CXXFLAGS="${CXXFLAGS:-} -gdwarf-3"

# EDIT: the cargo-fuzz crate directory. Use upstream's own fuzz/ when it builds on
# the pinned nightly; otherwise add an ADDITIVE mayhem/fuzz/ crate (leaves upstream
# untouched) and point --fuzz-dir at it.
FUZZ_DIR="mayhem/fuzz"
TRIPLE="x86_64-unknown-linux-gnu"

# Discover every target from the crate's fuzz_targets/ dir (one binary per target).
FUZZ_TARGETS=()
for f in "$FUZZ_DIR"/fuzz_targets/*.rs; do
  FUZZ_TARGETS+=("$(basename "${f%.*}")")
done
[ "${#FUZZ_TARGETS[@]}" -gt 0 ] || { echo "ERROR: no fuzz targets under $FUZZ_DIR/fuzz_targets/" >&2; exit 1; }

# The rustc nightly ships a PRECOMPILED ASan runtime (librustc-*_rt.asan.a) whose
# compiler-rt CUs carry DWARF-5 (clang default); those runtime CUs would land in the
# linked fuzz binary and (being emitted first) fail the DWARF < 4 gate. Triage does not
# need runtime debug symbols; strip them (the archive is writable, chowned to 2000).
for _asan in $(find /opt/toolchains/rust -name "librustc-*_rt.asan.a" 2>/dev/null); do
  echo "stripping DWARF-5 debug info from runtime archive: $_asan"
  objcopy --strip-debug "$_asan" "$_asan.tmp" && mv "$_asan.tmp" "$_asan"
done

echo "=== cargo fuzz build (image nightly, ASan via RUSTFLAGS) ==="
echo "RUSTFLAGS=$RUSTFLAGS"
echo "targets: ${FUZZ_TARGETS[*]}"

# Use the image's DEFAULT toolchain (the Dockerfile pinned it). A `+toolchain`
# override would make rustup try to install another channel into the locked /opt/rust.
for t in "${FUZZ_TARGETS[@]}"; do
  echo "--- building fuzz target: $t ---"
  # cargo-fuzz locates the host project by walking up from CWD; the cargo project
  # lives in src-tauri/, so run from there and point --fuzz-dir at the fuzz crate.
  (cd src-tauri && cargo fuzz build --fuzz-dir "$SRC/$FUZZ_DIR" "${CARGO_FUZZ_SAN[@]}" -O --debug-assertions "$t")
  bin="$SRC/$FUZZ_DIR/target/$TRIPLE/release/$t"
  [ -x "$bin" ] || { echo "ERROR: expected fuzz binary not found at $bin" >&2; exit 1; }
  cp "$bin" "/mayhem/$t"     # EDIT the output path/name to match your Mayhemfile target:
  echo "built /mayhem/$t"
done

# Build the project's TEST suite too — with the project's NORMAL flags (a clean,
# non-sanitized build) — so mayhem/test.sh only RUNS it. The sniffer_parser crate
# holds the upstream unit tests; the top-level tauri crate needs GUI system libs
# (webkit2gtk) and an unresolvable git dependency, so its tests are not built here.
echo "=== building upstream test suite (normal flags) ==="
# sniffer_parser's implicit cargo workspace root is src-tauri/Cargo.toml (the GUI
# crate), whose dependency graph is unresolvable here (live-git tauri-plugin-log,
# webkit2gtk/pcap system libs). Stage a copy with its own [workspace] so the test
# suite builds standalone against the same sources.
TEST_SRC="$SRC/mayhem/test-src"
rm -rf "$TEST_SRC"
mkdir -p "$TEST_SRC"
cp -r src-tauri/sniffer_parser "$TEST_SRC/sniffer_parser"
grep -q '^\[workspace\]' "$TEST_SRC/sniffer_parser/Cargo.toml" || printf '\n[workspace]\n' >> "$TEST_SRC/sniffer_parser/Cargo.toml"
# Pinned lockfile so the offline (air-gapped) re-run resolves from the image's
# cargo cache instead of the crates.io index.
cp mayhem/sniffer_parser-Cargo.lock "$TEST_SRC/sniffer_parser/Cargo.lock"
env -u RUSTFLAGS cargo test --no-run \
  --manifest-path "$TEST_SRC/sniffer_parser/Cargo.toml" \
  --target-dir "$SRC/mayhem/test-build"

echo "build.sh complete"
