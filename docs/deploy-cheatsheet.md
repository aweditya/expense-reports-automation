# Deploy cheatsheet

Single page of facts about the deployed system, so I don't rediscover them
each session. If something here is wrong, fix the code/config first, then
update this file.

## What runs where

| Thing | Value |
| --- | --- |
| GCP project | `soe-agile-agents` |
| Cloud Run service | `expense-reports` |
| Region | `us-west1` |
| Direct Cloud Run URL | https://expense-reports-wgnivgelea-uw.a.run.app |
| Public-ish URL | https://34.160.32.50.nip.io (HTTPS LB → IAP → Cloud Run) |
| Image registry | `gcr.io/soe-agile-agents/expense-reports:<short-sha>` |
| Service account | `603261681824-compute@developer.gserviceaccount.com` (default compute SA) |
| Auth | IAP (Identity-Aware Proxy) on the LB; Cloud Run is `--no-allow-unauthenticated` |
| Memory | 2 GiB |
| CPU throttling | off (`--no-cpu-throttling`) |
| Timeout | 600s |
| Min/max instances | 1 / 1 |

## Cloud Build

| Thing | Value |
| --- | --- |
| Config | `deploy/cloudbuild.yaml` |
| Source upload bucket | `gs://soe-agile-agents_cloudbuild` (gcloud-managed; manual submits only) |
| Trigger | `deploy-on-push` in **us-west1** — fires on push to `^main$`, runs `deploy/cloudbuild.yaml`. Connected to repo `aweditya/expense-reports-automation`. |
| Logs | `gcloud builds log <build-id> --project=soe-agile-agents --region=us-west1` for trigger-fired builds; drop `--region` for manual submits. |
| Substitutions | Trigger sets `COMMIT_SHA`, `SHORT_SHA`, `BRANCH_NAME`, etc. automatically. Manual submits via `scripts/deploy.sh` pass only `COMMIT_SHA`. |

## Document AI (workbench spot-check grounding)

| Thing | Value |
| --- | --- |
| Processor | `projects/603261681824/locations/us/processors/4d07d363581d419d` |
| Display name | `expense-receipts-ocr` |
| Type | `OCR_PROCESSOR` (pure OCR + bboxes; not field extraction) |
| Region | `us` (Document AI multi-region) |
| API endpoint | `us-documentai.googleapis.com` |
| Created | 2026-05-16 |

Called once per receipt at extract time to produce word-level bboxes
that the workbench uses for the spot-check halo. PDFs and images both
go through the same processor. Consumer is `scripts/evidence_bbox.py`
(Phase 6 Stage B.2 onward); result populates `_meta.evidence[].bboxes`
in the typed JSON.

Return format: normalized polygon vertices (0-1 range, top-left
origin). For axis-aligned overlay take min/max of x and y per token.

The Cloud Run compute SA already has Document AI access via its
existing `roles/editor` binding — no extra IAM grant needed.

## Deploy gesture

```bash
git push origin main           # this is the deploy. The deploy-on-push
                               # trigger in us-west1 picks it up.

# Watch the build that just got created:
gcloud builds list --region=us-west1 --project=soe-agile-agents --limit=3

# Manual escape hatch (redeploy without code change, deploy a non-main
# branch, or recover from a webhook hiccup) — NOT for routine deploys
# (would double-build with the trigger):
scripts/deploy.sh
scripts/deploy.sh <short-sha>  # tag the image with an explicit SHA
```

The trigger is **regional** (us-west1), so trigger-fired builds do NOT
appear in `gcloud builds list` without `--region=us-west1`. The default
global region only shows manually submitted builds.

## Auth checklist (run once per machine)

```bash
gcloud auth login                               # gcloud CLI commands
gcloud auth application-default login           # ADC for the per-kind extractors (extract_meal.py, etc.) local runs
gcloud config set project soe-agile-agents
gcloud config set run/region us-west1
gcloud config set builds/region global          # cloudbuild.yaml uses global
```

When ADC isn't set up locally, the per-kind extractors 401 and it's not
obvious whether the bug is auth or code. Do this first.

## Verify the deployed app from the CLI

```bash
# Direct Cloud Run URL (no IAP) — this works with a gcloud identity token
TOKEN=$(gcloud auth print-identity-token)
curl -H "Authorization: Bearer $TOKEN" \
  https://expense-reports-wgnivgelea-uw.a.run.app/

# IAP-fronted URL — needs an IAP-scoped JWT, which requires the IAP
# client ID. Easier to use a browser session for the IAP path.
```

For visual verification, just open https://34.160.32.50.nip.io in a
browser — IAP handles auth via the SSO session.

## Health probes that have caught real bugs

- `GET /` returns the upload form (200) — means gunicorn + Flask are up.
- POST a real receipt to `/upload` and follow the 303 — exercises the
  full extract → reduce → render pipeline.
- Workbench HTML loads, transaction summary totals are non-zero, and
  the "Download JSON" link 200s with a parseable report.

## Common mistakes (see also: `redesign-regrets.md`)

- `_PROJECT_ID` is **not** a substitution to pass — `$PROJECT_ID` in
  cloudbuild.yaml is Cloud Build's built-in. Only pass `COMMIT_SHA`.
- Cloud Run does **not** auto-set `$GOOGLE_CLOUD_PROJECT`. The deploy
  step in `cloudbuild.yaml` sets `VERTEX_PROJECT_ID=$PROJECT_ID`
  explicitly. Don't rely on auto-discovery.
- The local `.venv/` accumulates packages that the Cloud Build container
  doesn't have. A green local Python suite isn't proof Cloud Build will
  pass; the deploy gate is the proof.
