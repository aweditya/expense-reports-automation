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
