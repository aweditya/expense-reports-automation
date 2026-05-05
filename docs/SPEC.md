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
| 1 | **Extraction** | `scripts/spike_extract.py` | One Gemini call per uploaded document. Receipt image → typed JSON. |
| 2 | **Derivation** | (inside extraction) | Fields a single document can yield from its own contents. No cross-document signal. |
| 3 | **Reduction** | `src/reduce.rs` | Combines per-document JSONs into one `ExpenseReport`. Aggregations like `total_usd`, `transaction_date`, derived `category` + confidence. |
| 4 | **Validation** | `src/validator_typed.rs` (+ `src/validator.rs`) | Checks the assembled `ExpenseReport` against business rules. Produces `ValidationReport` (issues only — never mutates the report). |
| 5 | **Display** | `src/workbench_simple.rs` (+ `src/workbench_simple.css`) | Renders typed report + validation issues into the FA-facing workbench HTML. Pure formatting; no business decisions. |
| 6 | **Submission** | (not implemented) | Future: translate the internal model into Stanford's portal API payload. Today the FA reads the workbench and submits manually. |

The key invariants:

- **Reduction never reads from outside `[ExtractedReceipt]`.** It cannot fetch external data, call APIs, or look at the filesystem.
- **Validation takes `&ExpenseReport`** (immutable borrow). The type system literally forbids it from changing values; it can only emit issues.
- **Display takes everything and produces a `String`.** No side effects.
- **The orchestrating binary calls the layers in order**: reduce → validate → render. Never out of order, never reverse.

---

## 2. Component diagram

What each module is and how they wire together.

```mermaid
graph TB
    subgraph FA["FA's browser"]
        UI[Upload form / Workbench HTML]
    end

    subgraph CR["Cloud Run container"]
        subgraph Py["Python"]
            Flask["scripts/local_app_simple.py<br/>Flask + gunicorn"]
            Extract["scripts/spike_extract.py<br/>Gemini 3 Flash + response_schema"]
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
    Schema["schema.yaml<br/>(source of truth)"]
    Codegen["scripts/generate_schema_artifacts.py<br/>scripts/generate_response_schema.py"]

    UI -->|POST /upload| Flask
    Flask -->|per file| Extract
    Extract -->|HTTPS| Vertex
    Vertex -->|JSON| Extract
    Extract -->|.scratch/uploads/&lt;id&gt;/extractions/*.json| Flask
    Flask -->|spawns| Reduce
    Reduce -->|.scratch/uploads/&lt;id&gt;/reduced/report.json| Flask
    Flask -->|spawns| Render
    Render -->|workbench.html| Flask
    Flask -->|303 redirect| UI
    UI -->|GET workbench.html| Flask

    Schema -.->|generates| Codegen
    Codegen -.->|emits| ER
    Codegen -.->|emits| VR
    Codegen -.->|emits| Extract

    Reduce --- Reducer
    Render --- Vald
    Render --- Wb
    Reducer -.uses.-> ERcpt
    Reducer -.uses.-> ER
    Reducer -.uses.-> Meta
    Vald -.uses.-> ER
    Vald -.uses.-> VR
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
    participant Flask as Flask (gunicorn)
    participant Extract as spike_extract.py
    participant Gemini as Gemini API
    participant Reduce as reduce_extractions (Rust)
    participant Render as render_workbench_from_report (Rust)
    participant Disk as .scratch/uploads/&lt;id&gt;/

    FA->>Browser: pick files, click Process Receipts
    Browser->>IAP: POST /upload (multipart)
    IAP->>Flask: forwarded request (auth verified)
    Flask->>Disk: save raw files to files/

    loop for each uploaded file
        Flask->>Extract: subprocess: --image f --output extractions/f.json
        Extract->>Gemini: generate_content(prompt, image, response_schema)
        Gemini-->>Extract: structured JSON
        Extract->>Disk: write extractions/&lt;name&gt;.json
        Extract-->>Flask: exit 0
    end

    Flask->>Reduce: subprocess: --in extractions/ --out reduced/report.json
    Reduce->>Disk: read all extractions/*.json
    Reduce->>Disk: write reduced/report.json
    Reduce-->>Flask: exit 0

    Flask->>Render: subprocess: --report reduced/... --receipts-dir extractions/ --out workbench.html
    Render->>Disk: read report.json + extractions
    Note over Render: validate_typed(&report)<br/>render_workbench_html(report, receipts, validation)
    Render->>Disk: write workbench.html
    Render-->>Flask: exit 0

    Flask-->>IAP: 303 See Other → /uploads/&lt;id&gt;/workbench.html
    IAP-->>Browser: 303
    Browser->>IAP: GET /uploads/&lt;id&gt;/workbench.html
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
    Start([POST /upload]) --> Save[Save raw files to disk]
    Save --> Loop{More files?}

    Loop -->|yes| Ext[Run spike_extract.py on next file]
    Ext --> ExtOK{Exit 0?}
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
            Container["Container revision<br/>gcr.io/.../expense-reports:&lt;sha&gt;<br/>--max-instances=1<br/>--memory=2Gi<br/>--timeout=600s"]
            subgraph Process["Inside the container"]
                Gunicorn["gunicorn<br/>1 worker × 8 threads<br/>:8080"]
                Flask2["Flask app<br/>local_app_simple:app"]
                PyExt["spike_extract.py<br/>(subprocess per file)"]
                RustBin["reduce_extractions<br/>render_workbench_from_report<br/>(prebuilt at /usr/local/bin)"]
                Scratch["/app/.scratch/uploads/<br/>(ephemeral)"]
            end
        end

        Vertex["Vertex AI<br/>Gemini 3 Flash"]
        Registry["Artifact Registry<br/>gcr.io/soe-agile-agents/expense-reports"]

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

    GH -->|git push main| Trigger
    Trigger --> BuildSteps
    BuildSteps -->|push image| Registry
    BuildSteps -->|new revision| CR
```

Notes:

- The container runs **gunicorn**, not `flask run`. `--workers=1` matches `--max-instances=1` (single instance, single worker, threads handle concurrency).
- Storage under `/app/.scratch/uploads/` is **ephemeral** — destroyed when the container restarts. Production-acceptable today because the FA workflow is "upload → review immediately"; long-term retention would need GCS.
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
| Add a per-document field Gemini extracts | `schema.yaml` + `scripts/spike_extract.py` (prompt) + regenerate | Extraction |
| Compute a per-receipt value from other per-receipt values | `scripts/spike_extract.py` (in the same call) or post-process in Python before write | Derivation |
| Aggregate across receipts (sum, earliest, derived enum) | `src/reduce.rs` | Reduction |
| Add a business rule (e.g. "X required when Y") | `schema.yaml` (`required:` clause, regenerates `validation_rules.rs`) and/or hand-coded in `src/validator_typed.rs` | Validation |
| Change how a field looks on the workbench | `src/workbench_simple.rs` + `src/workbench_simple.css` | Display |
| Future: emit a Stanford-portal payload | new `src/submit.rs` (does not exist yet) | Submission |
| Add a new expense kind (hotel/cab/airfare/conference) | new `scripts/extract_<kind>.py` + new `generated/response_schema_<kind>.json` + per-kind detail block already in `schema.yaml` | Extraction |

---

## 8. Implementation file map

| Concern | File |
|---|---|
| Schema source of truth | `schema.yaml` |
| Codegen — Rust types + validation rules | `scripts/generate_schema_artifacts.py` |
| Codegen — Gemini response_schema | `scripts/generate_response_schema.py` |
| Generated Rust types | `generated/expense_report_model.rs` |
| Generated validation-rule constants | `generated/validation_rules.rs` |
| Generated response_schema for meal | `generated/response_schema_meal.json` |
| Per-document I/O type | `src/extracted_receipt.rs` |
| Provenance wrapper + metadata | `src/meta.rs`, `src/draft.rs` |
| Reduction | `src/reduce.rs` |
| Traversal-driven validation | `src/validator_typed.rs` |
| Validator types | `src/validator.rs` |
| Workbench HTML renderer | `src/workbench_simple.rs` (+ `src/workbench_simple.css`) |
| Reduction binary | `src/bin/reduce_extractions.rs` |
| Render binary | `src/bin/render_workbench_from_report.rs` |
| Round-trip contract check | `src/bin/roundtrip_check.rs` |
| Python extractor (Gemini call) | `scripts/spike_extract.py` |
| Flask + gunicorn entry point | `scripts/local_app_simple.py` |
| Acceptance harness | `scripts/spike_acceptance_check.py` |
| Manual deploy escape hatch | `scripts/deploy.sh` |
| Cloud Build pipeline | `deploy/cloudbuild.yaml` |
| Container | `deploy/Dockerfile` |
| Runtime deps | `deploy/requirements.txt` |

If a file isn't on this list, it shouldn't exist in the active
codebase. The non-active files are docs (`docs/*.md`), the test fixture
(`tests/test_deploy_config.py`), the unrelated `visualizer/` Vite app,
domain reference PDFs in `reference/`, and the receipts corpus in
`receipts/`.
