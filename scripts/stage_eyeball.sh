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
#   .scratch/<stage_name>/report.json           reduced ExpenseReport
#   .scratch/<stage_name>/workbench.html        rendered workbench
#   .scratch/<stage_name>/lines-domestic.csv    7-col CSV for Stanford's domestic portal page
#   .scratch/<stage_name>/lines-foreign.csv     20-col CSV for Stanford's foreign portal page
#   .scratch/uploads/<stage_name>/              staged for Flask static-serve
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
CSV_DOMESTIC="${CHECK_DIR}/lines-domestic.csv"
CSV_FOREIGN="${CHECK_DIR}/lines-foreign.csv"
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

echo "    fx_enrich → ${REPORT} (Frankfurter live rates)"
./.venv/bin/python scripts/fx_enrich.py --in "${REPORT}" --out "${REPORT}"

echo "    render → ${WORKBENCH}"
./target/debug/render_workbench_from_report \
  --report "${REPORT}" \
  --receipts-dir .scratch/spike \
  --out "${WORKBENCH}" \
  --csv-domestic-out "${CSV_DOMESTIC}" \
  --csv-foreign-out "${CSV_FOREIGN}" \
  >/dev/null

echo "    stage → ${UPLOAD_DIR}"
mkdir -p "${UPLOAD_DIR}/files" "${UPLOAD_DIR}/reduced" "${UPLOAD_DIR}/extractions"
cp "${WORKBENCH}" "${CSV_DOMESTIC}" "${CSV_FOREIGN}" "${UPLOAD_DIR}/"
# friday Stage 7 needs reduced/report.json + extractions/* so the
# /uploads/<id>/edit endpoint can read the report and re-render the
# workbench after an edit. Without these the edit POST returns 404.
cp "${REPORT}" "${UPLOAD_DIR}/reduced/report.json"
cp .scratch/spike/*.json "${UPLOAD_DIR}/extractions/" 2>/dev/null || true
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
