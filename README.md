# Stanford Expense Reports

AI-assisted expense-report extraction for Stanford CS/EE Faculty
Administrators. FAs upload receipt PDFs and images. The system
extracts structured fields with Gemini and Document AI, validates
them against Stanford portal rules, and emits two CSVs the FA
uploads to file the report.

Deployed at <https://34.160.32.50.nip.io> (Stanford SSO required).

## Entry point for maintainers

Read [`docs/onboarding.md`](docs/onboarding.md). It routes to the
rest of the docs in the correct order.

## Doc map

| Audience | Doc | Content |
|---|---|---|
| New maintainer | [`docs/onboarding.md`](docs/onboarding.md) | Reading order, first-week commands, known traps |
| Any | [`docs/SPEC.md`](docs/SPEC.md) | Architecture with mermaid diagrams |
| Engineer | [`docs/internals.md`](docs/internals.md) | Mechanism reference: extraction, validation, persistence, concurrency, refresh resilience, CI/CD |
| Ops | [`docs/deployment-guide.md`](docs/deployment-guide.md) | Stand-up runbook, service-account JSON setup, Cloud Build debug |
| Ops (fast) | [`docs/deploy-cheatsheet.md`](docs/deploy-cheatsheet.md) | One-page operational facts |
| FA end user | [`docs/fa-user-guide.md`](docs/fa-user-guide.md) | Website usage |
| Maintainer | [`docs/redesign-regrets.md`](docs/redesign-regrets.md) | Incident log — read before changes near flagged areas |

Pre-redesign design docs remain in `docs/` for historical context.
The docs above reflect the system as it stands.

## Quick start

Auth (once per machine):

```bash
gcloud auth login
gcloud auth application-default login
gcloud config set project soe-agile-agents
```

Setup:

```bash
./.venv/bin/pip install -r deploy/requirements.txt
./.venv/bin/playwright install chromium
cargo build
```

Run locally:

```bash
PORT=8088 VERTEX_PROJECT_ID=soe-agile-agents \
  USE_FIRESTORE_JOBS=1 USE_FIRESTORE_REPORTS=1 USE_GCS_ARTIFACTS=1 \
  ./.venv/bin/python scripts/local_app_simple.py
```

Open <http://127.0.0.1:8088>. Each upload incurs Vertex AI and
Document AI charges.

## Tests

```bash
cargo test
./.venv/bin/python -m unittest discover -s tests -p "test_*.py"
```

Prod E2E (real API costs, approximately USD 3 per full suite):

```bash
RUN_PROD_E2E=1 VERTEX_PROJECT_ID=soe-agile-agents \
  ./.venv/bin/python -m unittest tests.test_workbench_browser -v
```

## Deploy

```bash
git push origin main
```

Cloud Build trigger fires, runs tests, builds image, deploys to
Cloud Run. See [`docs/deployment-guide.md`](docs/deployment-guide.md)
§6.5 when the deploy fails.

## Project conventions

See [`CLAUDE.md`](CLAUDE.md) for non-negotiable workflow rules.
