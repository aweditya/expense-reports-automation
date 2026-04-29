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

### Phase 2 first result

- The first conditional-pass policy was validated on the hosted site with the same English receipt used in the Phase 1 smoke.
- Validation bundle:
  - `async_job_receipt_smoke_gate_20260429`
- Observed behavior:
  - upload still returned a job redirect immediately
  - the job completed successfully and reached the normal FA workbench
  - the hosted bundle did **not** expose an OCR pass comparison artifact for that receipt, which confirms the secondary OCR lane was skipped
- Reflection:
  - this is the right proof for the first gating step: preserved correctness with less OCR work on an easy receipt
  - the next refinement should focus on richer stage visibility and broader confidence rules, not reintroducing unconditional passes

### Hosted gauntlet harness regression

- The first attempt to rerun the small hosted SROIE gauntlet on the async branch reported `0/4` success, but that turned out to be a test-harness bug rather than a hosted product regression.
- Cause:
  - `scripts/evaluate_hosted_receipt_flow.py` still assumed `upload_documents(...)` returned a raw bundle id
  - the async hosted flow now returns redirect metadata and often enters the `/job/<job_id>` path first
  - the harness then tried to use that metadata object as a bundle id and failed with `quote_from_bytes() expected bytes`
- Fix:
  - make the harness follow async job redirects the same way the browser flow does
  - add explicit unit coverage for both async `/job/...` uploads and direct `/bundle/...` uploads
- Reflection:
  - this was a useful reminder that once the UI flow changes, the hosted evaluator has to stay in lockstep with the actual browser contract or it will “prove” false regressions

### Hosted background-worker deployment issue

- The first real hosted gauntlet attempt on the async branch then exposed a deployment-level problem: the job page stayed in `running / processing` far longer than the underlying receipt smoke tests suggested it should.
- Likely cause:
  - the async runner executes as a background subprocess after the upload response returns
  - Cloud Run was deployed without always-allocated CPU, so background work could be throttled between polling requests
- Fix:
  - add `--no-cpu-throttling` to the Cloud Run deploy step
  - add a regression test that asserts the deploy configuration preserves always-allocated CPU for the async-job branch
- Reflection:
  - the branch is doing exactly what it should here: surfacing orchestration realities before they reach `main`
  - async UX on Cloud Run is not just an application concern; deployment semantics are part of the product behavior
