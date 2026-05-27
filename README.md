# Stanford Expense Reports

AI-assisted expense-report extraction for Stanford CS/EE Faculty
Administrators. FAs upload receipt PDFs/images; we extract
structured fields with Gemini + Document AI, validate against
Stanford's portal rules, and emit two CSVs the FA uploads to file
the report.

Deployed at <https://34.160.32.50.nip.io> (Stanford SSO required).

---

## Start here

**New to this codebase?** Read [`docs/onboarding.md`](docs/onboarding.md)
first. It's the only entry point you need — it'll route you to
the rest of the docs in the right order.

5-minute orientation, 30-minute walkthrough, 2-hour deep dive.

---

## Docs map

| Audience | Doc | What's in it |
|---|---|---|
| New maintainer (you) | [`docs/onboarding.md`](docs/onboarding.md) | Reading-order index + first-day cheat sheet + gotchas |
| Anyone | [`docs/SPEC.md`](docs/SPEC.md) | Architecture spec with mermaid diagrams |
| Engineer | [`docs/internals.md`](docs/internals.md) | Code-mechanism deep dives (extraction, validation, persistence, concurrency, CI/CD, refresh resilience) |
| Ops engineer | [`docs/deployment-guide.md`](docs/deployment-guide.md) | Stand-up-from-scratch + service-account JSON + Cloud Build debug runbook |
| Ops engineer (fast) | [`docs/deploy-cheatsheet.md`](docs/deploy-cheatsheet.md) | One-pager of operational facts (URLs, IAM, region, gotchas) |
| FA (end user) | [`docs/fa-user-guide.md`](docs/fa-user-guide.md) | How to use the deployed website |
| Future-you | [`docs/redesign-regrets.md`](docs/redesign-regrets.md) | "What hurt when we shipped X" log — read before making changes near a flagged area |

Historical design docs (pre-redesign, kept for context, not
current): `docs/redesign-plan.md`, `docs/leapfrog-plan.md`,
`docs/durable-store-plan.md`, `docs/fa-input-plan.md`, etc. The
current docs above reflect the system as it stands today.

---

## Quick start

```bash
# Auth (once per machine)
gcloud auth login
gcloud auth application-default login
gcloud config set project soe-agile-agents

# Setup
./.venv/bin/pip install -r deploy/requirements.txt
./.venv/bin/playwright install chromium
cargo build

# Run locally with the full durable store
PORT=8088 VERTEX_PROJECT_ID=soe-agile-agents \
  USE_FIRESTORE_JOBS=1 USE_FIRESTORE_REPORTS=1 USE_GCS_ARTIFACTS=1 \
  ./.venv/bin/python scripts/local_app_simple.py

# Open http://127.0.0.1:8088
```

## Tests

```bash
cargo test                                                       # Rust (~127)
./.venv/bin/python -m unittest discover -s tests -p "test_*.py"  # Python (~168, ~50 skipped without RUN_PROD_E2E)
```

Prod E2E (~$3, ~25 min):

```bash
RUN_PROD_E2E=1 VERTEX_PROJECT_ID=soe-agile-agents \
  ./.venv/bin/python -m unittest tests.test_workbench_browser -v
```

## Deploy

```bash
git push origin main    # Cloud Build trigger fires, deploys to prod
```

That's the entire interface. See [`docs/deployment-guide.md`](docs/deployment-guide.md)
§6.5 if it breaks.

## Project conventions

See [`CLAUDE.md`](CLAUDE.md) for the non-negotiable workflow rules
this codebase has accumulated. They exist because of expensive
lessons; follow them.
