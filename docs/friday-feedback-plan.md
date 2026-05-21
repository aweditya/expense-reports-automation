# Friday-feedback plan — FA meeting 2026-05-20

Planning doc for the 7-stage incorporation of FA feedback received in
the 2026-05-20 meeting. Friday 2026-05-22 is the FA's test deadline.
Mirrors `docs/fa-input-plan.md` + `docs/leapfrog-plan.md` patterns —
design decisions written down before code lands so the scope stays
honest.

---

## 1. Context

A meeting with the target FA on 2026-05-20 surfaced two categories of
feedback:

**Correctness gap (caught by a screenshot of the live portal):**
my just-shipped E.7 CSV is wrong against Stanford's real
"Expense Lines Upload" portal. The `Copy of ERS Template.xlsm` I built
from is either an older or simplified version. The actual portal
columns are wider and use different expense-type strings (e.g.
`Lodging - Foreign and Domestic`, not `Lodging`).

**UX feedback (anything from "this draws my eye wrong" to "I want to
edit values"):** 8 distinct items, scoped below.

The two categories fold together because the CSV correctness fix is
itself FA-meeting feedback. Treating them in one plan keeps the
deploy story straight: Friday's revision should have all of it or be
explicit about what's deferred.

---

## 2. Stages (order = correctness first, then UX)

Each stage is one commit. Each stage either ships or is deferred —
no half-finished work.

| # | Stage | Files touched | Risk | ~LOC | Friday? |
|---|---|---|---|---|---|
| **1 (G)** | CSV format correction | `src/csv_export.rs`, tests | Low | 80 | Must. Correctness fix. |
| **2 (F1+F2+F3+C)** | Visual cleanup bundle: drop section labels + confidence text + summary cards + raise data weight over labels | `src/workbench_simple.{rs,css}` | Low | 80 | Yes |
| **3 (H)** | Copy-to-clipboard for business-purpose text | `src/workbench_simple.rs` (inline JS + button) | Low | 25 | Yes |
| **4 (A)** | Home screen: receipt upload moved to top, FA form below; async extraction starts when files are selected; "this takes time" message | `scripts/local_app_simple.py` | Med | 80 | Yes |
| **5 (B)** | Real SSE progress: server reports phase + per-receipt index; frontend shows real %; audio chime on done | `scripts/local_app_simple.py` (SSE endpoint), upload form JS, workbench done-signal | Med | 150 | Yes (fallback: light-B if SSE through Cloud Run hits an issue) |
| **6 (F4)** | Hide non-important fields under expand/collapse. **Visible**: amount, date, expense type, payee, business_purpose, foreign_activity_type. **Hidden behind "Show details"**: extras (printed_currency, merchant_address), evidence quotes, FX-conversion fields. | `src/workbench_simple.{rs,css}` | Med (partition is policy) | 80 | Yes |
| **7 (D)** | Editable fields with type guards. Click any value → inline edit. Save back via new Flask `POST /uploads/<id>/edit`. Date fields enforce DD-MMM-YYYY or YYYY-MM-DD; amount fields enforce numeric; text fields free-form. **No undo** (refresh wipes edits, persistence is post-Friday). | new Flask endpoint, workbench JS (edit-on-click + save), `src/workbench_simple.{rs,css}` | High | 180 | Yes — explicitly requested |

**Total: ~675 LOC across 7 commits.** Realistic 6-8h focused work
across today's evening + Thursday + Friday morning.

---

## 3. Stage-by-stage detail

### Stage 1 — CSV format correction (G)

**Why**: E.7's CSV is broken against Stanford's real portal. Wrong
columns, wrong expense-type strings, wrong date format.

**What changes**:

| Old (E.7) | New (real portal) |
|---|---|
| 4 columns: `Date \| Amount \| Expense Type \| Remarks` | 7 columns: `Line \| Expense Date \| Expense Currency \| Expense Amount \| USD Amount \| Expense Type \| Remarks` |
| `Lodging` | `Lodging - Foreign and Domestic` |
| `Airfare` | `Airfare - Foreign and Domestic` |
| `Business Meal` | `Business Meal` (unchanged?? — verify with FA, currently best-guess) |
| ISO date `2024-09-02` | `02-Sep-2024` (DD-MMM-YYYY) |
| no currency col | `BRL - Brazilian Real` (ISO code + name) |
| amount in USD only | original currency amount AND USD amount in separate cols |

**Implementation**:
- New `format_portal_date(iso: &str) -> String` — `2024-09-02` → `02-Sep-2024`
- New `currency_code_to_full(code: &str) -> String` — `BRL` → `BRL - Brazilian Real`. ISO 4217 lookup table for the common 30-50 currencies; falls back to `<code> - <code>` for unknown.
- `map_expense_type` updated with the `- Foreign and Domestic` suffix where the portal uses it. Best-effort still — surface to FA at next review.
- `report_to_lines_csv` emits 7 columns. Line number auto-increments from 1.
- All 9 existing csv_export tests updated for new shape; add 3 new for the new format helpers.

**Open question (defer in-code)**: which expense_type strings need
the `- Foreign and Domestic` suffix? Screenshot only shows Lodging
and Airfare. I'll assume the suffix applies to every type that has
foreign/domestic variants in our schema (airfare_*, lodging_*,
business_meal*, ground_transport_*); FA-review at deploy.

### Stage 2 — Visual cleanup bundle (F1+F2+F3+C)

**Why**: FA feedback called out four distinct visual problems that
all live in `workbench_simple.{rs,css}`. Bundling because all touch
the same files and ship together.

**What changes**:
- **F1**: drop `<h3 class="subsection-title">` "Section 1" / "Section 3" wording. Replace numeric labels with descriptive text already on each subsection (Payee, Business Purpose, etc.).
- **F2**: drop `field-reason--high` text prefix ("High: "). Keep the colored confidence dot + the reason text after the colon. Confidence is conveyed by the dot's color alone.
- **F3**: drop the `<section class="summary-cards">` rendering at end of page (Trip Date / Total USD / Category / Confidence). Hero already shows lines + status; bottom cards duplicate.
- **C**: raise data weight over label weight in `.field-card`. Currently labels are 13px-semibold and values inherit body text. Flip to labels 11px-uppercase-grey, values 15px-medium-dark.

### Stage 3 — Copy-to-clipboard for business purpose (H)

**Why**: FA said "would also be really nice just to be able to
copy-paste the business expense, rather than downloading."

**What changes**:
- New `<button>` next to the existing "Download Business Purpose text" link. Inline JS uses `navigator.clipboard.writeText(text)`.
- Text content embedded in a `<script>` tag at render time (escaped JSON) so the JS has the same blob the .txt download has.
- Keep the download too (FAs who prefer files).
- Toast on success ("Copied to clipboard").

### Stage 4 — Home screen restructure (A)

**Why**: FA said: "Move the receipt upload to the very top and
filling out the forms below so receipt upload gets started; friendly
message to indicate why things take time."

**What changes**:
- Reorder upload-form.html: file-upload fieldset first, FA detail fieldset second, submit button at bottom.
- "This takes about 2 minutes — feel free to keep filling the form" message inline.
- **Async-start**: when files are selected (not on submit), POST them to a new `/upload/preflight` endpoint that kicks off extraction in a background thread, returns an upload_id. Submit then POSTs the FA form data + upload_id; server consolidates.
- This is the architectural change for stage 4. SPEC update bundled into the SPEC commit at the end.

**Risk**: async extraction needs server-side state. For Cloud Run
with `max-instances=1`, in-process dict keyed by upload_id is fine
(state survives within a single instance). Multi-instance would need
external storage — not a Friday concern.

### Stage 5 — Real SSE progress + audio (B)

**Why**: FA said: "Better UI for deciding when the job is done (like
a sound, etc.). Some progress update. Progress bar: how do you know
when its done, dynamic progress bar, maybe some audio signaling
finish."

**What changes**:
- New Flask endpoint `GET /upload/progress/<upload_id>` returns `text/event-stream`. Each phase change emits an event:
  ```
  event: phase
  data: {"phase": "extract", "current": 3, "total": 7}
  ```
- Phases: `extract` (per-receipt), `reduce`, `render`, `done`.
- Server-side state: dict keyed by upload_id, updated by the worker thread (stage 4 setup).
- Frontend: EventSource subscribed to the stream; progress bar fills as `current/total`; "Done!" + audio chime on `done` event. Audio is a small base64-embedded WAV (<5KB) so no CDN.
- Cloud Run supports SSE over HTTP/2 — verify on first deploy.

**Risk**: SSE through Cloud Run with `max-instances=1` should work
fine. Background-thread + Flask request streaming is a common
pattern. Fallback: if SSE doesn't work, drop to polling
`GET /upload/status/<upload_id>` every 2s — same JS handler shape,
less efficient but reliable.

### Stage 6 — Hide non-important under expand/collapse (F4)

**Why**: FA said: "Things that are not important should be hidden
and then you can expand and see."

**Partition** (FA-approved):

| Visible by default | Hidden behind "Show details" |
|---|---|
| amount | printed_currency (extras) |
| date | merchant_address (extras) |
| expense type | evidence quotes |
| payee.name + affiliation | FX-conversion fields (original_amount, original_currency, exchange_rate) |
| business_purpose.* | source_document filename / type |
| foreign_activity_type | spotcheck halos (still clickable to view) |

**Implementation**:
- Per transaction line: split `render_line_common` into "primary" +
  "secondary" field lists. Render primary always; render secondary
  inside a `<details>` element with `<summary>Show details</summary>`.
- `<details>` is native HTML; no JS needed for expand/collapse.
- Per general_information: same pattern.

### Stage 7 — Editable fields with type guards (D)

**Why**: FA said: "Make the fields updatable."

**What changes**:
- Each `field-card` value becomes click-to-edit: click → text input
  appears with current value, Enter saves, Escape cancels.
- Save POSTs to `/uploads/<id>/edit` with `{path: "<field path>", value: "<new value>"}`.
- Flask endpoint loads `report.json`, applies edit by walking the path, writes back. Re-renders workbench HTML to disk (so subsequent GETs see the edit).
- Frontend update: card value swaps to new text inline.
- **Type guards** (the safety net since no undo):
  - Date fields: validate `\d{4}-\d{2}-\d{2}` OR `\d{2}-[A-Z][a-z]{2}-\d{4}`.
  - Amount fields: validate `^\d+(\.\d{1,2})?$`. Reject "$1,234.56" — strip the formatting on FA's behalf, then validate.
  - Text fields: free-form. Reject empty if the field is required.
- Validation failure → input flashes red border, save blocked, no POST sent.
- No undo affordance. Refresh wipes edits (no persistence per the deferred E).

**Risk**: server-side write path. New endpoint, new code path
through report.json. Re-rendering workbench HTML synchronously after
edit means edit-then-refresh shows the new value but the in-memory
state on the FA's tab can diverge if they have stale data. MVP
accepts that.

---

## 4. SPEC.md updates

Architecture changes:
- Stage 4 adds `/upload/preflight` endpoint
- Stage 5 adds `/upload/progress/<id>` SSE endpoint
- Stage 7 adds `/uploads/<id>/edit` write endpoint
- Stage 5 introduces server-side per-upload state (in-process dict)

Batched into one SPEC commit at the end of the round, mmdc-rendered
before push (per the regrets-promoted default).

---

## 5. Deferrals (explicit)

- **E** (persistence / multi-request) — FA explicitly said "nice to
  have, not necessary for Friday." Park.
- **#54** (retire text-matching fallback) — stability window
  unchanged from prior plan; still hours since L.6, not weeks.

---

## 6. Mistake-avoidance reminders

From `docs/redesign-regrets.md`:
- Don't reach for heredoc inline scripts. If a check is worth running,
  it's worth a real file under `scripts/` or `tests/`.
- Never write to `/tmp` — only `.scratch/`.
- Render SPEC.md diagrams via `npx -y @mermaid-js/mermaid-cli` before
  commit; option (b) "flag unrendered" is reserved for mmdc failures.
- Surface scope decisions in chat, don't absorb them.
- LOC estimates: count out the real pieces, round up.
- Co-Authored-By trailer on every commit.

---

## 7. Decision pre-conditions

Before starting stage 1:
- [x] Plan doc reviewed (this commit is the review artifact).
- [x] Friday bundle confirmed.
- [x] D scope confirmed: MVP + minimal type guards.
- [x] B scope confirmed: real SSE.
- [x] F4 partition confirmed.
