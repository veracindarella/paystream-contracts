#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# export-streams.sh — export an employer's PayStream streams for payroll reconciliation
#
# Reads the streams an employer owns straight from the contract and writes a
# spreadsheet-friendly table (CSV) or a JSON array to stdout, so finance teams
# can reconcile on-chain payments against payroll records without writing an
# indexer.
#
# Usage:
#   ./scripts/export-streams.sh <CONTRACT_ID> <EMPLOYER> [options]
#
# Options:
#   --json               emit JSON instead of CSV
#   --network <name>     Stellar network to query (default: testnet)
#   --source <account>   Stellar CLI source account (default: the CLI's own default)
#   --page-size <n>      streams per contract read (default: 50, the contract maximum)
#   --output <file>      write to <file> instead of stdout
#   -h, --help           show this help
#
# Environment:
#   STREAM_CONTRACT_ID   default for <CONTRACT_ID>
#   EMPLOYER_ADDRESS     default for <EMPLOYER>
#   STELLAR_NETWORK      default for --network
#
# Output columns:
#   id,employee,token,deposit,withdrawn,claimable,status,start_time,stop_time,rate_per_second
#
# Example:
#   ./scripts/export-streams.sh CCRH4EPUVIPESWYWOWPQ2QK3XN6KBR3RY6UFK36A4MXKKXIFH6ONRTVY GABC... \
#     --network testnet --output streams.csv

set -euo pipefail

CONTRACT_ID="${STREAM_CONTRACT_ID:-}"
EMPLOYER="${EMPLOYER_ADDRESS:-}"
NETWORK="${STELLAR_NETWORK:-testnet}"
SOURCE=""
PAGE_SIZE=50
FORMAT="csv"
OUTPUT=""

log() { echo "[export-streams] $*" >&2; }
fail() { echo "[export-streams] ERROR: $*" >&2; exit 1; }

usage() {
    sed -n '3,33p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# Arguments
# ---------------------------------------------------------------------------
positional=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --json)
            FORMAT="json"
            shift
            ;;
        --network)
            [[ $# -ge 2 ]] || fail "--network needs a value"
            NETWORK="$2"
            shift 2
            ;;
        --source)
            [[ $# -ge 2 ]] || fail "--source needs a value"
            SOURCE="$2"
            shift 2
            ;;
        --page-size)
            [[ $# -ge 2 ]] || fail "--page-size needs a value"
            PAGE_SIZE="$2"
            shift 2
            ;;
        --output)
            [[ $# -ge 2 ]] || fail "--output needs a value"
            OUTPUT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            positional+=("$@")
            break
            ;;
        -*)
            fail "unknown option: $1"
            ;;
        *)
            positional+=("$1")
            shift
            ;;
    esac
done

if [[ ${#positional[@]} -gt 0 ]]; then
    CONTRACT_ID="${positional[0]}"
fi
if [[ ${#positional[@]} -gt 1 ]]; then
    EMPLOYER="${positional[1]}"
fi

[[ -n "$CONTRACT_ID" ]] || fail "a contract ID is required (argument 1, or STREAM_CONTRACT_ID)"
[[ -n "$EMPLOYER" ]] || fail "an employer address is required (argument 2, or EMPLOYER_ADDRESS)"
[[ "$PAGE_SIZE" =~ ^[0-9]+$ ]] || fail "--page-size must be a positive number"
[[ "$PAGE_SIZE" -gt 0 ]] || fail "--page-size must be a positive number"
if [[ "$PAGE_SIZE" -gt 50 ]]; then
    log "page size $PAGE_SIZE exceeds the contract maximum of 50, using 50"
    PAGE_SIZE=50
fi

# ---------------------------------------------------------------------------
# Dependencies
# ---------------------------------------------------------------------------
command -v stellar > /dev/null 2>&1 || fail "the Stellar CLI (stellar) is required"
command -v jq > /dev/null 2>&1 || fail "jq is required to decode the contract responses"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
RECORDS="$TMP_DIR/records.jsonl"

# ---------------------------------------------------------------------------
# Contract calls
# ---------------------------------------------------------------------------

# invoke <function> [args...] — print the decoded contract result as JSON.
invoke() {
    local out payload
    local -a cmd=(stellar contract invoke --id "$CONTRACT_ID" --network "$NETWORK" --send=no)
    [[ -n "$SOURCE" ]] && cmd+=(--source "$SOURCE")
    cmd+=(-- "$@")

    if ! out=$("${cmd[@]}" 2>&1); then
        log "contract call failed: ${*: -1}"
        printf '%s\n' "$out" >&2
        return 1
    fi

    # The CLI prints the decoded result as a single JSON document; log lines
    # may precede it, so fall back to picking the last JSON-looking line.
    payload=$(printf '%s\n' "$out" | jq -c 'select(type != null)' 2>/dev/null | tail -n 1 || true)
    if [[ -z "$payload" ]]; then
        payload=$(printf '%s\n' "$out" | jq -R 'fromjson? // empty' 2>/dev/null | tail -n 1 || true)
    fi
    if [[ -z "$payload" ]]; then
        log "could not parse the contract response for: ${*: -1}"
        printf '%s\n' "$out" >&2
        return 1
    fi
    printf '%s\n' "$payload"
}

# ---------------------------------------------------------------------------
# Step 1 — the employer's stream IDs, paginated
# ---------------------------------------------------------------------------
# Uses the paginated streams_by_employer_page query so large employers do not
# exhaust instruction limits. Pages are read until an empty page is returned.
log "reading streams for employer $EMPLOYER on $NETWORK..."
STREAM_IDS=()
cursor=""
page=1
while :; do
    if [[ -z "$cursor" ]]; then
        page_json=$(invoke streams_by_employer_page \
            --employer "$EMPLOYER" --start_after null --limit "$PAGE_SIZE") ||
            fail "could not read the employer's streams"
    else
        page_json=$(invoke streams_by_employer_page \
            --employer "$EMPLOYER" --start_after "$cursor" --limit "$PAGE_SIZE") ||
            fail "could not read the employer's streams"
    fi

    mapfile -t page_ids < <(printf '%s' "$page_json" | jq -r 'if type == "array" then .[] else . end | tostring')

    if [[ ${#page_ids[@]} -eq 0 ]]; then
        break
    fi

    log "read page $page (${#page_ids[@]} stream ID(s))..."
    STREAM_IDS+=("${page_ids[@]}")
    cursor="${page_ids[-1]}"
    page=$((page + 1))
done

if [[ ${#STREAM_IDS[@]} -eq 0 ]]; then
    log "the employer has no streams"
fi

# ---------------------------------------------------------------------------
# Step 2 — read the streams, paginated
# ---------------------------------------------------------------------------
: > "$RECORDS"

total=${#STREAM_IDS[@]}
index=0
page=1
while [[ $index -lt $total ]]; do
    chunk=("${STREAM_IDS[@]:index:PAGE_SIZE}")
    ids_json=$(printf '%s\n' "${chunk[@]}" | jq -R 'tonumber' | jq -s '.')

    log "fetching batch $page (${#chunk[@]} stream(s))..."

    if streams_json=$(invoke get_streams_batch --stream_ids "$ids_json" 2>/dev/null) &&
        claimable_json=$(invoke claimable_batch --stream_ids "$ids_json" 2>/dev/null); then
        # Batch read — one call for the streams, one for the claimable amounts.
        printf '%s' "$streams_json" | jq -c --argjson c "$claimable_json" '
            (if type == "array" then . else [.] end) as $streams
            | [$streams | to_entries[] | .value + {claimable: ($c[.key] // 0)}][]
        ' >> "$RECORDS"
    else
        # Fallback for contracts without the batch entry points: one call per stream.
        log "batch reads unavailable, falling back to one call per stream..."
        for id in "${chunk[@]}"; do
            stream_json=$(invoke get_stream --stream_id "$id") || fail "could not read stream $id"
            claimable_json=$(invoke claimable --stream_id "$id") || fail "could not read the claimable amount of stream $id"
            printf '%s' "$stream_json" | jq -c --argjson c "$claimable_json" '
                (if type == "array" then .[0] else . end) + {claimable: $c}
            ' >> "$RECORDS"
        done
    fi

    index=$((index + PAGE_SIZE))
    page=$((page + 1))
done

# ---------------------------------------------------------------------------
# Step 3 — render
# ---------------------------------------------------------------------------
CSV_HEADER="id,employee,token,deposit,withdrawn,claimable,status,start_time,stop_time,rate_per_second"

render_csv() {
    echo "$CSV_HEADER"
    jq -sr '
        def s: if . == null then "" elif type == "string" then . else tostring end;
        def status:
            if . == null then ""
            elif type == "number" then (["Active", "Paused", "Cancelled", "Exhausted"][.] // tostring)
            else tostring end;
        .[]
        | [
            (.id | s),
            (.employee | s),
            (.token | s),
            (.deposit | s),
            (.withdrawn | s),
            (.claimable | s),
            (.status | status),
            (.start_time | s),
            (.stop_time | s),
            (.rate_per_second | s)
          ]
        | @csv
    ' "$RECORDS"
}

render_json() {
    jq -s '
        def s: if . == null then "" elif type == "string" then . else tostring end;
        def status:
            if . == null then ""
            elif type == "number" then (["Active", "Paused", "Cancelled", "Exhausted"][.] // tostring)
            else tostring end;
        [
            .[] | {
                id: (.id | s),
                employee: (.employee | s),
                token: (.token | s),
                deposit: (.deposit | s),
                withdrawn: (.withdrawn | s),
                claimable: (.claimable | s),
                status: (.status | status),
                start_time: (.start_time | s),
                stop_time: (.stop_time | s),
                rate_per_second: (.rate_per_second | s)
            }
        ]
    ' "$RECORDS"
}

if [[ -n "$OUTPUT" ]]; then
    if [[ "$FORMAT" == "json" ]]; then
        render_json > "$OUTPUT"
    else
        render_csv > "$OUTPUT"
    fi
    log "wrote $OUTPUT"
else
    if [[ "$FORMAT" == "json" ]]; then
        render_json
    else
        render_csv
    fi
fi
