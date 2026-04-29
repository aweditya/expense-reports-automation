# Async OCR Jobs Journal

This file tracks implementation notes, reflections, regrets, and rollout observations for the async OCR job pipeline work on `feature/async-ocr-jobs`.

## 2026-04-29

### Starting assumptions

- The current hosted receipt flow is accurate enough to preserve as the fallback demo baseline.
- The highest product pain is not raw extraction quality right now; it is user experience around long-running uploads and unconditional OCR escalation.
- A branch is necessary because this work changes the orchestration model and could temporarily destabilize the demo flow.

### Reflections before implementation

- The most likely failure mode is accidental code bloat from trying to redesign OCR and job orchestration at the same time.
- The safe sequence is orchestration first, OCR gating second.
- A numeric confidence model would feel premature right now; checklist-based gating is easier to reason about and test.

### Things to watch closely

- Job identity versus artifact reuse. Those are easy to conflate.
- Hosted UI regressions around refresh, duplicate submissions, and stale queue state.
- Hidden coupling between bundle manifests, ledger state, and newly introduced job state.

### Phase 1 progress notes

- The first implementation slice is deliberately narrower than the full roadmap:
  - stage uploads synchronously
  - persist a job record
  - run the heavy `ingest_bundle_workspace run` step asynchronously
  - redirect the user to a resumable `/job/<job_id>` page
- This keeps the orchestration change isolated from OCR decision logic.

### First regression encountered

- The initial HTTP test failures were misleading: the new job routes were mostly correct, but the test harness deadlocked on the larger FA-facing home page.
- Cause:
  - the test helper instantiated `BaseHTTPRequestHandler` on the same thread that later tried to read the socket response
  - once the rendered HTML exceeded the socket buffer, the handler blocked writing before the client started draining bytes
- Fix:
  - run the handler on a separate thread in the HTTP test helper so client reads and server writes can proceed concurrently
- Reflection:
  - this was a good reminder that UI-surface tests can fail because the harness no longer matches the size/shape of the real page, not because the product logic regressed

### Hosted validation outcome

- Phase 1 was validated on the hosted Cloud Run site with a real English receipt upload.
- Validation bundle:
  - `async_job_receipt_smoke_20260429`
- Observed behavior:
  - `/upload` returned a `/job/<job_id>` redirect in under a second
  - refreshing/polling the job route was safe
  - the job completed and redirected to the review workbench
  - the resulting hosted packet reached `user_input_required` with no automation gaps
- Reflection:
  - the new orchestration flow fixed the worst UX failure mode without changing OCR behavior
  - the remaining weakness in Phase 1 is observability, not correctness: the job currently stays in a coarse `running / processing` state for most of its lifetime
