#!/usr/bin/env bash
# Stage-eyeball helper: run the local reduce → render → stage → open
# pipeline that the friday-feedback stages keep needing, parameterized
# by a stage name. Replaces the per-stage inline `mkdir && cp && build
# && ./target/debug/X && open ...` bash chains that the user flagged
# as a violation of "if it's a script, it lives in scripts/" — even
# one-line chains, repeated 4-5 times, are a script.
#
# Usage:
#   scripts/stage_eyeball.sh <stage_name>
#
# Expects:
#   .scratch/<stage_name>/fa_input.json   ← caller writes this via the
#                                            harness's Write tool
#
# Produces:
#   .scratch/<stage_name>/report.json      reduced ExpenseReport
#   .scratch/<stage_name>/workbench.html   rendered workbench
#   .scratch/<stage_name>/lines.csv        CSV in Stanford portal format
#   .scratch/uploads/<stage_name>/         staged for Flask static-serve
#
# Then: starts Flask on port 8765 (no-op if already running on it),
# waits for /healthy 200, opens the workbench in the default browser.

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <stage_name>" >&2
  exit 2
fi

STAGE="$1"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

CHECK_DIR=".scratch/${STAGE}"
UPLOAD_DIR=".scratch/uploads/${STAGE}"
FA_INPUT="${CHECK_DIR}/fa_input.json"
REPORT="${CHECK_DIR}/report.json"
WORKBENCH="${CHECK_DIR}/workbench.html"
CSV="${CHECK_DIR}/lines.csv"
PORT="${PORT:-8765}"

if [[ ! -f "${FA_INPUT}" ]]; then
  echo "error: ${FA_INPUT} not found — caller should Write it before invoking this script" >&2
  exit 1
fi

echo "==> stage_eyeball: ${STAGE}"

echo "    build binaries"
cargo build --quiet --bin reduce_extractions --bin render_workbench_from_report

echo "    reduce → ${REPORT}"
./target/debug/reduce_extractions \
  --in .scratch/spike \
  --out "${REPORT}" \
  --fa-input "${FA_INPUT}" \
  >/dev/null

echo "    render → ${WORKBENCH}"
./target/debug/render_workbench_from_report \
  --report "${REPORT}" \
  --receipts-dir .scratch/spike \
  --out "${WORKBENCH}" \
  --csv-out "${CSV}" \
  >/dev/null

echo "    stage → ${UPLOAD_DIR}"
mkdir -p "${UPLOAD_DIR}/files"
cp "${WORKBENCH}" "${CSV}" "${UPLOAD_DIR}/"
# Copy receipts so spot-check links resolve. Globs allowed to fail
# silently if a particular extension isn't in the corpus.
cp receipts/*.pdf "${UPLOAD_DIR}/files/" 2>/dev/null || true
cp receipts/*.jpeg "${UPLOAD_DIR}/files/" 2>/dev/null || true
cp receipts/*.png "${UPLOAD_DIR}/files/" 2>/dev/null || true

# Reuse a running Flask if one already serves the port; otherwise
# start a fresh one in the background.
if curl -s -m 1 -o /dev/null -w "%{http_code}" "http://127.0.0.1:${PORT}/" 2>/dev/null | grep -q 200; then
  echo "    Flask already up on :${PORT}"
else
  echo "    starting Flask on :${PORT}"
  PORT="${PORT}" ./.venv/bin/python scripts/local_app_simple.py >/dev/null 2>&1 &
  # Wait for the port to start serving. 30s cap.
  for _ in $(seq 1 30); do
    if curl -s -m 1 -o /dev/null -w "%{http_code}" "http://127.0.0.1:${PORT}/" 2>/dev/null | grep -q 200; then
      break
    fi
    sleep 1
  done
fi

URL="http://127.0.0.1:${PORT}/uploads/${STAGE}/workbench.html"
echo "    open ${URL}"
open "${URL}"
