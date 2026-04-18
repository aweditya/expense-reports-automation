#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

default_key="$repo_root/soe-agile-agents-7581b31cd4d2.json"
default_sdk_python="$repo_root/.venv/bin/python"

key_path="${KEY_PATH:-${VERTEX_SERVICE_ACCOUNT_KEY:-$default_key}}"
sdk_python="${SDK_PY:-${VERTEX_GEMINI_SDK_PYTHON:-$default_sdk_python}}"
location="${VERTEX_LOCATION:-global}"
model="${VERTEX_MODEL:-gemini-3-flash-preview}"
packets="${PACKETS:-4}"
output_dir="${OUT:-/tmp/expense_gemini_smoke}"
project_id="${VERTEX_PROJECT_ID:-}"

usage() {
  cat <<'EOF'
Usage: scripts/run_gemini_smoke_test.sh [options]

Options:
  --key PATH         Service-account JSON key path
  --sdk-python PATH  Python interpreter with google-genai installed
  --location VALUE   Vertex location (default: global)
  --model VALUE      Gemini model id (default: gemini-3-flash-preview)
  --packets N        Synthetic packet count for corpus eval (default: 4)
  --out DIR          Output directory (default: /tmp/expense_gemini_smoke)
  --project ID       Optional explicit Vertex project id
  -h, --help         Show this help

Environment variables:
  KEY_PATH / VERTEX_SERVICE_ACCOUNT_KEY
  SDK_PY / VERTEX_GEMINI_SDK_PYTHON
  VERTEX_LOCATION
  VERTEX_MODEL
  PACKETS
  OUT
  VERTEX_PROJECT_ID
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --key)
      key_path="$2"
      shift 2
      ;;
    --sdk-python)
      sdk_python="$2"
      shift 2
      ;;
    --location)
      location="$2"
      shift 2
      ;;
    --model)
      model="$2"
      shift 2
      ;;
    --packets)
      packets="$2"
      shift 2
      ;;
    --out)
      output_dir="$2"
      shift 2
      ;;
    --project)
      project_id="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ ! -f "$key_path" ]]; then
  echo "service-account key not found: $key_path" >&2
  exit 1
fi

if [[ ! -x "$sdk_python" ]]; then
  echo "SDK python not executable: $sdk_python" >&2
  exit 1
fi

common_auth_args=(
  --service-account-key "$key_path"
  --location "$location"
  --sdk-python "$sdk_python"
)

if [[ -n "$project_id" ]]; then
  common_auth_args+=(--project "$project_id")
fi

echo "Output directory: $output_dir"
echo "Model: $model"
echo "Location: $location"
echo "SDK python: $sdk_python"
echo "Packets: $packets"

rm -rf "$output_dir"
mkdir -p "$output_dir"

pushd "$repo_root" >/dev/null

python3 scripts/render_text_documents_for_ocr.py \
  --output-dir "$output_dir/rendered" \
  --format both \
  fixtures/curated/flight_itinerary/airline_itinerary_classic.md \
  fixtures/curated/hotel_folio/hotel_folio_guest_bill.md \
  fixtures/curated/receipt/receipt_card_dotted.md

cargo run --bin transcribe_document -- \
  --engine vertex-gemini-sdk \
  "${common_auth_args[@]}" \
  --model "$model" \
  --format json \
  --output "$output_dir/receipt.transcribed.json" \
  "$output_dir/rendered/receipt_card_dotted.png"

cargo run --bin ingest_expense_documents -- \
  --output-dir "$output_dir/ingest" \
  --fx demo \
  --engine vertex-gemini-sdk \
  "${common_auth_args[@]}" \
  --model "$model" \
  "$output_dir/rendered/airline_itinerary_classic.png" \
  "$output_dir/rendered/hotel_folio_guest_bill.pdf" \
  "$output_dir/rendered/receipt_card_dotted.png"

python3 scripts/evaluate_synthetic_ocr_corpus.py \
  "${common_auth_args[@]}" \
  --output-dir "$output_dir/eval" \
  --packets "$packets" \
  --model "$model"

popd >/dev/null

cat <<EOF

Gemini smoke test completed.

Key outputs:
  Single-doc OCR JSON:  $output_dir/receipt.transcribed.json
  End-to-end workbench: $output_dir/ingest/review_workbench.html
  OCR eval report:      $output_dir/eval/ocr_evaluation.md
  OCR eval JSON:        $output_dir/eval/ocr_evaluation.json
EOF
