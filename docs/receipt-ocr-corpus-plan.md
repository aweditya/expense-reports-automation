# Receipt OCR Corpus Plan

This document defines the next OCR-hardening track for the repo: move from synthetic-only OCR benchmarking toward a realistic receipt corpus while keeping licensing, provenance, and evaluation explicit.

## Goal

Build a receipt-focused OCR benchmark that is realistic enough to expose failures that do not show up in the current synthetic corpus:

- thermal-print distortion
- inconsistent section headings
- merchant names without explicit labels
- totals printed on separate lines
- noisy phone captures
- long item lists
- mixed currencies and tax labels

The scope is intentionally receipts first, not hotel folios.

## Why receipts first

Receipts are the highest-value stress target for the OCR boundary because they vary more than the other currently supported document families and they are the most likely to arrive as arbitrary scanned PDFs or phone photos.

## Corpus tiers

The corpus should be split into explicit tiers so benchmark results remain interpretable.

### Tier 1: Licensed real receipt datasets

Primary targets:

- `SROIE`
  - English scanned receipts
  - good baseline for OCR + key-field fidelity
- `CORD`
  - large real receipt corpus with restaurant/shop diversity
  - useful for line-item ordering and restaurant-style structure
- `Korean Receipts Dataset`
  - additional visual diversity
  - explicitly described as anonymized

These should be treated as the main realism benchmark.

### Tier 2: Realistic synthetic augmentation

Primary target:

- `Nano Receipts`

Use this to increase layout variety and to generate more stress cases, but do not treat it as the primary realism benchmark.

### Tier 3: Failure-injection variants

Generate transformed variants of Tier 1 and Tier 2 assets to stress OCR robustness:

- blur
- low contrast
- shadow
- skew / perspective warp
- crop near edges
- fold / crease overlays
- JPEG compression
- long-receipt pagination into PDF wrappers

This tier exists to push OCR stability, not to estimate real-world field accuracy directly.

## Acquisition policy

Do not scrape arbitrary user-posted receipts from random websites.

Allowed sources for the benchmark:

- public datasets with clear redistribution or research-use terms
- explicitly anonymized public datasets
- public template/example sites only as visual references, not as bulk evaluation data
- later, a separately tracked gold set of donated and redacted real receipts if available

Every corpus entry should carry provenance:

- `source_name`
- `source_url`
- `license`
- `split`
- `is_real`
- `is_synthetic`
- `is_transformed`

## Corpus storage model

Add a manifest-driven receipt OCR corpus format so the evaluator can run on fixed assets instead of only generated synthetic packets.

Suggested layout:

```text
reference/receipt_corpus/
  manifest.json
  README.md
  assets/
    sroie/
    cord/
    korean_receipts/
    nano_receipts/
```

Suggested manifest shape:

```json
{
  "corpus_name": "receipt_ocr_realism_v1",
  "documents": [
    {
      "document_id": "sroie_x00016469612",
      "kind": "receipt",
      "source_name": "SROIE",
      "source_url": "https://huggingface.co/datasets/jsdnrs/ICDAR2019-SROIE",
      "license": "cc-by-4.0",
      "input_path": "assets/sroie/X00016469612.jpg",
      "ground_truth_markdown_path": "assets/sroie/X00016469612.md",
      "expected_fields": {
        "merchant_name": "BOOK TA .#",
        "transaction_date": "2018-03-14",
        "total_paid": "56.00"
      },
      "tags": ["real", "english", "scanned"]
    }
  ]
}
```

`ground_truth_markdown_path` is optional at first, but the final benchmark should prefer entries that have:

- OCR text ground truth
- key fields
- or both

## Evaluation axes

The receipt OCR benchmark should report separate metrics for different failure classes.

### OCR markdown fidelity

- exact markdown match
- relaxed markdown match
- content-only match

### Key-field fidelity

For receipt-oriented fields:

- merchant name
- transaction date
- subtotal
- tax
- tip
- total paid
- currency

### Structural fidelity

- line-item count
- line ordering preservation
- whether totals remain below the item list
- whether label/value lines stay mergeable by the extractor

### Downstream extraction outcome

- classified as `Receipt` vs `Unknown`
- extraction status
- projected as meal vs generic receipt
- readiness outcome

## Engineering steps

### Phase 1

- keep the current synthetic OCR evaluator intact
- add manifest-driven corpus support to the evaluator
- add receipt-specific OCR cleanup for common real-receipt formatting failures
- broaden receipt classification so receipt-like OCR output does not require literal `Merchant Receipt` headings

### Phase 2

- ingest a first licensed real receipt subset
- attach OCR/key-field ground truth where available
- run Gemini Flash on that fixed set
- record failure clusters

### Phase 3

- add transformed variants
- compare models and prompt variants
- expand receipt subtype handling downstream

## Immediate success criteria

The next OCR iteration should make these cases work better than today:

- receipt PDFs/images with no explicit `Merchant Receipt` heading
- totals split across two OCR lines
- item rows with dotted leaders or amount-only continuation lines
- noisy merchant names in uppercase at the top of the page

## Non-goals for this phase

- hotel folio corpus expansion
- generalized invoice support
- production data collection pipeline
- broad web scraping of consumer-posted receipts
