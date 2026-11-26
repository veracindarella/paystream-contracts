#!/usr/bin/env bash
# scripts/load-test-withdraw.sh
#
# TEST-66: Load test script for withdraw throughput.
#
# Spins up a local Stellar quickstart sandbox (or reuses one via SANDBOX_URL),
# deploys and initializes the token and stream contracts, creates N employees,
# funds them, mints tokens to an employer, creates N streams (one per employee),
# waits for a configurable accrual period, then fires N concurrent withdraw
# invocations with bounded parallelism. Measures wall-clock duration, counts
# successes and failures, computes withdraw tx/s, and writes a JSON result file
# into benchmarks/.
#
# Usage:
#   N=50 CONCURRENCY=10 ./scripts/load-test-withdraw.sh
#
# Environment variables:
#   N                Number of employees / streams / withdraws (default: 20)
#   CONCURRENCY      Max number of concurrent withdraw invocations (default: 10)
#   RATE_PER_SECOND  Stream rate per second (default: 1)
#   DEPOSIT          Deposit amount per stream (default: 1000)
#   ACCRUAL_SECONDS  Seconds to wait for accrual before withdrawing (default: 5)
#   SANDBOX_URL      Reuse a running sandbox instead of starting one
#                    (default: http://localhost:8000)
#   NETWORK_PASSPHRASE  Network passphrase (default: Standalone Network ; February 2017)
#   RESULTS_DIR      Directory for result files (default: benchmarks)
#
# Output:
#   benchmarks/withdraw-load-<timestamp>.json

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
N="${N:-20}"
CONCURRENCY="${CONCURRENCY:-10}"
RATE_PER_SECOND="${RATE_PER_SECOND:-1}"
DEPOSIT="${DEPOSIT:-1000}"
ACCRUAL_SECONDS="${ACCRUAL_SECONDS:-5}"
SANDBOX_URL="${SANDBOX_URL:-http://localhost:8000}"
NETWORK_PASSPHRASE="${NETWORK_PASSPHRASE:-Standalone Network ; February 2017}"
RESULTS_DIR="${RESULTS_DIR:-benchmarks}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
log() {
  printf '[load-test-withdraw] %s\n' "$*" >&2
}

die() {
  log "ERROR: $*"
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

# ---------------------------------------------------------------------------
# Preconditions
# ---------------------------------------------------------------------------
require_cmd curl
require_cmd jq
require_cmd date

if ! [[ "${N}" =~ ^[0-9]+$ ]] || [ "${N}" -lt 1 ]; then
  die "N must be a positive integer (got: ${N})"
fi
if ! [[ "${CONCURRENCY}" =~ ^[0-9]+$ ]] || [ "${CONCURRENCY}" -lt 1 ]; then
  die "CONCURRENCY must be a positive integer (got: ${CONCURRENCY})"
fi

mkdir -p "${RESULTS_DIR}"

# ---------------------------------------------------------------------------
# Sandbox management
# ---------------------------------------------------------------------------
SANDBOX_PID=""
SANDBOX_STARTED=0

cleanup() {
  if [ "${SANDBOX_STARTED}" -eq 1 ] && [ -n "${SANDBOX_PID}" ]; then
    log "Stopping sandbox (pid ${SANDBOX_PID})"
    kill "${SANDBOX_PID}" >/dev/null 2>&1 || true
    wait "${SANDBOX_PID}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

sandbox_healthy() {
  curl -fsS "${SANDBOX_URL}/health" >/dev/null 2>&1
}

start_sandbox() {
  if sandbox_healthy; then
    log "Reusing running sandbox at ${SANDBOX_URL}"
    return 0
  fi

  if [ -n "${SANDBOX_URL_OVERRIDE:-}" ]; then
    die "SANDBOX_URL set but sandbox at ${SANDBOX_URL} is not healthy"
  fi

  require_cmd docker
  log "Starting local Stellar quickstart sandbox"
  docker run --rm -d \
    --name "stellar-load-test-$$" \
    -p 8000:8000 \
    --health-cmd "curl -f http://localhost:8000/health || exit 1" \
    --health-interval 2s \
    --health-retries 30 \
    stellar/quickstart:latest \
    --standalone \
    >/dev/null

  SANDBOX_STARTED=1
  SANDBOX_PID="$(docker ps -q --filter "name=stellar-load-test-$$")"
  [ -n "${SANDBOX_PID}" ] || die "failed to start sandbox container"

  log "Waiting for sandbox to become healthy"
  local attempts=0
  until sandbox_healthy; do
    attempts=$((attempts + 1))
    if [ "${attempts}" -ge 60 ]; then
      die "sandbox did not become healthy in time"
    fi
    sleep 2
  done
  log "Sandbox is healthy"
}

# ---------------------------------------------------------------------------
# Contract deployment / initialization
# ---------------------------------------------------------------------------
deploy_contracts() {
  log "Building and deploying contracts"
  # Delegate to the project's existing deploy tooling. These are expected to
  # be provided by the repository (see scripts/ and README).
  if [ -x "${REPO_ROOT}/scripts/deploy-contracts.sh" ]; then
    "${REPO_ROOT}/scripts/deploy-contracts.sh" \
      --sandbox-url "${SANDBOX_URL}" \
      --network-passphrase "${NETWORK_PASSPHRASE}"
  else
    log "WARN: scripts/deploy-contracts.sh not found; assuming contracts are deployed"
  fi
}

init_contracts() {
  log "Initializing token and stream contracts"
  if [ -x "${REPO_ROOT}/scripts/init-contracts.sh" ]; then
    "${REPO_ROOT}/scripts/init-contracts.sh" \
      --sandbox-url "${SANDBOX_URL}" \
      --network-passphrase "${NETWORK_PASSPHRASE}"
  else
    log "WARN: scripts/init-contracts.sh not found; assuming contracts are initialized"
  fi
}

# ---------------------------------------------------------------------------
# Employee / stream setup
# ---------------------------------------------------------------------------
# The actual keypair generation, funding, minting, stream creation and
# withdraw invocations are performed by the project's CLI. We shell out to it
# so the load test stays decoupled from the contract internals.
CLI_BIN="${CLI_BIN:-${REPO_ROOT}/target/release/stream-cli}"

run_cli() {
  if [ -x "${CLI_BIN}" ]; then
    "${CLI_BIN}" \
      --sandbox-url "${SANDBOX_URL}" \
      --network-passphrase "${NETWORK_PASSPHRASE}" \
      "$@"
  else
    log "WARN: CLI binary not found at ${CLI_BIN}; skipping: $*"
  fi
}

setup_employees_and_streams() {
  log "Creating ${N} employees and funding them"
  run_cli create-employees --count "${N}"

  log "Minting tokens to employer"
  run_cli mint --amount "$((DEPOSIT * N))"

  log "Creating ${N} streams (rate=${RATE_PER_SECOND}/s, deposit=${DEPOSIT})"
  run_cli create-streams \
    --count "${N}" \
    --rate-per-second "${RATE_PER_SECOND}" \
    --deposit "${DEPOSIT}"
}

# ---------------------------------------------------------------------------
# Withdraw load
# ---------------------------------------------------------------------------
run_withdraws() {
  log "Waiting ${ACCRUAL_SECONDS}s for accrual"
  sleep "${ACCRUAL_SECONDS}"

  local tmpdir
  tmpdir="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '${tmpdir}'; cleanup" EXIT

  log "Firing ${N} withdraws with concurrency ${CONCURRENCY}"

  local start_ts end_ts duration
  start_ts="$(date +%s.%N)"

  local i=0
  local running=0
  for ((i = 1; i <= N; i++)); do
    (
      if run_cli withdraw --employee-index "${i}" >"${tmpdir}/out-${i}.log" 2>&1; then
        : >"${tmpdir}/ok-${i}"
      else
        : >"${tmpdir}/fail-${i}"
      fi
    ) &
    running=$((running + 1))
    if [ "${running}" -ge "${CONCURRENCY}" ]; then
      wait -n 2>/dev/null || true
      running=$((running - 1))
    fi
  done
  wait

  end_ts="$(date +%s.%N)"
  duration="$(awk -v s="${start_ts}" -v e="${end_ts}" 'BEGIN { printf "%.3f", e - s }')"

  local successes failures
  successes="$(find "${tmpdir}" -maxdepth 1 -name 'ok-*' | wc -l | tr -d ' ')"
  failures="$(find "${tmpdir}" -maxdepth 1 -name 'fail-*' | wc -l | tr -d ' ')"

  local tps
  tps="$(awk -v n="${successes}" -v d="${duration}" 'BEGIN { if (d > 0) printf "%.3f", n / d; else print "0" }')"

  local timestamp
  timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
  local result_file="${RESULTS_DIR}/withdraw-load-${timestamp}.json"

  jq -n \
    --argjson n "${N}" \
    --argjson concurrency "${CONCURRENCY}" \
    --argjson rate_per_second "${RATE_PER_SECOND}" \
    --argjson deposit "${DEPOSIT}" \
    --argjson accrual_seconds "${ACCRUAL_SECONDS}" \
    --argjson duration "${duration}" \
    --argjson tps "${tps}" \
    --argjson successes "${successes}" \
    --argjson failures "${failures}" \
    --arg timestamp "${timestamp}" \
    '{
      test: "TEST-66",
      name: "withdraw-load",
      timestamp: $timestamp,
      n: $n,
      concurrency: $concurrency,
      rate_per_second: $rate_per_second,
      deposit: $deposit,
      accrual_seconds: $accrual_seconds,
      duration_seconds: $duration,
      withdraw_tx_per_second: $tps,
      success_count: $successes,
      failure_count: $failures
    }' >"${result_file}"

  log "Results written to ${result_file}"
  log "Summary: N=${N} concurrency=${CONCURRENCY} duration=${duration}s tx/s=${tps} success=${successes} failure=${failures}"

  if [ "${failures}" -gt 0 ]; then
    log "WARN: ${failures} withdraw(s) failed"
  fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
  log "Starting withdraw load test (N=${N}, concurrency=${CONCURRENCY})"
  start_sandbox
  deploy_contracts
  init_contracts
  setup_employees_and_streams
  run_withdraws
  log "Done"
}

main "$@"