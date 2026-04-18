# UI Workbench Redesign

This document turns the current FA workbench feedback into a concrete implementation plan.

## Problem Statement

The current workbench is a deterministic static review page. It is useful for inspection, but it is not yet the right interaction model for an FA who needs to:

- see a queue of unresolved items
- correct model-filled values in place
- enter missing values directly
- inspect evidence in context, not in a detached side rail
- open the raw supporting document that produced a field

In short, the current workbench behaves like a rendered packet, while the target experience is an interactive review workstation.

## Design Direction

The target information architecture is:

- left rail: issue queue and unresolved-item navigation
- center column: editable filing form
- contextual evidence: embedded inside each field card
- document preview: opened on demand from evidence links, not always visible

This intentionally removes the dedicated right-hand evidence index as the primary evidence UX.

## Phase 1

Phase 1 is the first concrete implementation step and is what the code changes in this pass target.

### Goals

- keep the left-hand queue
- replace static field text with review controls
- render missing values as editable empty controls, not just `[missing]`
- allow machine-filled values to be edited in place
- move evidence into each field card
- add clickable raw-document links from evidence
- add an in-page document preview modal

### Non-Goals

- persistent edit storage
- multi-user collaboration
- highlighted OCR bounding boxes on top of page images
- full issue queue live-resolution logic

## Editing Model

Each field card should render according to `entry_mode` from the review packet:

- `computed_readonly`: readonly input; missing values remain an automation gap
- `model_prefill_review`: editable input initialized with the machine value
- `user_confirmed_input`: editable input or textarea, blank if missing

Field cards should surface local state:

- missing
- needs review
- edited
- readonly

The copy button should always copy the current field-control value, not just the original rendered value.

## Evidence Model

Evidence should be rendered directly inside the field card, under an expandable section such as `Evidence (N)`.

Each inline evidence block should show:

- evidence type
- source document
- page when available
- origin when system-generated
- excerpt when available
- a raw-document link when the evidence came from an uploaded file

## Raw Document Viewing

The local app should expose bundle document files over HTTP so the workbench can link to them.

Preferred route shape:

- `/bundle/<bundle_id>/document/<document_id>/<filename>`

The workbench should use relative links so the static HTML still works when served from the local app’s bundle routes.

## Phase 2

Phase 2 adds persistence and typed review-save behavior:

- submit edited values back into a typed corrected-draft artifact
- mark queue items resolved when values are supplied or confirmed
- record edit deltas into the feedback/ledger system
- rewrite `draft.yaml`, `validation.json`, `readiness.json`, `review_packet.json`, `review_workbench.html`, and `ledger.json` to the latest reviewed version

## Phase 3

Phase 3 turns the page into a fuller workstation:

- side-by-side raw document viewer with page-aware preview
- richer field widgets for enums, booleans, and structured lists
- save-and-recompute review loop
- export links for the edited current state
- versioned review-session persistence via the bundle workspace

## Status

All three phases in this redesign are now implemented in the repo.

Remaining future work is mainly beyond this redesign scope:

- OCR bbox overlays once the evidence contract carries bounding boxes everywhere
- richer multi-user collaboration semantics
- a more polished production web frontend on top of the same review-session contract
