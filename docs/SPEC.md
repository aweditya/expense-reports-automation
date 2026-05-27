# SPEC

Architecture reference for the Stanford expense-report extraction
pipeline. Describes **the system as it stands today** — what the layers
are, what each one is responsible for, how data flows between them, and
what runs where in production.

For the *history* of how we got here, see `redesign-plan.md` and
`redesign-regrets.md`. For the *deploy* facts (URLs, region, IAM, etc.)
see `deploy-cheatsheet.md`. This file is the architecture, not the
incident log and not the runbook.

All diagrams are Mermaid; GitHub renders them inline.

---

## 1. Layer model

The pipeline is a strict 6-layer cake. Each layer's only job is to
consume the previous layer's output and produce the next layer's input.
If a piece of code touches two layers' worth of concern, it is wrong.

| # | Layer | Implemented in | Owns |
|---|---|---|---|
| 1 | **Extraction** | `scripts/extract_<kind>.py` (one per expense kind) | One Gemini call per uploaded document — receipt image → typed JSON — for small-detail-block kinds (miscellaneous, membership, mileage). Kinds whose detail block plus the common+extras blocks exceed Vertex's schema property-count ceiling (> ~5 `_meta`-wrapped T2/T3 leaves, in practice) make **multiple parallel calls** with their schema split across them; the per-kind script merges the outputs back into the same per-doc JSON shape. As of B1 this applies to meal (2-call: `main`+`extras`), transport (2-call), lodging (2-call), and airfare (3-call: `main`+`aux`+`extras`). B2 added single-call `mileage` (personal mileage at the IRS Standard Mileage Rate; reduction computes `line_amount_usd = distance × rate` from `generated/irs_mileage_rates.json`, fetched manually via `scripts/fetch_irs_mileage_rate.py`). The dispatcher in `local_app_simple.py` picks the script based on the FA's per-file kind choice in the upload form. See §7 for when to use the multi-call pattern. **Each extractor also makes ONE Document AI OCR call per document BEFORE Gemini** (Leapfrog L.3–L.5; docs/leapfrog-plan.md): DocAI's numbered token list gets appended to Gemini's prompt; Gemini returns the value + a verbatim `quote` + `token_ids` (integer indices into the token list) per `document_span` evidence entry. After Gemini returns, `scripts/evidence_bbox.py::populate_bboxes` resolves `token_ids` to bboxes via dict lookup. A Levenshtein verifier (threshold 0.7) catches Gemini-hallucinated IDs; on rejection (or absent token_ids) the code falls back to the legacy text-matching path against the same DocAI tokens. Either way bboxes land in `_meta.evidence[].bboxes` — pure metadata the workbench reads at render time to draw a spot-check halo. OCR failure is non-fatal: extraction proceeds, bboxes are absent, workbench falls back to "no halo." |
| 2 | **Derivation** | (inside extraction) | Fields a single document can yield from its own contents. No cross-document signal. |
| 3 | **Reduction** | `src/reduce.rs` (+ `src/fa_input.rs::apply_to_report`) | Combines per-document JSONs into one `ExpenseReport`. Aggregations like `total_usd`, `transaction_date`, derived `category` + confidence. Foreign lines get a **mock FX rate** here as a safety fallback; the real rate is patched in step 3a. **FA-entered general_information fields** (payee, business_purpose, authorized_by, payment_method, foreign_activity_type) overlay the reduced report via `apply_to_report` when the `reduce_extractions` binary gets `--fa-input <path>`; FA values are wrapped with `kind: user_input, origin: fa_upload_form` evidence. |
| 3a | **FX enrichment** | `scripts/fx_enrich.py` (uses `scripts/fx_lookup.py`) | Walks `transaction_lines`; for every foreign line (`common.original_currency` set), calls Frankfurter (`https://api.frankfurter.dev/v1/{date}?from={ccy}&to=USD`) for the historical ECB rate, overwrites `common.exchange_rate.value` + recomputes `common.line_amount_usd`, then cascades `transaction_summary.total_usd`. Failure-tolerant: network errors / unsupported currencies / 4xx responses leave the reducer's mock rate in place so the workbench still renders. Stamps `_meta.evidence[].origin = "fx_enrich.frankfurter"` so the FA can audit which lines came from a live lookup vs mock. |
| 4 | **Validation** | `src/validator_typed.rs` (+ `src/validator.rs`) | Checks the assembled `ExpenseReport` against business rules. Produces `ValidationReport` (issues only — never mutates the report). Includes hand-written passes alongside the rule-engine ones — e.g. `check_dates_within_trip_window` warns when a transaction line's date falls outside the FA-entered `business_purpose.when` window. |
| 5 | **Display** | `src/workbench_simple.rs` (+ `src/workbench_simple.css`) + `src/csv_export.rs` | Renders typed report + validation issues into the FA-facing workbench HTML. Pure formatting; no business decisions. Also emits FA-downloadable line-item CSVs alongside the workbench — one per Stanford portal page: `lines-domestic.csv` (7 columns, plain expense-type strings) and `lines-foreign.csv` (20 columns, suffixed expense-type strings with the Stanford-side typos preserved verbatim). Per-line routing in `csv_export::route_to_foreign` decides which file each line lands in; either file may be header-only when the report has no lines routed to that page. Both are static-served via the existing `/uploads/<id>/<filename>` route. The business_purpose concatenated text used to be a sidecar `.txt` download but is now an in-page click-to-copy card under General Information (friday Stage 3). |
| 6 | **Submission** | (not implemented) | Future: translate the internal model into Stanford's portal API payload. Today the FA reads the workbench and submits manually. |

The key invariants:

- **Reduction never reads from outside `[ExtractedReceipt]`.** It cannot fetch external data, call APIs, or look at the filesystem.
- **Validation takes `&ExpenseReport`** (immutable borrow). The type system literally forbids it from changing values; it can only emit issues.
- **Display takes everything and produces a `String`.** No side effects.
- **The orchestrating binary calls the layers in order**: reduce → validate → render. Never out of order, never reverse.

**FA input as a side channel**: The FA fills a fieldset on the upload form before picking files (payee, business_purpose, authorized_by, payment_method, foreign_activity_type, trip date window). Flask serializes those fields to `fa_input.json` in the upload directory; `reduce_extractions --fa-input <path>` reads the file after the per-receipt reduction and overlays the values into `general_information.*` with `kind: user_input` evidence. The file is optional — omitting it leaves the document-driven flow unchanged. The FA's "Where" field also uses Google Places API autocomplete (via Flask's `/places/autocomplete` proxy) for live city/state/country suggestions; the underlying input remains free-form text.

---

## 2. Component diagram

What each module is and how they wire together.

```mermaid
graph TB
    subgraph FA["FA's browser"]
        UI[Upload form: per-file kind dropdown / Workbench HTML]
    end

    subgraph CR["Cloud Run container"]
        subgraph Py["Python"]
            Flask["scripts/local_app_simple.py<br/>Flask + gunicorn<br/>(dispatcher routes per FA-chosen kind)"]
            ExtMeal["scripts/extract_meal.py"]
            ExtTransport["scripts/extract_transport.py"]
            ExtLib["scripts/extractor_lib.py<br/>(shared: CLI, ADC, Gemini call)"]
            EvBbox["scripts/evidence_bbox.py<br/>(OCR grounding:<br/>quote → token bboxes)"]
        end
        subgraph Rust["Rust binaries"]
            Reduce["reduce_extractions<br/>(uses src/reduce.rs)"]
            Render["render_workbench_from_report<br/>(uses validator_typed + workbench_simple)"]
        end
        subgraph Lib["Rust lib (expense_report_schema)"]
            ER[expense_report_model<br/>generated]
            VR[validation_rules<br/>generated]
            ERcpt[extracted_receipt]
            Meta[meta: Wrapped&lt;T&gt;, FieldMetadata]
            Reducer[reduce]
            Vald[validator_typed]
            Wb[workbench_simple]
        end
    end

    Vertex["Vertex AI / Gemini API"]
    DocAI["Document AI<br/>(OCR_PROCESSOR)"]
    Places["Google Places API<br/>(New)"]
    Schema["schema.yaml<br/>(source of truth)"]
    Codegen["scripts/generate_schema_artifacts.py<br/>scripts/generate_response_schema.py"]
    RsMeal["generated/response_schema_meal_{main,extras}.json"]
    RsTransport["generated/response_schema_transport_{main,extras}.json"]
    FaInput["fa_input.json<br/>FA fieldset values"]
    FaInputRs[fa_input]

    UI -->|GET /places/autocomplete?q=...<br/>on Where keystrokes| Flask
    Flask -->|HTTPS POST| Places
    Places -->|city/state/country suggestions| Flask
    UI -->|POST /upload<br/>file_N + kind_N + FA fieldset values| Flask
    Flask -->|writes per FA fieldset| FaInput
    Flask -->|kind=meal| ExtMeal
    Flask -->|kind=transport| ExtTransport
    ExtMeal --- ExtLib
    ExtTransport --- ExtLib
    ExtLib --- EvBbox
    EvBbox -->|HTTPS| DocAI
    DocAI -->|tokens with ids + bboxes| EvBbox
    ExtMeal -->|HTTPS prompt + image + token list| Vertex
    ExtTransport -->|HTTPS prompt + image + token list| Vertex
    Vertex -->|JSON: values + quote + token_ids| ExtMeal
    Vertex -->|JSON: values + quote + token_ids| ExtTransport
    ExtMeal -->|extractions/*.json<br/>incl. bboxes in _meta| Flask
    ExtTransport -->|extractions/*.json<br/>incl. bboxes in _meta| Flask
    Flask -->|spawns: --in --out --fa-input| Reduce
    FaInput -.read by.-> Reduce
    Reduce -->|reduced/report.json| Flask
    Flask -->|spawns: --csv-domestic-out --csv-foreign-out| Render
    Render -->|workbench.html + lines-domestic.csv + lines-foreign.csv| Flask
    Flask -->|303 redirect| UI
    UI -->|GET workbench.html| Flask

    Schema -.->|generates| Codegen
    Codegen -.->|emits| ER
    Codegen -.->|emits| VR
    Codegen -.->|emits| RsMeal
    Codegen -.->|emits| RsTransport
    RsMeal -.read by.-> ExtMeal
    RsTransport -.read by.-> ExtTransport

    Reduce --- Reducer
    Reduce --- FaInputRs
    Render --- Vald
    Render --- Wb
    Reducer -.uses.-> ERcpt
    Reducer -.uses.-> ER
    Reducer -.uses.-> Meta
    FaInputRs -.uses.-> ER
    Vald -.uses.-> ER
    Vald -.uses.-> VR
    Vald -.uses.-> FaInputRs
    Wb -.uses.-> ER
    Wb -.uses.-> ERcpt
```

---

## 3. Sequence diagram — one upload, end to end

The temporal flow from "FA clicks Process Receipts" to "FA sees the
workbench."

```mermaid
sequenceDiagram
    actor FA
    participant Browser
    participant IAP as Google IAP
    participant Flask as Flask (gunicorn + dispatcher)
    participant Extract as extract_(kind).py
    participant Gemini as Gemini API
    participant DocAI as Document AI
    participant Reduce as reduce_extractions (Rust)
    participant FxEnrich as fx_enrich.py
    participant Frankfurter as Frankfurter API
    participant Render as render_workbench_from_report (Rust)
    participant Disk as .scratch/uploads/{id}/

    FA->>Browser: fill fieldset (payee, business_purpose, dates, etc.)<br/>pick files + kind per file, click Process Receipts
    Browser->>IAP: POST /upload (multipart with file_N, kind_N pairs, and FA fieldset values)
    IAP->>Flask: forwarded request (auth verified)
    Flask->>Disk: save raw files to files/
    Flask->>Disk: write fa_input.json (FA fieldset values)

    loop for each (uploaded file, kind) pair
        Note over Flask: dispatcher picks extract_meal.py or<br/>extract_transport.py from FA's kind choice
        Flask->>Extract: subprocess: --image f --output extractions/f.json
        Extract->>DocAI: process_document(image)
        DocAI-->>Extract: tokens (id, text, bbox)
        Note over Extract: format_tokens_for_prompt:<br/>append numbered token list to prompt
        Extract->>Gemini: generate_content(prompt+tokens, image, response_schema)
        Gemini-->>Extract: structured JSON (values + quote + token_ids)
        Note over Extract: populate_bboxes (dual path):<br/>token_ids → dict lookup + verifier<br/>then fallback to text-match the quote
        Extract->>Disk: write extractions/{name}.json
        Extract-->>Flask: exit 0
    end

    Flask->>Reduce: subprocess: --in extractions/ --out reduced/report.json --fa-input fa_input.json
    Reduce->>Disk: read all extractions/*.json
    Reduce->>Disk: read fa_input.json (if present)
    Note over Reduce: reduce_to_expense_report(receipts)<br/>then fa_input::apply_to_report(report, fa)<br/>foreign lines get a mock FX rate as safety fallback
    Reduce->>Disk: write reduced/report.json
    Reduce-->>Flask: exit 0

    Flask->>FxEnrich: subprocess: --in reduced/report.json --out reduced/report.json
    FxEnrich->>Disk: read reduced/report.json
    loop for each unique (currency, date) on a foreign line
        FxEnrich->>Frankfurter: GET /v1/{date}?from={ccy}&to=USD
        Frankfurter-->>FxEnrich: rate (or 4xx for unsupported)
    end
    Note over FxEnrich: overwrite mock exchange_rate + line_amount_usd<br/>cascade transaction_summary.total_usd<br/>fall back to mock on network / unsupported
    FxEnrich->>Disk: write reduced/report.json
    FxEnrich-->>Flask: exit 0

    Flask->>Render: subprocess: --report reduced/... --receipts-dir extractions/ --out workbench.html --csv-domestic-out lines-domestic.csv --csv-foreign-out lines-foreign.csv
    Render->>Disk: read report.json + extractions
    Note over Render: validate_typed(&report) — incl. check_dates_within_trip_window<br/>render_workbench_html(report, receipts, validation)<br/>report_to_domestic_csv(report) + report_to_foreign_csv(report)
    Render->>Disk: write workbench.html + lines-domestic.csv + lines-foreign.csv
    Render-->>Flask: exit 0

    Flask-->>IAP: 303 See Other → /uploads/{id}/workbench.html
    IAP-->>Browser: 303
    Browser->>IAP: GET /uploads/{id}/workbench.html
    IAP->>Flask: forwarded GET
    Flask->>Disk: send_from_directory
    Flask-->>Browser: HTML
    Browser-->>FA: rendered workbench
```

---

## 4. Activity diagram — per-upload control flow

The decision flow inside a single upload, including failure paths.

```mermaid
flowchart TD
    Start([POST /upload]) --> Validate{All file_N + kind_N pairs<br/>well-formed?}
    Validate -->|no| Err0[Return 400: missing or unknown kind]
    Validate -->|yes| Save[Save raw files to disk]
    Save --> Loop{More files?}

    Loop -->|yes| Kind{file's kind?}
    Kind -->|meal| ExtM[Run extract_meal.py]
    Kind -->|transport| ExtT[Run extract_transport.py]
    ExtM --> ExtOK{Exit 0?}
    ExtT --> ExtOK
    ExtOK -->|no| Err1[Raise PipelineError step=extract]
    ExtOK -->|yes| Loop

    Loop -->|no| Red[Run reduce_extractions]
    Red --> RedOK{Exit 0?}
    RedOK -->|no| Err2[Raise PipelineError step=reduce]
    RedOK -->|yes| Ren[Run render_workbench_from_report]

    Ren --> RenOK{Exit 0?}
    RenOK -->|no| Err3[Raise PipelineError step=render]
    RenOK -->|yes| Redirect[Return 303 → workbench.html]

    Err1 --> ErrorPage[Render friendly error page<br/>via @app.errorhandler]
    Err2 --> ErrorPage
    Err3 --> ErrorPage

    Redirect --> Done([FA sees workbench])
    Err0 --> Done3([FA sees 400])
    ErrorPage --> Done2([FA sees error page])
```

---

## 5. Block diagram — deployment topology

What runs where in production.

```mermaid
graph LR
    subgraph User["User"]
        Browser["FA browser"]
    end

    subgraph GCP["Google Cloud Platform — soe-agile-agents"]
        subgraph LB["External HTTPS Load Balancer"]
            LBFR["Forwarding rule<br/>34.160.32.50<br/>(nip.io)"]
            IAP["Identity-Aware Proxy<br/>(SSO auth)"]
        end

        subgraph CR["Cloud Run service: expense-reports (us-west1)"]
            Container["Container revision<br/>gcr.io/.../expense-reports:{sha}<br/>--max-instances=1<br/>--memory=2Gi<br/>--timeout=600s"]
            subgraph Process["Inside the container"]
                Gunicorn["gunicorn<br/>1 worker × 8 threads<br/>:8080"]
                Flask2["Flask app<br/>local_app_simple:app"]
                PyExt["extract_meal.py / extract_transport.py<br/>(subprocess per file, kind chosen by FA)"]
                RustBin["reduce_extractions<br/>render_workbench_from_report<br/>(prebuilt at /usr/local/bin)"]
                Scratch["/app/.scratch/uploads/<br/>(ephemeral)"]
            end
        end

        Vertex["Vertex AI<br/>Gemini 3 Flash"]
        DocAI3["Document AI<br/>OCR_PROCESSOR<br/>(us multi-region)"]
        Registry["Artifact Registry<br/>gcr.io/soe-agile-agents/expense-reports"]
        Firestore["Firestore (default db)<br/>jobs/{upload_id} (Phase 1)<br/>reports/{upload_id} (Phase 2a)"]
        GCS["Cloud Storage<br/>soe-agile-agents-expense-reports-state<br/>uploads/{id}/files + extractions (Phase 2b)"]

        subgraph CB["Cloud Build (us-west1)"]
            Trigger["Trigger: deploy-on-push<br/>fires on push to main"]
            BuildSteps["1. Rust tests<br/>2. Python tests<br/>3. docker build<br/>4. docker push<br/>5. gcloud run deploy"]
        end
    end

    subgraph Source["Source"]
        GH["GitHub:<br/>aweditya/expense-reports-automation"]
    end

    Browser -->|HTTPS| LBFR
    LBFR --> IAP
    IAP --> Container
    Gunicorn --> Flask2
    Flask2 --> PyExt
    Flask2 --> RustBin
    Flask2 --> Scratch
    PyExt --> Scratch
    RustBin --> Scratch
    PyExt -->|ADC + grpc| Vertex
    PyExt -->|ADC + grpc<br/>via evidence_bbox.py| DocAI3
    Flask2 -->|dual-write +<br/>rehydrate on cache miss| Firestore
    Flask2 -->|dual-write +<br/>rehydrate on cache miss| GCS

    GH -->|git push main| Trigger
    Trigger --> BuildSteps
    BuildSteps -->|push image| Registry
    BuildSteps -->|new revision| CR
```

Notes:

- The container runs **gunicorn**, not `flask run`. `--workers=1` matches `--max-instances=1` (single instance, single worker, threads handle concurrency).
- Storage under `/app/.scratch/uploads/` is a **disk cache**, not authoritative. A container recycle wipes it, but the durable tier (Firestore + GCS) survives — Phase 2c rehydrates the cache on the next request. The "upload → review immediately" workflow remains the common case; the rehydrate path makes 24-hour-old URLs work after a recycle.
- **Durable store (2026-05-25 → 2026-05-26)**, gated by three independent env vars set in `deploy/cloudbuild.yaml`:
  - **`USE_FIRESTORE_JOBS=1`** (Phase 1) — `_set_job` / `_get_job` in `scripts/local_app_simple.py` dispatch to `scripts/firestore_jobs.py` (`jobs/{upload_id}`, 7-day TTL). Container recycle mid-upload no longer wipes the SSE stream.
  - **`USE_FIRESTORE_REPORTS=1`** (Phase 2a) — every pipeline completion + every edit/delete/undo dual-writes to `scripts/firestore_reports.py` (`reports/{upload_id}`, 90-day TTL). Stores `{report_json, fa_input, history}`. Report payload is JSON-encoded as a string to dodge Firestore's "invalid nested entity" rejection of arrays-of-arrays (bbox coordinates).
  - **`USE_GCS_ARTIFACTS=1`** (Phase 2b) — source PDFs + per-receipt extraction JSONs dual-write via `scripts/gcs_artifacts.py` to `gs://soe-agile-agents-expense-reports-state/uploads/{id}/{files,extractions}/`.
  All three writes are **best-effort**: failures log via `log_error` but never break the FA flow; disk remains authoritative for in-flight reads.
- **Rehydrate-on-cache-miss (Phase 2c)** — `_rehydrate_upload` in `scripts/local_app_simple.py` is called from the `/uploads/<id>/<filename>` route when the upload directory is absent. Pulls report + fa_input + history from Firestore, downloads source PDFs + extractions from GCS, re-runs the `render_workbench_from_report` binary, then serves the now-materialized file. Returns false (→ 404) when Firestore has no record. This is what makes 24-hour-old URLs survive container recycles. See `docs/durable-store-plan.md`.
- **Auth**: the public URL is fronted by IAP, which gates on SSO. Cloud Run itself is `--no-allow-unauthenticated`. Vertex calls from inside the container use the runtime service account via ADC (no key file).
- Deploy gesture: `git push origin main`. The trigger is **regional (us-west1)** — see `deploy-cheatsheet.md`.

---

## 6. Class diagram — the type contract

The Rust types that flow between layers. The same shape is what the
Python extractor produces (deserialized via serde).

```mermaid
classDiagram
    class Wrapped~T~ {
        +Option~T~ value
        +FieldMetadata meta
        +known(value) Wrapped
        +unknown() Wrapped
        +is_unknown() bool
    }

    class FieldMetadata {
        +ConfidenceLevel confidence
        +Option~String~ confidence_reason
        +Vec~EvidenceReference~ evidence
        +bool needs_review
        +Vec~String~ flags
    }

    class EvidenceReference {
        +EvidenceKind kind
        +Option~String~ filename
        +Option~u32~ page
        +Option~String~ quote
        +Option~String~ origin
    }

    class ExtractedReceipt {
        +String source_filename
        +String expense_kind
        +ExpenseReportTransactionLinesItem line
        +Extras extras
    }

    class Extras {
        +Wrapped~String~ merchant_address
        +Wrapped~String~ printed_currency
    }

    class ExpenseReport {
        +ExpenseReportGeneralInformation general_information
        +ExpenseReportTransactionSummary transaction_summary
        +Option~Vec~ transaction_lines
        +Option~Vec~ per_diem_expenses
        +Option~Vec~ mileage_expenses
        +ExpenseReportAllocationAndApprovers allocation_and_approvers
    }

    class ValidationReport {
        +Vec~ValidationIssue~ issues
    }

    class ValidationIssue {
        +String path
        +ValidationSeverity severity
        +ValidationIssueKind kind
        +String message
    }

    Wrapped --> FieldMetadata : meta
    FieldMetadata --> EvidenceReference : evidence[]
    ExtractedReceipt --> Extras : extras
    ExtractedReceipt ..> Wrapped : every leaf is Wrapped~T~
    ExpenseReport ..> Wrapped : every T2/T3 leaf is Wrapped~T~
    ValidationReport --> ValidationIssue : issues[]
```

The single most important shape rule: **every T2 (system-derived) and
T3 (extracted) leaf is a `Wrapped<T>` carrying its own `_meta`**. T1
(FA-entered) leaves are bare `Option<T>` because they don't have model
provenance — the FA filled them in.

---

## 7. Layer responsibility table

A cheat-sheet for "which file does X belong in?"

| Need to … | Lives in | Layer |
|---|---|---|
| Add a per-document field Gemini extracts | `schema.yaml` + `scripts/extract_<kind>.py` (prompt) + regenerate | Extraction |
| Compute a per-receipt value from other per-receipt values | `scripts/extract_<kind>.py` (in the same call) or post-process in Python before write | Derivation |
| Aggregate across receipts (sum, earliest, derived enum) | `src/reduce.rs` | Reduction |
| Add a business rule (e.g. "X required when Y") | `schema.yaml` (`required:` clause, regenerates `validation_rules.rs`) and/or hand-coded in `src/validator_typed.rs` | Validation |
| Change how a field looks on the workbench | `src/workbench_simple.rs` + `src/workbench_simple.css` | Display |
| Future: emit a Stanford-portal payload | new `src/submit.rs` (does not exist yet) | Submission |
| Persist JOBS / reports / source artifacts across container recycle | `scripts/firestore_jobs.py` (Phase 1) + `scripts/firestore_reports.py` (Phase 2a) + `scripts/gcs_artifacts.py` (Phase 2b) + `_rehydrate_upload` in `scripts/local_app_simple.py` (Phase 2c) | Durable store |
| Add a new expense kind (hotel/cab/airfare/conference) | (1) per-kind detail block already in `schema.yaml`; (2) `scripts/generate_response_schema.py` — add the kind to `KIND_EXPENSE_TYPES` + a detail-block factory + an entry in `SCHEMAS_TO_GENERATE`; (3) new `scripts/extract_<kind>.py` (~30 lines using `run_extraction` from `extractor_lib`); (4) new dropdown option + dispatcher entry in `scripts/local_app_simple.py`; (5a) **add `render_<kind>_details(html, <kind>, path)` in `src/workbench_simple.rs` mirroring `render_lodging_details` / `render_airfare_details`**; (5b) **add the corresponding `if let Some(<kind>) = &line.<kind>_details` branch in `render_transaction_line`**; (5c) extend `line_summary_headline` with a per-kind headline; (5d) walk the new detail block in `src/validator_typed.rs`; (6) acceptance harness entries in `scripts/acceptance_check.py` (`EXTRACTORS` + `DETAIL_BLOCK_BY_KIND` + per-receipt fixtures with predicates). The Dockerfile globs `generated/response_schema_*.json`, so no Dockerfile change. Mental check: upload a {kind} receipt — does the FA see all the {kind}-specific fields on the workbench? If no, step (5) isn't done. **If the detail block has more than ~5 T2/T3 leaves** (Vertex's schema property-count ceiling), it needs the multi-call pattern: emit multiple schemas from `SCHEMAS_TO_GENERATE` (with `include_common`/`include_detail`/`include_extras` knobs) and orchestrate parallel `single_call` invocations + merge in the extractor — see `scripts/extract_lodging.py` (2-call) or `scripts/extract_airfare.py` (3-call when the detail block is too big to fit in one main call). | Extraction + Display + Validation |

---

## 8. Implementation file map

| Concern | File |
|---|---|
| Schema source of truth | `schema.yaml` |
| Codegen — Rust types + validation rules | `scripts/generate_schema_artifacts.py` |
| Codegen — Gemini response_schema | `scripts/generate_response_schema.py` |
| Generated Rust types | `generated/expense_report_model.rs` |
| Generated validation-rule constants | `generated/validation_rules.rs` |
| Generated response_schemas (per call) | `generated/response_schema_meal_main.json` + `_extras.json`, `generated/response_schema_transport_main.json` + `_extras.json`, `generated/response_schema_lodging_main.json` + `_extras.json`, `generated/response_schema_airfare_main.json` + `_aux.json` + `_extras.json`, plus single-call `generated/response_schema_miscellaneous.json`, `generated/response_schema_membership.json`, and `generated/response_schema_mileage.json` (B2). (meal/transport/lodging are split across 2 parallel calls; airfare across 3 — see §1 and §7) |
| Per-document I/O type | `src/extracted_receipt.rs` (incl. `Extras`, `NightlyRate`, `Segment` bare-array entries) |
| Provenance wrapper + metadata | `src/meta.rs`, `src/draft.rs` |
| Reduction | `src/reduce.rs` |
| Traversal-driven validation | `src/validator_typed.rs` (incl. `check_dates_within_trip_window` hand-written pass) |
| Validator types | `src/validator.rs` |
| FA-input overlay (type + parser + `apply_to_report` + `parse_when_window`) | `src/fa_input.rs` (consumed by `reduce_extractions --fa-input` and by the validator's date-window check) |
| FA-input per-upload file contract | `.scratch/uploads/<id>/fa_input.json` (JSON keys mirror `FaInput` struct field names; Flask writes it from POST form, Rust parses it via serde) |
| FA-downloadable export file contracts | `.scratch/uploads/<id>/lines-domestic.csv` (7 cols: `Line\|Expense Date\|Expense Currency\|Expense Amount\|USD Amount\|Expense Type\|Remarks`, plain-name expense types) + `.scratch/uploads/<id>/lines-foreign.csv` (20 cols inlining airfare/lodging details, suffixed-name expense types with Stanford-side typos preserved). Workbench `<a download>` links render only for whichever CSVs have non-zero line counts. |
| FA-supplied Stanford ERS Templates (SSOT for CSV column shape + Expense Type taxonomy per portal page) | `reference/ers-template.xlsm` (legacy 4-col domestic), `reference/ers-template-foreign.xlsx` (20-col foreign template + all foreign-page dropdown values: 25 expense types, 75 currencies, 3 affiliations, 5 booking methods, 16 activity types), `reference/ers-expense-type-dropdown-domestic.png` (26 domestic expense-type dropdown values from the live portal) |
| Workbench HTML renderer | `src/workbench_simple.rs` (+ `src/workbench_simple.css`) |
| FA-downloadable export generators (per-page CSV emitters + per-page mappers + business-purpose text concat) | `src/csv_export.rs` (consumed by the render binary; `report_to_domestic_csv` + `report_to_foreign_csv` + `line_counts` for the hero hide-if-empty) |
| Reduction binary | `src/bin/reduce_extractions.rs` |
| Render binary | `src/bin/render_workbench_from_report.rs` (writes `workbench.html`; with `--csv-domestic-out` and `--csv-foreign-out` also writes the two per-portal-page CSVs) |
| Round-trip contract check | `src/bin/roundtrip_check.rs` |
| Per-kind Python extractors (Gemini call) | `scripts/extract_meal.py` (B1: orchestrates 2 parallel calls — `main` carries common+meal_details; `extras` carries merchant_address+printed_currency), `scripts/extract_transport.py` (B1: same 2-call pattern), `scripts/extract_lodging.py` (2 parallel calls; extras includes per-night rate breakdown), `scripts/extract_airfare.py` (3 parallel calls — main/aux/extras — and 1-deep-merges `airfare_details` from main+aux), `scripts/extract_miscellaneous.py` (posters/printing/etc., no detail block; Stage 13, single-call), `scripts/extract_membership.py` (ACM/IEEE/USENIX dues, no detail block; Stage 14, single-call), `scripts/extract_mileage.py` (B2: personal mileage from Google Maps screenshot / driving log; single-call) |
| Shared extractor infrastructure | `scripts/extractor_lib.py` (CLI parsing, ADC client, `single_call` primitive used by all multi-call extractors, `run_extraction` wrapper for single-call kinds like miscellaneous/membership/mileage, retry-with-backoff on Vertex 5xx — Stage 21) |
| External-data scripts (manual periodic refresh) | `scripts/fetch_irs_mileage_rate.py` (B2: scrapes IRS standard mileage rates → `generated/irs_mileage_rates.json`; run when IRS announces a rate change). `scripts/generate_airport_codes.py` (Stanford-side airport-code lookup table). Both follow the same pattern: manual fetch → JSON cache committed to git → consumed at runtime with no network dep. |
| OCR grounding (per-doc Document AI call + dual-path bbox resolution) | `scripts/evidence_bbox.py` (`ocr_document` runs BEFORE Gemini so the token list can be inlined into Gemini's prompt; `populate_bboxes` resolves `token_ids` via dict lookup + Levenshtein verifier, falls back to text-matching for entries without token_ids or whose verifier failed; ~$0.0015/doc, ~1–3s latency) |
| Retrofit cached per-doc JSONs with bboxes | `scripts/retrofit_bboxes.py` (one-shot batch over `.scratch/spike/*.json`, lets workbench be validated without re-running Gemini) |
| Evidence + bbox coverage audit | `scripts/spike_evidence_audit.py` (walks `.scratch/spike/*.json` and emits `.scratch/audit/evidence_coverage.txt` with per-receipt and per-field-path coverage stats + miss list) |
| Leapfrog architecture spike (kept as historical artifact) | `scripts/spike_leapfrog.py` (5-receipt + corpus-scale spike that validated token-id grounding at 100/100 verifier pass; informed L.1–L.5), `scripts/spike_leapfrog_retrofit.py` (one-shot UI overlay tool used during L.0 visual sanity-check; superseded by L.3–L.5 production wiring) |
| Leapfrog design doc | `docs/leapfrog-plan.md` |
| Schema-acceptance pre-deploy probe | `scripts/probe_response_schemas.py` (auto-discovers every `generated/response_schema_*.json` via glob — drift-proof — and sends a minimal `generate_content` per schema; `KNOWN_BROKEN` allowlist for schemas that exceed Vertex's ceiling but aren't yet wired to any extractor. **Required pre-push gate after any detail-block schema change**; see CLAUDE.md) |
| Flask + gunicorn entry point | `scripts/local_app_simple.py` (Python routes + dispatcher only — HTML/CSS/JS templates extracted to `templates/upload_form.html`, `templates/progress.html`, `templates/error.html` and loaded at module import via `read_text()`). Includes `/places/autocomplete` proxy backed by Google Places API via ADC, and the custom combobox JS that drives the "Where" autocomplete UI. |
| HTML templates | `templates/upload_form.html` (upload form + form-persistence JS), `templates/progress.html` (SSE-driven live progress + per-file outcomes), `templates/error.html` (PipelineError friendly page). All use the `__PLACEHOLDER__` substitution pattern (no Jinja2). |
| Airport-code lookup | `src/airport_codes.rs` + `generated/airport_codes.json` (~9208 entries from `reference/ers-template-foreign-example-filled.xlsx` "Departure Airport" sheet; built by `scripts/generate_airport_codes.py`; consumed by `src/csv_export.rs::report_to_foreign_csv` to expand IATA codes to Stanford-portal full display strings) |
| Unified pipeline test harness | `scripts/test_pipeline.py` (subcommands: `parallel` for N concurrent uploads, `batch` for 1 upload × N files, `watch` for SSE polling an existing upload_id; auto IAP-bypass via `gcloud auth print-identity-token` for `https://*.run.app` URLs) |
| Acceptance harness | `scripts/acceptance_check.py` |
| Manual deploy escape hatch | `scripts/deploy.sh` |
| Cloud Build pipeline | `deploy/cloudbuild.yaml` |
| Container | `deploy/Dockerfile` |
| Runtime deps | `deploy/requirements.txt` |

Non-active files: docs (`docs/*.md`), the test fixture
(`tests/test_deploy_config.py`), the standalone schema visualizer
(`visualizer/`), domain reference PDFs in `reference/`, and the
receipts corpus in `receipts/`.
