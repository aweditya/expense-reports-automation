# Project Instructions

## Architecture

Stanford expense-report extraction pipeline. End-to-end flow:

1. FA uploads receipts at the deployed Cloud Run URL.
2. `scripts/local_app_simple.py` (Flask + gunicorn) saves them and runs
   the pipeline per upload:
   - **Extract** (Python): a per-kind extractor (`scripts/extract_meal.py`,
     `scripts/extract_transport.py`, …) makes one Gemini
     call per receipt with a structured `response_schema_meal.json` and
     writes one typed JSON per receipt.
   - **Reduce** (Rust): the `reduce_extractions` binary aggregates the
     per-receipt JSONs into one `ExpenseReport` (typed against
     `schema.yaml`-generated structs), deriving fields like total USD,
     trip date, and category along with their confidence.
   - **Render** (Rust): the `render_workbench_from_report` binary runs
     `validate_typed` against the report and emits a self-contained
     workbench HTML file the FA can review.
3. Workbench shows per-field confidence dots, provenance text, and a
   sticky issues rail; FA can download the JSON.

Boundary between Python and Rust is a typed JSON file on disk.
`schema.yaml` is the source of truth for field shapes and validation
rules; `scripts/generate_schema_artifacts.py` regenerates `generated/`
on every schema edit.

History of how this came together is in `docs/redesign-plan.md` and
`docs/redesign-regrets.md`.

## Non-negotiable workflow rules

1. **Plan first, act second.** Track in-flight multi-step work in a
   visible plan doc (e.g., `docs/redesign-plan.md` while it was active);
   update as steps complete.
2. **No code bloat.** No "for the future" abstractions, no half-finished
   implementations, no scaffolding without a concrete consumer. Three similar
   lines beat a premature abstraction.
3. **Frequent commits.** One logical change per commit, descriptive message.
4. **No inline scripts.** If it's a script, it lives as a real file in `scripts/`.
5. **Never write to `/tmp`** (or any system temp dir like `/var/folders`,
   `mktemp`). Use `./.scratch/` (gitignored) for ephemeral artifacts, or read
   from harness-provided task output files.
6. **CLI testing is sanity-only. The deployed Cloud Run site is the verdict.**
   Local CLI runs prove the loop works; the final yes/no comes from observing
   the hosted app on real receipts.
7. **Eyeball the artifact before claiming code is done.** For any code that
   produces a user-visible artifact (HTML, JSON, generated files), render
   the artifact and look at it before the commit lands. Unit tests check
   "doesn't crash"; they do not check "looks right." Once the workbench
   exists, that means *open it in a browser and verify the change looks
   correct* — not just running cargo tests.
8. **Track mistakes in `docs/redesign-regrets.md`** so they don't recur. Add
   an entry every time a mistake is caught, by me or by the user.
9. **Never trust a piped command's exit code.** `cmd | tee | tail; echo $?`
   captures the last command's exit, not the first. Either run unpiped, set
   `pipefail`, or inspect the output for FAILED markers before declaring success.
10. **Flag every new file before creating it.** Even small one-off scripts.
    "I'm adding `path/to/foo.rs` to do X" before the file appears, never
    after. One-off scripts are exactly the files that live forever uncalled.
11. **Update `docs/SPEC.md` whenever the architecture changes.** Adding a
    new layer, moving a responsibility between layers, changing the
    type-contract between layers, or adding/removing a major component
    all qualify. Update the relevant Mermaid diagrams + text and **render
    them in GitHub's preview** before claiming the SPEC change is done —
    Mermaid syntax that parses in your head can still fail in the viewer
    (see regrets log). Architecture should change rarely; when it does,
    SPEC.md is the artifact that has to stay true.

## Working style

- Before making a change: explain **why** the change is needed.
- After making a change: summarize **what** changed and why.
- Write tests proportional to the change. For throwaway spike scripts, the
  test is the inspectable output on real inputs, not unit tests for the
  spike's internals. For production code (Rust modules, Python production
  scripts), follow the existing pattern: `#[cfg(test)]` modules in Rust source
  files; tests under `tests/` for Python.
- Carefully consider the reversibility and blast radius of actions. Confirm
  before pushes, force operations, deletions of others' work, or anything that
  touches shared infrastructure.

## Build and test

- `cargo test` runs the full Rust test suite (~39 tests across the
  reduce / validator / workbench modules).
- `python3 -m unittest discover -s tests -p "test_*.py"` runs the
  Python suite (matches what Cloud Build runs; currently just the
  cloudbuild.yaml shape check).
- `./.venv/bin/python scripts/acceptance_check.py` runs the
  end-to-end acceptance harness against four real receipts in
  `receipts/`. By default it checks the cached `.scratch/spike/*.json`;
  pass `--run` to re-invoke Gemini (~3 min, costs API calls).
- `tests/test_workbench_browser.py` is a Playwright headless-Chromium
  regression test for the workbench HTML/JS. Skips itself when
  Playwright isn't installed. To enable locally:
  `./.venv/bin/pip install -r dev-requirements.txt && ./.venv/bin/playwright install chromium`.
  Dev-only — NOT in `deploy/requirements.txt`, so it doesn't ship
  to Cloud Run.

## Schema artifacts

- `schema.yaml` is the source of truth for the expense report shape.
- `scripts/generate_schema_artifacts.py` regenerates `generated/`:
  `expense_report_model.rs`, `validation_rules.rs`, plus YAML mirrors. Run it
  after every `schema.yaml` edit.
- `scripts/generate_response_schema.py` regenerates per-kind
  `generated/response_schema_<kind>.json` for each Gemini extractor.
  Run after any `schema.yaml` change to a per-kind detail block.
- **Required pre-push gate after ANY change to a detail block**:
  `VERTEX_PROJECT_ID=soe-agile-agents ./.venv/bin/python scripts/probe_response_schemas.py`.
  Sends one minimal live `generate_content` per schema (~$0.01 each,
  <2s per file) — catches Vertex's property-count ceiling rejections
  that `cargo test` + local SDK validation miss. Stage 9c shipped
  without running this and broke prod for the FA; the regrets log
  captures it. Known-broken schemas (no extractor wires them yet,
  e.g. conference_registration) are allowlisted in
  `probe_response_schemas.py::KNOWN_BROKEN` so the gate still passes
  for active schemas.
- The generator parses only the `expense_report:` top-level key; sibling keys
  like `_meta_convention:` are intentional documentation/spec living alongside
  the schema but outside codegen.

## Deployment

- Target: Google Cloud Run in the `soe-agile-agents` GCP project,
  region `us-west1`. The full facts table (URLs, image registry, IAP,
  service account, common gotchas) is in `docs/deploy-cheatsheet.md` —
  start there, don't rediscover.
- **Deploy gesture:** `git push origin main`. The Cloud Build trigger
  `deploy-on-push` in **us-west1** (not global!) fires on push to
  `^main$`, runs `deploy/cloudbuild.yaml`, and deploys to Cloud Run.
  `scripts/deploy.sh` exists as a manual escape hatch (redeploy without
  a code change, deploy a non-main branch, recover from a webhook
  hiccup) — don't use it for routine deploys, that just double-builds.
- To watch a build in flight after pushing, query the **regional**
  builds list: `gcloud builds list --region=us-west1 --project=
  soe-agile-agents --limit=5`. The default `gcloud builds list` looks
  at the global region and won't show trigger-fired builds.
- The cloudbuild pipeline runs Rust tests → Python tests → Docker
  build → push → `gcloud run deploy`. Test failures block deploy.
- Deployment-related changes are limited to: `scripts/local_app_simple.py`
  (host/port/binary resolution), `deploy/Dockerfile`, `deploy/cloudbuild.yaml`,
  `scripts/deploy.sh`. Do not modify Rust pipeline code purely for deployment.
- The Dockerfile uses a two-stage build: Rust compilation in builder stage,
  pre-compiled binaries copied to Python slim runtime.
- Cloud Run config: `--max-instances=1`, `--timeout=600`, ephemeral
  container-local storage for `.scratch/uploads/`.

### Auth checklist (run once per machine before a deploy session)

```bash
gcloud auth login                               # gcloud CLI commands
gcloud auth application-default login           # ADC for the per-kind extractors (extract_meal.py, etc.)
gcloud config set project soe-agile-agents
gcloud config set run/region us-west1
gcloud config set builds/region global
```

When ADC isn't set up, a 401 from Vertex looks identical to a code bug.
Do this first.
