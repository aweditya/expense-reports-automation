# Project Instructions

## Active redesign

A pipeline redesign is in progress on branch `redesign/single-call-extraction`.
The previous "UI-only, never touch the backend" scope is **superseded for files
relevant to that redesign**. Track current state in `docs/redesign-plan.md`.

End state: Python owns the Gemini extraction call (structured output typed
against `schema.yaml`), Rust owns reduction, validation, ledger, and workbench
rendering. Boundary is a typed JSON file on disk.

Outside the redesign, the historical scope (UI-only) still applies.

## Non-negotiable workflow rules

1. **Plan first, act second.** Update `docs/redesign-plan.md` as work progresses.
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

- `cargo test` runs the full Rust test suite.
- `python3 -m unittest discover -s tests -p "test_*.py"` runs the Python
  suite (matches what Cloud Build runs).
- `cargo test --lib review_workbench` runs workbench-specific tests.
- The workbench regression fixtures live in `fixtures/workbench_regressions/`.

## Schema artifacts

- `schema.yaml` is the source of truth for the expense report shape.
- `scripts/generate_schema_artifacts.py` regenerates `generated/`:
  `expense_report_model.rs`, `validation_rules.rs`, plus YAML mirrors. Run it
  after every `schema.yaml` edit.
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
gcloud auth application-default login           # ADC for spike_extract.py
gcloud config set project soe-agile-agents
gcloud config set run/region us-west1
gcloud config set builds/region global
```

When ADC isn't set up, a 401 from Vertex looks identical to a code bug.
Do this first.
