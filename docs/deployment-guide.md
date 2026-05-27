# Deployment guide

How to stand this system up — from a green-field GCP project, or
from a working project that needs maintenance. Audience: an engineer
(or operationally-minded admin) taking over the deployment story.
Assumes basic GCP + git familiarity, no Stanford-specific knowledge.

Reference companions:
- **`docs/SPEC.md` §5** — architecture diagram (what runs where)
- **`docs/deploy-cheatsheet.md`** — fast-reference facts (URLs, IAM,
  region, gotchas). Read it after this guide for day-to-day work.
- **`CLAUDE.md`** — project conventions if you're going to edit code.

---

## 1. The shape of the deployment

One Cloud Run service in GCP, deployed via Cloud Build on every push
to `main`. Architecture summary (see SPEC.md §5 for the diagram):

```
FA browser → HTTPS LB → Identity-Aware Proxy → Cloud Run container
                                                  ├── gunicorn + Flask (Python)
                                                  ├── Rust binaries (reduce + render)
                                                  ├── Vertex AI Gemini (extraction)
                                                  ├── Document AI OCR (grounding)
                                                  ├── Firestore (JOBS + reports — durable)
                                                  └── GCS (PDFs + extractions — durable)
```

Cloud Build trigger fires on git push, runs tests, builds the Docker
image, deploys to Cloud Run as a new revision. Old revisions stick
around for rollback.

---

## 2. One-time setup (green-field)

Skip this section if you're inheriting an already-working project.
Run-once steps; subsequent deploys are just `git push origin main`.

### 2.1 GCP project + billing

```bash
# Pick a project ID — e.g. expense-reports-prod
gcloud projects create expense-reports-prod
gcloud config set project expense-reports-prod
gcloud billing projects link expense-reports-prod --billing-account=<YOUR_BILLING_ACCT>
```

### 2.2 Enable required APIs

```bash
gcloud services enable \
  run.googleapis.com \
  cloudbuild.googleapis.com \
  artifactregistry.googleapis.com \
  containerregistry.googleapis.com \
  aiplatform.googleapis.com \
  documentai.googleapis.com \
  firestore.googleapis.com \
  storage.googleapis.com \
  iap.googleapis.com
```

### 2.3 Set defaults

```bash
gcloud config set run/region us-west1     # or your preferred region
gcloud config set builds/region us-west1  # Cloud Build trigger region
```

### 2.4 Firestore database (default)

```bash
gcloud firestore databases create \
  --location=us-west1 \
  --database='(default)' \
  --type=firestore-native
```

Set TTL policies in the GCP console (one-time, manual):
- Collection `jobs`, field `ttl` → 7-day expiration
- Collection `reports`, field `ttl` → 90-day expiration

Without TTL policies the documents linger but cost stays trivial.

### 2.5 GCS bucket (artifacts)

```bash
gcloud storage buckets create \
  gs://<project-id>-expense-reports-state/ \
  --location=us-west1 \
  --uniform-bucket-level-access \
  --public-access-prevention
```

Update `scripts/gcs_artifacts.py::BUCKET_NAME` if your bucket name
differs from the default.

### 2.6 Document AI processor

The OCR processor is **not** code-managed today; it's a one-time
manual create in the GCP console:

1. Console → Document AI → Create Processor → **Document OCR**
2. Region: `us` (multi-region)
3. Note the processor ID (looks like
   `projects/.../locations/us/processors/<hash>`)
4. Update the processor ID reference in `scripts/evidence_bbox.py`
   (search for the existing processor ID in the file)

The runtime service account needs `roles/documentai.apiUser`.

### 2.7 IAP (Identity-Aware Proxy)

Front the Cloud Run service with an external HTTPS load balancer, then
enable IAP on the backend. This is GCP boilerplate; the steps:

1. Console → Network Services → Load Balancing → Create
2. Backend: Serverless network endpoint group pointing at the Cloud
   Run service
3. Frontend: HTTPS, static IP (note it), managed cert against a
   domain or use `<ip>.nip.io`
4. Security → Identity-Aware Proxy → enable on the backend
5. Grant IAP-Secured Web App User role to the Google groups /
   individuals who should access the site

The FAs need to be members of whichever group you grant access to.

### 2.8 Cloud Build trigger

GitHub repo → Cloud Build → Create Trigger:
- Region: **us-west1** (must match `gcloud config set builds/region`)
- Event: push to branch
- Repo: your GitHub fork of this repo
- Branch regex: `^main$`
- Configuration: Cloud Build configuration file
- Location: `deploy/cloudbuild.yaml`
- Name: `deploy-on-push`

The trigger uses the project's default Cloud Build SA. Grant it:
- `roles/run.admin` (deploy Cloud Run revisions)
- `roles/iam.serviceAccountUser` (act as the runtime SA)
- `roles/storage.admin` (push images to Container Registry / Artifact
  Registry)

### 2.9 Runtime service account

The Cloud Run runtime defaults to the **compute SA**
(`<project-number>-compute@developer.gserviceaccount.com`). Grant it:
- `roles/aiplatform.user` (Vertex AI Gemini)
- `roles/documentai.apiUser` (Document AI OCR)
- `roles/datastore.user` (Firestore)
- `roles/storage.objectAdmin` (GCS bucket — scope to the bucket if
  you want least-privilege)

Or use a dedicated SA — set it via `gcloud run services update
expense-reports --service-account=<sa-email>`.

---

## 3. Deploy gesture

```bash
git push origin main
```

That's it. The trigger fires, Cloud Build runs the pipeline in
`deploy/cloudbuild.yaml`:

1. Rust tests (`cargo test`)
2. Python tests (`unittest discover`)
3. Docker build (two-stage: Rust builder → Python slim runtime)
4. Push image to `gcr.io/<project>/expense-reports:<short-sha>`
5. `gcloud run deploy expense-reports` with the new image + env vars

Test failures block deploy.

Watch a build in flight:

```bash
gcloud builds list --region=us-west1 --project=<project> --limit=5
gcloud builds log <build-id> --region=us-west1 --project=<project>
```

The trigger region matters — `gcloud builds list` without `--region`
queries the global region and won't show trigger-fired builds.

---

## 4. Environment variables

Set in `deploy/cloudbuild.yaml` (final `gcloud run deploy` step,
`--set-env-vars`):

| Var | Purpose |
|---|---|
| `VERTEX_PROJECT_ID` | GCP project for Vertex / DocAI / Firestore / GCS clients. Pass `$PROJECT_ID` (Cloud Build built-in). |
| `USE_FIRESTORE_JOBS` | `1` = JOBS dict reads/writes go through Firestore (durable). `0` = in-memory dict (loses state on container recycle). |
| `USE_FIRESTORE_REPORTS` | `1` = report.json + fa_input + edit history dual-written to Firestore. Required for cross-recycle survival of edits. |
| `USE_GCS_ARTIFACTS` | `1` = source PDFs + extraction JSONs dual-written to GCS. Required for the workbench rehydrate path. |

All three gates default to `0` if unset (in-memory + disk-only, dev
behavior). Production has all three on.

---

## 5. Local development

Two paths: use your personal ADC, or use a service-account JSON.

### 5.1 Personal ADC (most common for devs in the project)

```bash
gcloud auth login                       # for gcloud CLI commands
gcloud auth application-default login   # ADC for the SDKs
gcloud config set project soe-agile-agents
```

Then:

```bash
./.venv/bin/pip install -r deploy/requirements.txt
./.venv/bin/pip install playwright
./.venv/bin/playwright install chromium
cargo build
PORT=8088 VERTEX_PROJECT_ID=soe-agile-agents \
  USE_FIRESTORE_JOBS=1 USE_FIRESTORE_REPORTS=1 USE_GCS_ARTIFACTS=1 \
  ./.venv/bin/python scripts/local_app_simple.py
```

Open http://127.0.0.1:8088 in a browser, fill the form, upload a
receipt. Costs apply (Vertex + DocAI per upload).

### 5.2 Service-account JSON (for anyone without personal IAM)

Use this when you don't have a personal IAM binding on the project
but the project admin has issued you a service-account key. This is
the "run the website without Kayvon's gcloud" path.

#### Issuing a key (project admin, one-time per recipient)

```bash
PROJECT=soe-agile-agents

# Create a scoped SA for the recipient
gcloud iam service-accounts create expense-reports-runner-<alice> \
  --display-name="Expense reports runner — alice" \
  --project=$PROJECT
SA="expense-reports-runner-<alice>@$PROJECT.iam.gserviceaccount.com"

# Grant the runtime roles (least-privilege; same set as §2.9)
for role in \
    roles/aiplatform.user \
    roles/documentai.apiUser \
    roles/datastore.user; do
  gcloud projects add-iam-policy-binding $PROJECT \
    --member="serviceAccount:$SA" --role="$role"
done

# Bucket-scoped storage access (don't grant project-wide storageAdmin)
gcloud storage buckets add-iam-policy-binding \
  gs://soe-agile-agents-expense-reports-state/ \
  --member="serviceAccount:$SA" --role="roles/storage.objectAdmin"

# Issue a key — DO NOT commit this file
gcloud iam service-accounts keys create \
  ./alice-runner-key.json \
  --iam-account="$SA"
```

Hand the JSON to the recipient out-of-band (1Password share, secure
Drive folder, USB stick at a Stanford coffee). **Never** paste it in
Slack, email, or commit it to a repo.

Rotation: keys are long-lived. Set a calendar reminder to rotate
every 90 days. To rotate: create a new key, distribute, then delete
the old one with `gcloud iam service-accounts keys delete <old-key-id>`.

#### Using a key (recipient)

```bash
# One time — save the JSON somewhere your shell can read it
mkdir -p ~/.config/expense-reports
mv ~/Downloads/alice-runner-key.json ~/.config/expense-reports/
chmod 600 ~/.config/expense-reports/alice-runner-key.json

# Each shell session — point the SDK at the key
export GOOGLE_APPLICATION_CREDENTIALS=~/.config/expense-reports/alice-runner-key.json
export VERTEX_PROJECT_ID=soe-agile-agents

# Start the server
PORT=8088 USE_FIRESTORE_JOBS=1 USE_FIRESTORE_REPORTS=1 USE_GCS_ARTIFACTS=1 \
  ./.venv/bin/python scripts/local_app_simple.py
```

The SDK picks up the key automatically from `GOOGLE_APPLICATION_CREDENTIALS`
and proves identity to GCP without any `gcloud auth login` step.

#### Security checklist

- **Never** commit the JSON. Add `*-runner-key.json` to `.gitignore`
  if you keep it in the repo root.
- **Never** paste the JSON into chat / email / a screenshot.
- The key has API write access to Firestore + the GCS bucket. A
  leaked key = an attacker can corrupt or read every FA's reports.
- Rotate every ~90 days. Delete old keys after rotation.
- Prefer Workload Identity Federation (WIF) if you can — it
  replaces long-lived keys with short-lived OIDC tokens from your
  CI provider. Worth the effort if you're running this in CI; less
  worth it for an FA's laptop.

### 5.3 Tests

```bash
cargo test                                                  # Rust (~127 tests)
./.venv/bin/python -m unittest discover -s tests -p "test_*.py"  # Python
```

Prod E2E tests (cost real API calls, ~$3 for the full suite):

```bash
RUN_PROD_E2E=1 VERTEX_PROJECT_ID=soe-agile-agents \
  ./.venv/bin/python -m unittest tests.test_workbench_browser.TestProdFailureModes -v
```

---

## 6. Monitoring + debugging

### Cloud Run logs

```bash
gcloud logging read 'resource.type="cloud_run_revision"' \
  --project=<project> --limit=50 --format=json
```

Or use the Logs Explorer in the console. The app emits structured
JSON via `scripts/log_event.py` — filter on `jsonPayload.event` for
specific signals:

| Event | Meaning |
|---|---|
| `subprocess.start` / `subprocess.done` / `subprocess.fail` | Per-extract / reduce / render subprocess run with duration |
| `extract.retry` | Vertex returned a retryable error; we backed off |
| `rehydrate.success` / `rehydrate.failed` | Workbench cache miss → durable-state recovery attempt |
| `firestore_reports.save_failed` / `gcs_artifacts.upload_failed` | Best-effort dual-write hit an error (disk is still authoritative) |
| `workbench.delete_line` / `workbench.undo` | FA-side mutation |
| `add_receipts.start` | FA appended more receipts to an existing report |

### Live progress for a specific upload

`gcloud firestore documents describe jobs/<upload_id>` shows the
current phase, file list, errors.

### Failed deploys

```bash
gcloud builds list --region=us-west1 --limit=5
gcloud builds log <build-id> --region=us-west1
```

Tests usually fail first; the build log shows which test + the
traceback.

### Rollback

```bash
# List revisions
gcloud run revisions list --service=expense-reports --region=us-west1

# Route 100% of traffic to a known-good revision
gcloud run services update-traffic expense-reports \
  --region=us-west1 \
  --to-revisions=expense-reports-00xxx-yyy=100
```

This is **destructive-ish** — confirm before doing it in front of an
FA mid-session.

---

## 7. Cost shape

At ~10 uploads/day (current load):
- Vertex AI Gemini: $0.10 / receipt × ~50 receipts/day = ~$5/day
- Document AI OCR: $1.50 / 1K pages × ~150 pages/day = ~$0.25/day
- Cloud Run: free tier covers it
- Firestore: trivial ($0.05/month)
- GCS: trivial ($0.01/month)
- Cloud Build: free tier covers it

Total: **~$150/month** at current load. Order-of-magnitude scales
linearly with upload volume; the durable-store tier stays trivial.

---

## 8. What can go wrong

| Symptom | Likely cause | Fix |
|---|---|---|
| Deploy succeeds but service errors on every request | New env var added to code but not to cloudbuild.yaml | Add the var to `--set-env-vars` |
| `phase=lost` after a few minutes mid-upload | `USE_FIRESTORE_JOBS` somehow unset; in-memory state on a recycled container | Confirm env var; check Firestore IAM |
| FA gets 403 IAP page | Their SSO account isn't in the IAP-Secured Web App User binding | Grant access in IAP console |
| Vertex 429 / 503 | Quota or transient | Retry layer in `scripts/extractor_lib.py` handles it; check Cloud Logging for `extract.retry` events |
| `InvalidArgument: Property X contains an invalid nested entity` | New Firestore field has a shape Firestore can't store (e.g. arrays-of-arrays) | JSON-encode the field as a string in the Firestore wrapper |
| Build fails on `cargo test` | Rust code change broke a test | Read the failure, fix locally with `cargo test`, push again |
| Build fails on Docker push | Container Registry permission issue | Confirm Cloud Build SA has `roles/storage.admin` |

See `docs/redesign-regrets.md` for the historical "what we learned
when X broke" log.

---

## 9. Who to ask

Single-engineer project today. The author is the contact.

If the author is no longer reachable: the system is designed to be
operated by following this guide + `deploy-cheatsheet.md` +
`SPEC.md`. The code is intentionally small (~5K LOC Python + Rust)
and reading it top-to-bottom takes a focused day.

Start with:
- `scripts/local_app_simple.py` (the Flask app, routes + dispatcher)
- `src/workbench_simple.rs` (the rendered workbench HTML/JS)
- `schema.yaml` (the source of truth for everything)
