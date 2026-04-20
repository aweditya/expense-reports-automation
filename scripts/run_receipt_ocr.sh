#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
venv_python="$repo_root/.venv/bin/python"
default_location="global"
default_model="gemini-3-flash-preview"

usage() {
  cat <<'EOF'
Run Gemini OCR yourself from this repo.

Prereqs:
  1. A repo-local Python environment at ./.venv
  2. A Vertex service-account JSON key
  3. cargo available on PATH

Examples:
  # OCR one document into JSON
  bash scripts/run_receipt_ocr.sh single \
    --key /abs/path/to/service-account.json \
    --document reference/receipt_corpus/assets/sroie/x00016469619.png \
    --out out/single_receipt

  # Compare an original pass with a table-focused binarized pass
  bash scripts/run_receipt_ocr.sh compare \
    --key /abs/path/to/service-account.json \
    --document reference/receipt_corpus/assets/sroie/x00016469619.png \
    --out out/receipt_compare

  # Evaluate the fixed 12-receipt public seed corpus
  bash scripts/run_receipt_ocr.sh corpus \
    --key /abs/path/to/service-account.json \
    --manifest reference/receipt_corpus/manifests/sroie_train_0_12.json \
    --out out/sroie12

  # Run OCR plus full downstream ingestion for one document
  bash scripts/run_receipt_ocr.sh ingest \
    --key /abs/path/to/service-account.json \
    --document reference/receipt_corpus/assets/sroie/x00016469619.png \
    --out out/ingest_receipt

Subcommands:
  single   -> Gemini OCR only, writes one transcribed JSON file
  compare  -> Runs two OCR passes for one document (original + table-focused)
  corpus   -> Gemini OCR evaluation over a manifest-backed corpus
  ingest   -> Gemini OCR plus the full downstream ingestion pipeline
EOF
}

fail() {
  echo "error: $*" >&2
  exit 1
}

require_common_tools() {
  [[ -x "$venv_python" ]] || fail "missing repo-local Python at $venv_python"
  command -v cargo >/dev/null 2>&1 || fail "cargo is not on PATH"
}

default_key_path() {
  local candidate="$repo_root/soe-agile-agents-7581b31cd4d2.json"
  if [[ -f "$candidate" ]]; then
    printf '%s\n' "$candidate"
  fi
}

run_cmd() {
  printf '+'
  for arg in "$@"; do
    printf ' %q' "$arg"
  done
  printf '\n'
  "$@"
}

subcommand="${1:-}"
if [[ -z "$subcommand" || "$subcommand" == "--help" || "$subcommand" == "-h" ]]; then
  usage
  exit 0
fi
shift

key_path="${VERTEX_SERVICE_ACCOUNT_KEY:-$(default_key_path)}"
location="${VERTEX_LOCATION:-$default_location}"
model="${VERTEX_MODEL:-$default_model}"
project="${VERTEX_PROJECT_ID:-}"
output_dir=""
document_path=""
manifest_path=""
pass_id=""
pass_kind="primary"
preprocess_variant="original"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --key)
      key_path="$2"
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
    --project)
      project="$2"
      shift 2
      ;;
    --document)
      document_path="$2"
      shift 2
      ;;
    --manifest)
      manifest_path="$2"
      shift 2
      ;;
    --out)
      output_dir="$2"
      shift 2
      ;;
    --pass-id)
      pass_id="$2"
      shift 2
      ;;
    --pass-kind)
      pass_kind="$2"
      shift 2
      ;;
    --preprocess-variant)
      preprocess_variant="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

require_common_tools
[[ -n "$key_path" ]] || fail "missing --key and no VERTEX_SERVICE_ACCOUNT_KEY/default key was found"
[[ -f "$key_path" ]] || fail "service-account key does not exist: $key_path"

case "$subcommand" in
  single)
    [[ -n "$document_path" ]] || fail "single requires --document"
    [[ -n "$output_dir" ]] || fail "single requires --out"
    mkdir -p "$output_dir"
    output_json="$output_dir/$(basename "${document_path%.*}").transcribed.json"

    cmd=(
      cargo run --bin transcribe_document --
      --engine vertex-gemini-sdk
      --service-account-key "$key_path"
      --location "$location"
      --model "$model"
      --sdk-python "$venv_python"
      --format json
      --output "$output_json"
      --pass-kind "$pass_kind"
      --preprocess-variant "$preprocess_variant"
    )
    if [[ -n "$pass_id" ]]; then
      cmd+=(--pass-id "$pass_id")
    fi
    if [[ -n "$project" ]]; then
      cmd+=(--project "$project")
    fi
    cmd+=("$document_path")
    run_cmd "${cmd[@]}"
    printf '\nOCR JSON written to %s\n' "$output_json"
    ;;

  compare)
    [[ -n "$document_path" ]] || fail "compare requires --document"
    [[ -n "$output_dir" ]] || fail "compare requires --out"
    mkdir -p "$output_dir"

    base_name="$(basename "${document_path%.*}")"
    primary_json="$output_dir/${base_name}.primary_original.json"
    table_json="$output_dir/${base_name}.table_focused_binarized.json"

    primary_cmd=(
      cargo run --bin transcribe_document --
      --engine vertex-gemini-sdk
      --service-account-key "$key_path"
      --location "$location"
      --model "$model"
      --sdk-python "$venv_python"
      --format json
      --output "$primary_json"
      --pass-kind primary
      --preprocess-variant original
      --pass-id "${base_name}_primary_original"
    )
    table_cmd=(
      cargo run --bin transcribe_document --
      --engine vertex-gemini-sdk
      --service-account-key "$key_path"
      --location "$location"
      --model "$model"
      --sdk-python "$venv_python"
      --format json
      --output "$table_json"
      --pass-kind table_focused
      --preprocess-variant binarized
      --pass-id "${base_name}_table_focused_binarized"
    )
    if [[ -n "$project" ]]; then
      primary_cmd+=(--project "$project")
      table_cmd+=(--project "$project")
    fi
    primary_cmd+=("$document_path")
    table_cmd+=("$document_path")

    run_cmd "${primary_cmd[@]}"
    run_cmd "${table_cmd[@]}"
    printf '\nOCR comparison artifacts:\n'
    printf '  %s\n' "$primary_json"
    printf '  %s\n' "$table_json"
    ;;

  corpus)
    [[ -n "$manifest_path" ]] || fail "corpus requires --manifest"
    [[ -n "$output_dir" ]] || fail "corpus requires --out"
    mkdir -p "$output_dir"

    cmd=(
      "$venv_python" scripts/evaluate_synthetic_ocr_corpus.py
      --service-account-key "$key_path"
      --location "$location"
      --sdk-python "$venv_python"
      --output-dir "$output_dir"
      --corpus-manifest "$manifest_path"
      --model "$model"
    )
    run_cmd "${cmd[@]}"
    printf '\nCorpus reports:\n'
    printf '  %s\n' "$output_dir/ocr_evaluation.json"
    printf '  %s\n' "$output_dir/ocr_comparison.md"
    ;;

  ingest)
    [[ -n "$document_path" ]] || fail "ingest requires --document"
    [[ -n "$output_dir" ]] || fail "ingest requires --out"
    mkdir -p "$output_dir"

    cmd=(
      cargo run --bin ingest_expense_documents --
      --output-dir "$output_dir"
      --bundle-id "$(basename "${document_path%.*}")"
      --fx demo
      --engine vertex-gemini-sdk
      --service-account-key "$key_path"
      --location "$location"
      --model "$model"
      --sdk-python "$venv_python"
    )
    if [[ -n "$project" ]]; then
      cmd+=(--project "$project")
    fi
    cmd+=("$document_path")
    run_cmd "${cmd[@]}"
    printf '\nIngestion artifacts:\n'
    printf '  %s\n' "$output_dir/review_workbench.html"
    printf '  %s\n' "$output_dir/transcriptions"
    printf '  %s\n' "$output_dir/facts"
    ;;

  *)
    fail "unknown subcommand: $subcommand"
    ;;
esac
