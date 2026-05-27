# Onboarding — your first day with this codebase

You've just been handed the keys to this system. The previous
maintainer wrote this so you can be productive in days, not weeks.
Read this in order; skip nothing in the 30-minute path.

This file is a **reading order index**, not the content itself.
Each step points you at the doc or source file you should actually
spend time on.

---

## What this system does, in 3 sentences

Stanford Faculty Administrators (FAs) upload receipt PDFs/images.
We extract structured fields with Gemini + Document AI, validate
them against Stanford's portal rules, and emit two CSVs (one per
portal page) that the FA uploads to Stanford to file the expense
report.

The hard parts are:

- making the AI extraction trustworthy enough that FAs don't
  re-key everything, and
- making the deployed website survive Cloud Run container
  recycles without losing FA work.

---

## The 5-minute orientation

```mermaid
graph TB
  FA[FA's browser]
  IAP[Identity-Aware Proxy<br/>Stanford SSO]
  CR[Cloud Run container<br/>Flask + Rust binaries]
  V[Vertex AI Gemini]
  D[Document AI OCR]
  FS[Firestore]
  GCS[GCS bucket]

  FA -->|HTTPS| IAP
  IAP --> CR
  CR -->|extraction| V
  CR -->|OCR token grounding| D
  CR <-->|JOBS, reports| FS
  CR <-->|source PDFs, extractions| GCS
```

One Cloud Run service, behind Stanford SSO, that talks to:
- **Vertex AI Gemini** — does the structured extraction
- **Document AI** — does OCR with bbox coordinates (for spot-check
  highlighting on the workbench)
- **Firestore + GCS** — durable store so a container recycle
  doesn't lose your work

If this picture doesn't make sense yet, read it again after the
30-minute walkthrough.

---

## The 30-minute walkthrough

Read in this order. Don't skip; each doc assumes you've read the
previous.

| # | Doc | Time | Why |
|---|---|---|---|
| 1 | `docs/fa-user-guide.md` | 5 min | What the user sees. You'll be debugging their bug reports against this mental model. |
| 2 | `docs/SPEC.md` — sections 1, 2, 5 | 10 min | Layered architecture (extract → reduce → validate → render) + deployment topology. |
| 3 | `docs/internals.md` §1, §1.1, §2, §8 | 10 min | Code layout, "where the data lives" diagram, end-to-end pipeline trace, persistence story. |
| 4 | `docs/deploy-cheatsheet.md` | 5 min | The 1-pager of operational facts (URLs, region, IAM, gotchas). Reference, not study. |

After this you'll be able to talk about the system at meetings and
know roughly which file to open for a given concern.

---

## The 2-hour deep dive

When you have a real task (fix a bug, add a feature), read these
in this order. Each is ~30 lines focused on one concern; you'll
absorb them faster than you expect.

### Code path: an upload, source-by-source

1. `scripts/local_app_simple.py` — the Flask entry. Read top to
   bottom; ~1600 lines. Routes first (search for `@app.`),
   pipeline orchestration second.
2. `scripts/extract_meal.py` — the simplest per-kind extractor.
   ~10 lines that call into:
3. `scripts/extractor_lib.py` — shared Gemini call + retry
   wrapper + multi-call helper. ~250 lines.
4. `scripts/evidence_bbox.py` — Document AI OCR + token-id
   grounding. ~400 lines.

### Code path: reduce + render

5. `src/reduce.rs` — combines per-receipt JSONs into one
   `ExpenseReport`. Pure function, no IO.
6. `src/validator_typed.rs` — validates the report; produces
   issues, never mutates.
7. `src/workbench_simple.rs` — renders the FA-facing HTML. The
   biggest file (~2000 lines) but mostly straightforward
   `html.push_str(...)` calls.
8. `src/csv_export.rs` — emits the two Stanford-portal CSVs.

### Type contracts

9. `schema.yaml` — the source of truth for everything.
10. `generated/expense_report_model.rs` — Rust types generated
    from schema.yaml. **Don't edit.** Read for shape.
11. `src/meta.rs` — `Wrapped<T>` + `FieldMetadata`. Every
    extracted field is wrapped with confidence + evidence.

Now you have a working mental model.

---

## The first-day cheat sheet

Things you'll need in the first week:

### Run it locally

```bash
# Auth (once per machine)
gcloud auth login
gcloud auth application-default login
gcloud config set project soe-agile-agents

# Setup
./.venv/bin/pip install -r deploy/requirements.txt
./.venv/bin/playwright install chromium
cargo build

# Start the server (with the full durable store on)
PORT=8088 VERTEX_PROJECT_ID=soe-agile-agents \
  USE_FIRESTORE_JOBS=1 USE_FIRESTORE_REPORTS=1 USE_GCS_ARTIFACTS=1 \
  ./.venv/bin/python scripts/local_app_simple.py
```

Open <http://127.0.0.1:8088>. Costs apply per upload (Vertex +
DocAI calls hit the real APIs).

### Run the tests

```bash
cargo test                                                       # Rust (~127)
./.venv/bin/python -m unittest discover -s tests -p "test_*.py"  # Python
```

For prod verification (real API costs ~$3 for full suite):

```bash
RUN_PROD_E2E=1 VERTEX_PROJECT_ID=soe-agile-agents \
  ./.venv/bin/python -m unittest tests.test_workbench_browser.TestProdFailureModes -v
```

### Deploy

```bash
git push origin main
```

That's the entire interface. Cloud Build picks it up, runs tests,
deploys. ~6 min to first traffic on the new revision.

### When the deploy breaks

`docs/deployment-guide.md` §6.5 has the diagnostic flowchart +
failure-pattern table.

### When the FA sees a bug

`docs/fa-user-guide.md` §8 is the user-facing troubleshooting
table. For the underlying mechanism:

- Workbench bug → `src/workbench_simple.rs`
- Extracted field wrong → `scripts/extract_<kind>.py` + the
  receipt's saved JSON at
  `.scratch/uploads/<id>/extractions/<basename>.json`
- Edit didn't stick → check `_save_report_state` + the Firestore
  doc at `reports/<id>`

---

## The "I want to change X" cookbook

| You want to | Read | Then edit |
|---|---|---|
| Add a new expense kind (visa, fee, …) | `internals.md` §4 (13-step checklist) | `schema.yaml` → regen → new `extract_*.py` → workbench render arm + validator arm |
| Add a validation rule | `internals.md` §7 | `schema.yaml` for declarative; `validator_typed.rs` for procedural |
| Change a workbench field's label / format | `internals.md` §7 list | `src/workbench_simple.rs` (the relevant `render_*` function) |
| Tune Gemini retry behavior | `internals.md` §6.3 | `scripts/extractor_lib.py::RETRY_MAX_ATTEMPTS` + `_retry_with_backoff` |
| Bump extract concurrency | `internals.md` §6.1 | `EXTRACT_MAX_PARALLEL` env var (or `extract_all` default) |
| Persist a new field across recycle | `internals.md` §8 | `firestore_reports.set_report` + `_rehydrate_upload` |
| Add a deploy env var | `internals.md` §9.4 | `deploy/cloudbuild.yaml` `--set-env-vars` |

If your task isn't in this table, the right next read is whatever
`internals.md` section is closest to the concern.

---

## What WILL hurt the first time you touch it

These are the gotchas the previous maintainer wishes they'd known.
All have regrets-log entries with the original incident.

| Trap | What to do instead |
|---|---|
| Editing `schema.yaml` and forgetting to regenerate | Always run both `generate_schema_artifacts.py` + `generate_response_schema.py`, then `probe_response_schemas.py` for any per-kind block change |
| Adding a field to `reports/<id>` Firestore doc that contains arrays-of-arrays | JSON-encode it as a string; Firestore rejects nested arrays |
| Inline `python -c` for "quick" introspection | Write a real script in `scripts/`. The regrets log has 7+ slips on this; the rule is firm |
| Writing to `/tmp` | Use `.scratch/` (gitignored) |
| Trusting `cmd | tee | tail; echo $?` | The exit code is `tail`'s, not `cmd`'s. Inspect output for FAIL markers |
| Adding HTML5 validation (`pattern=` / `min=` / etc.) without a Playwright load test | Chromium's strict `/v` regex mode rejects things JS's `/u` accepts. `tests/test_workbench_browser.py::test_upload_form_loads_with_all_fa_fields` is the guard |
| Editing `src/workbench_simple.rs` HTML in a string + skipping the eyeball | Generate a fixture render + open it in Chrome. Visual bugs don't show up in `cargo test`. |

---

## When you get stuck

In priority order:

1. **Read `docs/redesign-regrets.md`.** Half the gotchas you'll
   hit are documented there with the original incident. Search by
   keyword first.
2. **Run the full prod E2E suite.** If it passes, the issue is
   local to your change. If it fails, the failure tells you what
   broke.
3. **Read the Cloud Logging output.** Every meaningful event is
   structured (`scripts/log_event.py`). Filter on
   `jsonPayload.event` in the Logs Explorer.
4. **Open the source files in the order from the deep-dive
   section above.** The code is ~5K LOC of Python + Rust; reading
   top-to-bottom takes a focused day.

---

## What's deliberately not here

Things you might expect a doc to cover that we explicitly don't:

- **A formal API reference.** The Flask routes are the API; read
  `scripts/local_app_simple.py`. There are ~15 routes total.
- **A schema dictionary.** `schema.yaml` is short enough to read
  in 5 minutes. It's the source of truth; any docstring would
  drift.
- **A separate Rust crate doc.** `src/` modules have ~1 sentence
  of header doc each. Read the file.
- **A diagrams-as-code repo.** Mermaid blocks live inline in the
  docs that need them. GitHub renders them automatically.

These are kept absent on purpose: documentation that drifts is
worse than no documentation. The code + this set of docs is the
contract.

---

## Where to go next

- New maintainer just onboarded: read `docs/internals.md` end to
  end (≈1 hour). Worth it.
- Need to deploy or debug ops: `docs/deployment-guide.md`.
- Want the "why we built it this way" history:
  `docs/redesign-plan.md` + `docs/redesign-regrets.md`.
- Need to file a bug-fix PR: see `CLAUDE.md` for the project's
  non-negotiable workflow rules. (Yes, follow them. They're the
  result of expensive lessons.)
