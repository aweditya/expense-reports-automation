# Docs

## Current docs (read these)

These reflect the system as it stands today. Start with
`onboarding.md` if you're new.

| Doc | Audience | Purpose |
|---|---|---|
| [`onboarding.md`](onboarding.md) | New maintainer | Reading-order index, first-day cheat sheet, gotchas |
| [`SPEC.md`](SPEC.md) | Anyone | Architecture spec with mermaid diagrams |
| [`internals.md`](internals.md) | Engineer | Code-mechanism deep dives — extraction, multi-call, validation, persistence, concurrency, refresh resilience, CI/CD, local dev |
| [`deployment-guide.md`](deployment-guide.md) | Ops engineer | Stand-up from scratch + service-account JSON + Cloud Build debug runbook |
| [`deploy-cheatsheet.md`](deploy-cheatsheet.md) | Ops engineer (fast) | One-pager of facts (URLs, IAM, region, gotchas) |
| [`fa-user-guide.md`](fa-user-guide.md) | FA end user | How to use the deployed website |
| [`redesign-regrets.md`](redesign-regrets.md) | Maintainer | "What hurt when we shipped X" — read before touching flagged areas |

## Historical / design docs (context only)

Pre-redesign design notes and roadmaps. Kept because they explain
*why* certain decisions were made, not what the system does today.
Don't trust them for current behavior — read the current docs
above for that.

- `redesign-plan.md` — the master redesign plan when we ripped out
  the old multi-stage pipeline
- `leapfrog-plan.md` — OCR token-id grounding migration
- `durable-store-plan.md` — the Firestore + GCS design before we
  built it
- `fa-input-plan.md` — FA form fieldset design
- `fa-schema-rationale-roadmap.md` — schema layering rationale
- `friday-feedback-plan.md` — meeting-feedback triage
- `code-audit-2026-05-25.md` — pre-shipping audit snapshot
- `async-ocr-jobs-journal.md`, `async-ocr-jobs-roadmap.md` —
  superseded async ingestion design
- `getting-up-to-speed.md`, `system-architecture.md`,
  `document-facts.md`, `bundle-synthesis.md`, `review-workbench.md`,
  `review-submission-ledger.md`, `ui-workbench-redesign.md`,
  `local-app.md`, `real-document-ingestion.md`,
  `ingestion-workspace.md`, `demo-playbook.md`,
  `feedback-capture.md` — pre-redesign docs describing the older
  architecture

If you're tempted to read one of these to understand current
behavior: stop, and read the corresponding current doc instead.
The historical docs are valuable but easy to mistake for current.
