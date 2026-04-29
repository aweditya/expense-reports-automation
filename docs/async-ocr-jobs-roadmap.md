# Async OCR Jobs Roadmap

## Goal

Move the hosted receipt flow from a long-running synchronous upload into a production-style asynchronous job pipeline that:

- returns control to the user quickly
- preserves progress across refreshes
- conditionally escalates OCR work only when the fast lane is not good enough
- keeps the FA-facing UI minimal
- preserves the current demo behavior as a rollback point

## Product Constraints

- OCR-facing validation is judged primarily through the hosted website, not just local tests.
- The acceptance bar is arbitrary real English receipts, not just curated corpus examples.
- The FA-facing UI should show simple states like `Processing`, `Needs review`, and `Ready for review`, not internal OCR jargon.
- The current `main` branch remains the stable demo baseline. This work happens on `feature/async-ocr-jobs`.

## Current Problems

1. Upload requests block until OCR, extraction, projection, and review artifact generation complete.
2. Refreshing during a long run feels broken because the user sees "another bundle is being processed" instead of a resumable status page.
3. Every `vertex-gemini-sdk` receipt run forces a secondary OCR lane, even for easy receipts.
4. The frontend has no durable job model, no progress state, and no cancellation semantics.
5. Duplicate uploads are treated as conflicts instead of a first-class retry/resume workflow.

## Target Architecture

### Separation of concerns

- **Artifact cache**
  - keyed by normalized document hash
  - stores normalized upload bytes, OCR artifacts, grounding previews, and extraction sidecars
  - safe to reuse across jobs
- **Job model**
  - keyed by bundle/report context, not raw file hash alone
  - owns status, stage, progress, retries, cancellation, and user-facing state
- **Bundle projection**
  - remains the place where schema transaction lines and readiness are derived

### Job lifecycle

The hosted upload flow should become:

1. `POST /upload` validates request, stages uploads, creates a job record, and returns quickly.
2. User is redirected to `/job/<job_id>`.
3. The job page polls status.
4. Background worker advances the job.
5. On success, the job page redirects to `/bundle/<id>/workbench`.

### Job states

- `accepted`
- `queued`
- `normalizing_uploads`
- `ocr_fast_pass`
- `extracting_fields`
- `scoring_receipt`
- `escalating`
- `ocr_fallback_pass`
- `reprojecting`
- `ready_for_review`
- `failed`
- `canceled`

### User-visible status copy

- `Processing your receipt`
- `Double-checking a few fields`
- `Ready for your review`
- `This run failed`
- `This run was canceled`

## Confidence-Gated Escalation

## Principle

Do not use a single blended numeric score initially. Use a receipt-type-aware checklist.

### Fast-pass success checklist for English receipts

The fast lane can stop if all of the following are true:

- merchant is present
- transaction date is present
- total is present
- currency is present or safely defaultable
- a schema transaction line is created
- no OCR-caused automation gap remains
- no hard arithmetic contradiction is detected

### Additional requirements for foreign receipts

- original currency is present
- original amount is present
- transaction date is stable enough for FX lookup
- FX conversion succeeds

### Escalation triggers

Escalate if any of the following hold:

- key OCR fields are missing
- no schema transaction line is created
- classification is weak or generic when the receipt looks structured
- grounding for key fields is missing on a hard photographed receipt
- amount/date parsing is inconsistent
- pass disagreement is strong

### Escalation ladder

Run the lowest-cost next step only:

1. fast OCR pass on normalized original
2. projection and checklist scoring
3. localized crop OCR if needed
4. preprocessing retry (`contrast_boosted`, `binarized`, `grayscale`) if needed
5. secondary `table_focused` OCR pass if needed
6. FA review with clear reason if still unresolved

## Job Data Model

Initial durable job record fields:

- `job_id`
- `bundle_id`
- `workspace_bundle_id`
- `status`
- `stage`
- `created_at_epoch_ms`
- `updated_at_epoch_ms`
- `retry_after_seconds`
- `cancel_requested`
- `error_message`
- `document_ids`
- `document_hashes`
- `artifact_refs`
- `escalation_reasons`
- `current_run_id`
- `result_bundle_href`

Initial persistent storage approach:

- job state: repo already uses filesystem persistence; first slice can keep job JSON in the managed workspace
- artifacts: reuse current workspace artifact layout
- follow-up production step: move job state and artifacts to durable hosted storage

## Implementation Phases

### Phase 1: Job model and status surface

Deliverables:

- job record schema
- hosted `/job/<job_id>` status page
- async `POST /upload` entrypoint
- status polling endpoint
- duplicate in-flight upload handling that returns the existing job instead of an error
- lightweight background runner that advances one persisted job outside the request handler

Acceptance criteria:

- refreshing the page during OCR shows status instead of a dead-end error
- the user can reopen the bundle/job after a long run without restarting the work
- no OCR logic changes yet

Implementation note:

- keep Phase 1 intentionally small: synchronous staging + asynchronous `run` execution
- do not introduce confidence gating, cancellation, or Cloud Tasks in the same slice

### Phase 2: Confidence checklist and staged OCR decisions

Deliverables:

- explicit `ReceiptEvaluation` / `ReceiptEscalationDecision` structure
- fast-pass checklist per receipt class
- conditional secondary OCR path
- human-readable escalation reasons

Acceptance criteria:

- easy receipts stop after the fast lane
- hard receipts still escalate and produce the same or better schema output
- hosted receipt flows remain stable

### Phase 3: Cancellation and partial-result persistence

Deliverables:

- `cancel_requested` job flag
- best-effort cancel endpoint
- stage checkpointing so partial OCR artifacts survive cancellation/failure

Acceptance criteria:

- user can cancel a long-running job
- retry can reuse prior artifacts when safe

### Phase 4: Artifact reuse by normalized file hash

Deliverables:

- normalized file hash cache for OCR artifacts
- reuse policy scoped to artifact cache, not job identity
- bundle-context-safe job dedup semantics

Acceptance criteria:

- same receipt in a different report can reuse artifacts without becoming the same job
- repeated upload of the same file to the same bundle returns or resumes the same logical job

## Testing Strategy

### Primary validation

For OCR-facing behavior, validate through the hosted website flow:

- submit upload
- observe job state transitions
- confirm final workbench behavior
- verify schema projection and readiness outcome

### Supporting automated tests

Keep rigorous unit and integration coverage for:

- job state transitions
- duplicate job detection
- cancellation semantics
- confidence checklist decisions
- render logic for the status page and queue transitions
- structured regression around reviewed/manual fields

### Hosted acceptance scenarios

At minimum, each phase should be exercised against:

- one clean English receipt
- one photographed English receipt with background clutter
- one foreign-currency English receipt
- one duplicate/in-flight retry scenario

## Rollback and Safety

- Preserve `main` as the current demo checkpoint.
- Keep commits small and deployable.
- Do not remove the current synchronous path until the async path has hosted validation.
- Prefer additive feature flags or route boundaries for the first cut.

## Known Risks

- introducing async jobs without durable storage can create new failure modes
- confidence checklists can be too strict or too loose without enough real receipts
- upload dedup can accidentally merge distinct user intents if tied too closely to file hash
- cancellation while Gemini is in-flight is best-effort only

## Immediate Next Slice

1. Add a best-effort cancel path for hosted async jobs.
2. Persist stage checkpoints or reusable intermediate artifacts so retries do not redo safe completed work.
3. Surface `cancel requested` / `canceled` / `stalled` states clearly on the hosted job page.
4. Validate cancellation and retry behavior through the hosted website before moving on to artifact-cache deduplication.
