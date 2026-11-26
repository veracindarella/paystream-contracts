#!/usr/bin/env bash
# OPS-18: Contract ABI/interface diff check.
#
# Generates a JSON interface spec from the compiled stream contract WASM using
# `stellar contract bindings` and diffs it against the committed baseline at
# contracts/stream/interface.json. Fails with a clear diff when the public
# interface (function signatures or event schemas) has changed.
#
# Usage:
#   scripts/check-interface.sh                  # verify interface + bindings
#   scripts/check-interface.sh --update         # regenerate the committed baseline
#   scripts/check-interface.sh --check-bindings # verify SDK bindings only
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACT_DIR="${ROOT_DIR}/contracts/stream"
BASELINE="${CONTRACT_DIR}/interface.json"
WASM="${CONTRACT_DIR}/target/wasm32-unknown-unknown/release/stream_contract.wasm"
BINDINGS_DIR="${CONTRACT_DIR}/bindings"

UPDATE=0
CHECK_BINDINGS_ONLY=0
if [[ "${1:-}" == "--update" ]]; then
  UPDATE=1
elif [[ "${1:-}" == "--check-bindings" ]]; then
  CHECK_BINDINGS_ONLY=1
fi

# Regenerate SDK bindings from the committed interface.json into a temp dir and
# diff them against the committed bindings directory. Fails on drift.
check_bindings() {
  local tmp_dir="$1"

  if [[ ! -f "${BASELINE}" ]]; then
    echo "error: interface baseline not found at ${BASELINE}" >&2
    echo "       run 'scripts/check-interface.sh --update' to create it" >&2
    exit 1
  fi

  if [[ ! -d "${BINDINGS_DIR}" ]]; then
    echo "error: committed bindings not found at ${BINDINGS_DIR}" >&2
    echo "       regenerate them from the committed interface.json and commit them" >&2
    exit 1
  fi

  stellar contract bindings typescript \
    --output-dir "${tmp_dir}/bindings" \
    --overwrite \
    < "${BASELINE}" >/dev/null

  if diff -ru "${BINDINGS_DIR}" "${tmp_dir}/bindings" >"${tmp_dir}/bindings-diff.txt"; then
    echo "SDK bindings match committed interface.json."
    return 0
  fi

  echo "" >&2
  echo "========================================================================" >&2
  echo " SDK BINDINGS DRIFT DETECTED" >&2
  echo "========================================================================" >&2
  echo "The committed SDK bindings at contracts/stream/bindings do not match the" >&2
  echo "bindings generated from contracts/stream/interface.json." >&2
  echo "" >&2
  echo "Diff (committed bindings vs. generated):" >&2
  echo "------------------------------------------------------------------------" >&2
  cat "${tmp_dir}/bindings-diff.txt" >&2
  echo "------------------------------------------------------------------------" >&2
  echo "" >&2
  echo "Regenerate the bindings from the committed interface.json and commit:" >&2
  echo "    stellar contract bindings typescript \\" >&2
  echo "        --output-dir contracts/stream/bindings --overwrite \\" >&2
  echo "        < contracts/stream/interface.json" >&2
  echo "" >&2
  exit 1
}

if ! command -v stellar >/dev/null 2>&1; then
  echo "error: 'stellar' CLI not found; install the Stellar CLI to run the interface check" >&2
  exit 1
fi

if [[ "${CHECK_BINDINGS_ONLY}" -eq 1 ]]; then
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "${TMP_DIR}"' EXIT
  check_bindings "${TMP_DIR}"
  exit 0
fi

if [[ ! -f "${WASM}" ]]; then
  echo "error: compiled WASM not found at ${WASM}" >&2
  echo "       build the contract first (e.g. 'cargo build --target wasm32-unknown-unknown --release' in ${CONTRACT_DIR})" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

# Generate the interface spec from the compiled WASM.
stellar contract bindings json \
  --wasm "${WASM}" \
  --output "${TMP_DIR}/interface.json" >/dev/null

GENERATED="${TMP_DIR}/interface.json"

if [[ "${UPDATE}" -eq 1 ]]; then
  cp "${GENERATED}" "${BASELINE}"
  echo "Updated interface baseline: ${BASELINE}"
  exit 0
fi

if [[ ! -f "${BASELINE}" ]]; then
  echo "error: interface baseline not found at ${BASELINE}" >&2
  echo "       run 'scripts/check-interface.sh --update' to create it" >&2
  exit 1
fi

if diff -u "${BASELINE}" "${GENERATED}" >"${TMP_DIR}/diff.txt"; then
  echo "Contract interface matches committed baseline."
  check_bindings "${TMP_DIR}"
  exit 0
fi

echo "" >&2
echo "========================================================================" >&2
echo " CONTRACT INTERFACE CHANGED" >&2
echo "========================================================================" >&2
echo "The public interface of the stream contract differs from the committed" >&2
echo "baseline at contracts/stream/interface.json. This is a breaking change" >&2
echo "for clients unless intentional." >&2
echo "" >&2
echo "Diff (committed baseline vs. generated):" >&2
echo "------------------------------------------------------------------------" >&2
cat "${TMP_DIR}/diff.txt" >&2
echo "------------------------------------------------------------------------" >&2
echo "" >&2
echo "If this change is intentional, update the baseline and commit it:" >&2
echo "    scripts/check-interface.sh --update" >&2
echo "    git add contracts/stream/interface.json" >&2
echo "" >&2
exit 1
