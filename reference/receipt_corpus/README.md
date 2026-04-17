# Receipt Corpus

This directory is the home for fixed, manifest-backed receipt OCR corpora.

Purpose:

- hold a stable set of real or synthetic receipt images for OCR benchmarking
- keep provenance and licensing explicit
- let the OCR evaluator run against a fixed corpus instead of only generated synthetic packets

The intended layout is:

```text
reference/receipt_corpus/
  README.md
  .gitignore
  manifests/
    *.json
  assets/
    sroie/
    korean_receipts/
    nano_receipts/
```

The checked-in repo only contains the scaffold and import tooling by default. Imported dataset assets are large and should stay out of git unless we intentionally curate a tiny seed subset.

Importer:

```bash
python3 scripts/import_receipt_corpus.py --dataset sroie --limit 8
```

That writes a manifest under `reference/receipt_corpus/manifests/` and the corresponding images under `reference/receipt_corpus/assets/`.

You can then evaluate Gemini OCR on that fixed corpus with:

```bash
python3 scripts/evaluate_synthetic_ocr_corpus.py \
  --service-account-key /abs/path/to/service-account.json \
  --location global \
  --output-dir /tmp/expense_receipt_eval \
  --corpus-manifest reference/receipt_corpus/manifests/sroie_train_0_8.json \
  --model gemini-3-flash-preview
```
