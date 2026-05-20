# FA-input plan — front-page form for general_information + foreign_activity_type

Planning doc for Phase 5 scope-completion: wire the FA-entered fields
the advisor identified (project memory 2026-05-02 + reaffirmed
2026-05-19). Mirrors the `docs/leapfrog-plan.md` pattern — design
decisions written down before code lands.

---

## 1. Why

`schema.yaml` declares a `general_information` block with these
T1/T4 fields:

- `payee.name`, `payee.affiliation`
- `event_name`
- `business_purpose.{who, what, when, where, why, key_30char}`
- `authorized_by`
- `rush_processing`
- `payment_method`

Plus `transaction_lines[*].common.foreign_activity_type` (per-line
enum: `conference | research_collaboration | fieldwork | other`).

**Today**: none of these are populated. Only `general_information.
category` gets filled (by the reducer's currency-based heuristic).
Every other field renders as `—` in the workbench because nothing
upstream sets them. Phase 5 originally planned a synthesis layer (T4)
to auto-derive event_name + business_purpose from conference receipts,
but the advisor said on 2026-05-02 to skip synthesis and have the FA
fill these directly. We kept the schema + tier vocabulary work from
that conversation (T4 was retained) but never wired the input path.

This plan wires the input path.

---

## 2. What

Add a `<fieldset>` ABOVE the file-upload table on the existing
`/upload` page. FA fills it out; on POST, Flask saves the values to
`fa_input.json` next to the uploaded files. The reduction binary
gets a new `--fa-input <path>` arg and fills `general_information.*`
+ propagates `foreign_activity_type` from that file.

```
FA fills fieldset            ─┐
+ picks files + kinds          │
                               ▼
POST /upload                   ─►  scripts/local_app_simple.py
                                       │
                                       ├──► save raw files to .scratch/uploads/<id>/files/
                                       └──► save fa_input.json to .scratch/uploads/<id>/
                                                │
                                                ▼ (subprocess)
                                        reduce_extractions
                                          --in extractions/
                                          --out reduced/report.json
                                          --fa-input fa_input.json   ← NEW
                                                │
                                                ▼
                                        ExpenseReport with
                                          general_information.* filled
                                          foreign_activity_type per line
                                                │
                                                ▼
                                        render_workbench_from_report
                                          (unchanged — reads what reducer wrote)
```

---

## 3. UI shape (what the FA sees)

A single fieldset, three columns where it makes sense, ABOVE the file
upload table. Labels match the schema field names so an FA reading the
workbench can map back without confusion.

Fields (with input types):

| Field | Input type | Required? | Notes |
|---|---|---|---|
| Payee Name | text | yes | |
| Payee Affiliation | dropdown | yes | `faculty / staff / student / postdoc / other` (matches schema enum) |
| Event Name | text | no | Hint: "Conference / event name. Leave blank for non-event reports." |
| Business Purpose — Who | text | yes | Hint: "Who attended (e.g. payee + 2 collaborators)" |
| Business Purpose — What | text | yes | Hint: "What was the activity" |
| Business Purpose — When | text | yes | Hint: "Date range or single date" |
| Business Purpose — Where | text | yes | Hint: "City + country" |
| Business Purpose — Why | text | yes | Hint: "How it advances Stanford research" |
| Business Purpose — Key (30 chars) | text, maxlength=30 | yes | Hint: "Short label for the report (≤30 chars)" |
| Authorized By | text | yes | Hint: "Approver name / SUNet ID" |
| Rush Processing | dropdown | yes | `no / yes`, default `no` |
| Payment Method | text | yes | Free-form for now (PCard / Personal / etc.) |
| **Foreign Activity Type** | dropdown | no | `(skip if domestic) / conference / research_collaboration / fieldwork / other`. Propagated to every foreign-typed transaction line. |

Required fields enforced by HTML5 `required` attribute. Server-side
validation in Flask catches the rare missing-required case and returns
a friendly error page.

---

## 4. Concrete changes by file

| File | Change | Est. LOC |
|---|---|---|
| `docs/fa-input-plan.md` | This doc | (S.0) |
| `src/fa_input.rs` (NEW) | Type + parser for `fa_input.json`. Helper `apply_to_report(report, fa_input)` that sets `general_information.*` with `kind: user_input` evidence and propagates `foreign_activity_type` to foreign transaction lines. | +130 |
| `src/lib.rs` | `pub mod fa_input;` | +1 |
| `src/bin/reduce_extractions.rs` | New `--fa-input <path>` arg; parse the file if provided; call `apply_to_report` after the existing reduction. | +20 |
| `scripts/local_app_simple.py` | New `<fieldset>` in HTML form. POST handler parses the new form fields, writes `fa_input.json`. Subprocess invocation of `reduce_extractions` gets `--fa-input` arg. | +130 |
| `docs/SPEC.md` | §1 layer table: add note about FA input; §2 component diagram: add fa_input.json file node + edge from Flask; §3 sequence diagram: FA form fields in the POST + fa_input.json write step + --fa-input arg to reducer; §8 file map: add `src/fa_input.rs` and the new JSON path. | +25 lines doc |
| Tests | A round-trip test in Rust: build a fake `fa_input.json`, apply to a fake report, assert fields landed in `general_information` with `user_input` evidence. | +50 |

**Total production: ~330 LOC across 6 files. About the size of
Leapfrog L.5.**

No new Python dep. Stdlib JSON only.

---

## 5. Stages

Each S.x is one commit, independently revertable.

| Stage | Description | Risk |
|---|---|---|
| **S.0** | Commit this planning doc | None |
| **S.1** | `src/fa_input.rs` type + parser + `apply_to_report` helper; unit tests. Not wired to any binary yet. | Low — pure additive |
| **S.2** | Flask form HTML + POST handler writes `fa_input.json`. Reducer doesn't read it yet — file is generated but unused. | Low — front-end only |
| **S.3** | `reduce_extractions --fa-input` arg; Flask subprocess invocation gets it; reducer calls `apply_to_report`. First real behavior change. | Medium — reducer behavior shift |
| **S.4** | Local end-to-end test (Flask up, submit form, inspect workbench). Tighten any UX rough edges. | Low |
| **S.5** | Push + Cloud Run verdict. FA tests with a real upload. | Standard |
| **S.6** | SPEC.md update + regrets sweep + close out. | Low |

---

## 6. Evidence shape for FA-entered fields

Per `_meta_convention`: FA-entered values use `kind: user_input` with
an `origin` string identifying the FA form (e.g.
`"origin: fa_upload_form"`). Workbench's existing `field_card_inner`
renders user_input evidence the same way as document_span minus the
spot-check icon (no source document to halo).

Concrete example for `business_purpose.what`:

```json
{
  "value": "Presented research at ASPLOS 2026",
  "_meta": {
    "confidence": "high",
    "confidence_reason": "Provided by FA on upload form.",
    "evidence": [
      {
        "kind": "user_input",
        "origin": "fa_upload_form"
      }
    ],
    "needs_review": false,
    "flags": []
  }
}
```

For `foreign_activity_type` propagation: every transaction line whose
expense_type ends in `_foreign` (e.g. `airfare_foreign`,
`business_meal_foreign`) gets the FA-selected value with the same
user_input evidence. Domestic lines stay None.

---

## 7. Risks + fallbacks

| Risk | Mitigation |
|---|---|
| FA submits form with blank required field | HTML5 `required` blocks at client; Flask validates server-side and returns a friendly error page (re-rendering the form with filled values preserved). |
| FA's report has both foreign + domestic transactions; foreign_activity_type doesn't make sense for every line | We propagate ONLY to foreign-typed lines. Domestic lines stay None (correct per schema's conditional). |
| FA leaves event_name blank (non-conference report) | `event_name` is optional. Reducer skips it. `business_purpose.key_30char` stays as FA-entered, doesn't try to derive from event_name. |
| FA forgets to fill the form, only uploads files | All required fields have HTML5 `required`. If somehow bypassed (programmatic POST), reducer raises a clear error rather than producing a half-populated report. |
| FA's form input has characters that break the JSON (quotes, unicode) | `json.dumps` handles escaping. Display in workbench escapes for HTML. Test with a stress payload (single + double quotes + emoji). |
| Reducer signature change breaks the local `acceptance_check.py --run` flow | `--fa-input` is optional. Reducer skips the apply step when omitted. Existing harness unchanged. |

---

## 8. Non-goals

Explicitly NOT in this stage:

- **Per-file `foreign_activity_type`** — FA picks one report-level value; all foreign lines get it. Per-file picker is a future enhancement only if a real FA report needs mixed purposes.
- **Phase 5 cross-doc synthesis (T4)** — auto-deriving event_name + business_purpose from conference registrations is deferred indefinitely per advisor.
- **Multi-payee / allocation_and_approvers block** — not currently filled by anything; future scope.
- **Per-diem / mileage expense kinds** — separate stage entirely.
- **Form state persistence across page reloads** — if the FA's form submission fails server-side validation, we re-render with values preserved; we do NOT support browser-back-button restoration or draft saving.
- **Validation that key_30char is actually ≤30 chars** — HTML5 `maxlength` is the only enforcement; reducer trusts the input. If overlong, the workbench truncates display.

---

## 9. Success criteria

After S.5 deploys:

1. FA opens the deployed app, sees the new fieldset above file uploads.
2. FA fills it (or skips optional fields), uploads receipts, submits.
3. Workbench loads with `general_information.payee.name`,
   `business_purpose.*`, etc. all showing the FA-entered values with
   "FA-entered" provenance text (and no spotcheck icon — there's no
   source document to halo).
4. Every foreign-typed transaction line has its `foreign_activity_type`
   set to whatever the FA chose.
5. Existing flow (no FA input — purely document-driven) still works
   via the `--fa-input` flag being optional.

---

## 10. Open questions resolved at plan-time

- **Affiliation enum**: matches schema (faculty / staff / student / postdoc / other)
- **Payment method**: text input (schema is freeform string); upgrade to dropdown later if FA wants
- **Rush processing**: dropdown default `no`
- **Foreign activity type default**: blank (FA must opt in); hint says "skip if domestic-only report"
- **Key 30char**: text input, no auto-fill; HTML5 maxlength=30

---

## 10a. Post-S.5 addendum (2026-05-19): polish + date cross-check

Three follow-ons after the initial S.5 deploy, batched as E.x stages
so they ship in one re-deploy alongside S.6 close-out:

- **E.1** — `VERTEX_PROJECT_ID` auto-fill from `gcloud config` at local
  startup. Cloud Run sets it via `--set-env-vars`; locals don't get it
  and the extractor error is opaque. Trivial.
- **E.2** — Validator pass: every transaction line's `common.date`
  must fall within the FA-entered `business_purpose.when` window.
  Out-of-window dates emit `ValidationIssueKind::ManualReviewRequired`
  with severity `Warning` — surfaces under "Needs review" in the
  workbench rail. **Why warning, not error**: airfare extractors today
  inconsistently return purchase/booking dates (vs. flight dates), so
  legitimately-pre-purchased airfare would false-positive. FA reviews,
  confirms, files. Future work (separate): tighten airfare prompts to
  always return travel date.

  Implementation: `parse_when_window(s)` in `src/fa_input.rs`
  round-trips the canonical format we produce in
  `write_fa_input` (`"YYYY-MM-DD"` or `"YYYY-MM-DD to YYYY-MM-DD"`).
  `check_dates_within_trip_window(report, &mut issues)` in
  `src/validator_typed.rs` does string comparison (YYYY-MM-DD sorts
  lexicographically — no `chrono` dep needed). Skips check entirely
  when `business_purpose.when` is unparseable or missing.

  **No schema change**: `common.date` and `business_purpose.when` are
  both already declared; the schema description literally says
  `validation: "Must fall within trip date window"`. We're
  operationalizing intent.
- **E.3** — Google Places autocomplete on the FA form's "Where" field.
  Stanford project already has Vertex enabled; enabling Places API on
  the same project is cheap. Single API key, debounced JS call,
  populates a `<datalist>`. Event name stays free-form text.
- **E.5** — Custom combobox to replace `<datalist>` for the "Where"
  field. The native `<datalist>` rendering is browser-controlled and
  ignores our light-theme CSS — Safari in dark mode shows a dark
  dropdown over a light form, which reads as inconsistent. Custom
  combobox keeps the same `/places/autocomplete` backend (E.3
  unchanged) but renders our own absolutely-positioned `<ul>` below
  the input. Implements the ARIA combobox 1.2 pattern: `role`
  attributes, `aria-activedescendant`, keyboard nav (↓/↑ to navigate,
  Enter to select, Escape to close, Tab advances focus and closes),
  click-outside dismissal, `mousedown` (not `click`) for selection so
  the focus-blur race doesn't eat the selection. ~120 LOC across
  HTML/JS/CSS in `scripts/local_app_simple.py`.

- **E.4** — "Mark as reviewed" per issue card. FA clicks a dismiss
  button on the issue → card hides, section count decrements, section
  hides if 0 active, rail hides if 0 active total. Field card loses
  its `.has-issue` highlight in sync. **No persistence**: workbench is
  a single-session artifact (FA opens, reviews, downloads, abandons);
  refresh wipes dismissals, which is fine because they're acknowl-
  edgements, not data. Single "Show N dismissed" toggle at top of
  rail lets the FA un-hide cards (with per-card "Restore" buttons) if
  they accidentally dismissed something. ~100 LOC across
  `src/workbench_simple.{rs,css}` + the inline JS. No SPEC change
  (validator/render contract unchanged; this is pure client-side
  state).

- **E.6** — PDF download of the workbench. "Print" button triggers
  `window.print()`; `@media print` CSS hides the rail / dismiss
  buttons / jump arrows / sticky chrome so the printed page shows the
  form cards + transaction lines cleanly. Browser handles the actual
  PDF generation via its "Save as PDF" dialog. Zero new deps,
  ~30 LOC. If a Stanford-portal-template-matching PDF is ever needed,
  that's a separate feature (server-side weasyprint or similar);
  E.6's scope is "give the FA a clean printable version of what
  they're already looking at."

## 11. Decision pre-conditions

Before starting S.1:

- [x] This doc is reviewed (this commit IS the review artifact).
- [x] Phase 6 leapfrog (L.6) is live + stable on Cloud Run.
- [x] No in-flight commits on `scripts/local_app_simple.py` that would
      conflict (note: task #46 HEIC pre-flight is running in a separate
      worktree to avoid this).

All three true as of 2026-05-19.
