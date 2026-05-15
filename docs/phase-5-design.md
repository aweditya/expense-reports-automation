# Phase 5 Design Walkthrough

## Purpose

Phase 5 introduces a new pipeline layer (cross-document LLM synthesis), a
new per-document JSON shape pattern (no `common` block), and a new tier
in the schema vocabulary (T4). Previous phases let LLM-vs-rules
decisions be made implicitly during schema design and prompt writing —
which surfaced as bugs (validator's wrong-scope rules; FX placeholder's
silent assumption). To avoid repeating that, this doc walks every new
field through the framework explicitly **before any code is written**.

Sign-off on this doc unlocks Phase 5 Stage 2.

## Framework recap

For each new field, we ask:

1. **Determinism**: Is the answer uniquely determined by the input?
   - Yes → leans rule-based
   - No → leans LLM (judgment call)
2. **Format closure**: Is the input space a finite, enumerable set of formats?
   - Closed → rules viable
   - Open → LLM more robust to format variance
3. **Stakes**: What's the cost of getting it wrong?
   - High (money, dates, identity) → bias toward rules; LLM only with predicate-checks + audit trail
   - Low (display string, narrative) → LLM acceptable
4. **Operation type**:
   - Extractive ("find this value in the doc") → either viable; rules give an audit trail
   - Generative ("compose / infer / classify") → LLM only

Composite as a 2x2:

```
                    │ Closed problem        │ Open problem
                    │ (finite formats)      │ (variable formats)
────────────────────┼───────────────────────┼─────────────────────
HIGH stakes         │ Rules                 │ LLM extract +
(money, dates,      │                       │ rule-validate
 identity)          │                       │
────────────────────┼───────────────────────┼─────────────────────
LOW stakes          │ Either                │ LLM
(display, narrative)│                       │
```

**The meta-principle**: the LLM's job is to convert raw documents into
structured data, plus a small amount of cross-document semantic
reasoning. After that, everything is structured and rules take over.

## Tier vocabulary (refined for Phase 5)

| Tier | Definition | Producer |
|---|---|---|
| **T1** | FA-entered | Human (in the FA portal/form) |
| **T2** | Rule-derived (deterministic computation, single-document OR cross-document) | Rust `reduce.rs` |
| **T3** | LLM-extracted from a single document | Python+Gemini per-doc extractor |
| **T4** (NEW) | LLM-synthesized across multiple per-doc JSONs for fuzzy reasoning a rule cannot do | Python+Gemini synthesis call |

**Cross-document is orthogonal to tier.** T2 has always read across
documents (`total_usd` sums across receipts). T4 is specifically for
cross-document operations *that need LLM judgment* — semantic joining,
inference, generation. Cross-document deterministic operations (date
range, venue union, cost sum) stay T2.

---

## `conference_registration_details` (per-doc, T3 extractor)

Per-receipt detail block emitted by `extract_conference_registration.py`.
The receipt carries the purchase information for one conference
registration line. Mirrors the per-kind detail-block pattern of
`meal_details` / `airfare_details` / `lodging_details`.

Modeling this against the only registration system in our corpus today
(Whova). Format-agnostic prompt should generalize to ACM RegOnline /
Cvent / Eventbrite / USENIX once those receipts exist.

| Field | Type | Tier | Reasoning |
|---|---|---|---|
| `conference_name` | string | **T3** | Open format (every receipt prints it differently); high stakes (FA-relevant); extractive (find in doc). LLM extracts the printed conference name as-is; canonicalization happens in T4 synthesis later. |
| `ticket_type` | string | **T3** | Open format ("Main Conference Only", "Two Day Workshops/Tutorials", "Full Pass + Banquet"). Variable across registration systems. LLM extraction. |
| `attendee_name` | string | **T3** | Open format (name appears in different fields per system); high stakes (compliance); extractive. |
| `registration_system` | enum (`whova`/`cvent`/`acm_regonline`/`eventbrite`/`usenix`/`other`) | **T3** | Closed enum, but inferring it from format is a judgment call (look at logo, URL, ID format). LLM with prompt-encoded mapping; default `other` when unrecognized. |

**Detail-block leaf count: 4.** Comfortably under the ~5-leaf single-call
ceiling. Single-call extractor (no multi-call orchestration needed).

**Fields intentionally NOT in detail block (rationale):**

| Candidate | Why excluded |
|---|---|
| `attendee_status` ("ACM Student Member") | Useful but not yet validated as FA workflow signal. Captured in `remarks` for now. Promote to detail block later if the FA confirms it matters. |
| `early_bird` (bool) | Inferable from price + ticket name. Captured in `remarks`. |
| `registration_id` (e.g., "WHV-YKQ2BWN") | Audit trail field, belongs in `extras` (parallel to airfare's ticket_number being in detail today — TODO: revisit). For now: `extras.registration_id`. |
| `order_confirmation` (e.g., "ch_3Sv6...") | Same audit-trail rationale. `extras.order_confirmation`. |
| `credit_card_last_four` | Same. `extras.credit_card_last_four`. |

**Common block fields populated by this extractor (existing pattern, T3 each):**
`date` (= order_date), `line_amount_usd`, `original_currency`,
`original_amount`, `expense_type` (=`conference_registration`),
`country_of_activity`, `foreign_activity_type`, `remarks` (free text
including attendee_status + early_bird notes).

**Schema.yaml change**: `expense_type` master enum already has
`conference_registration`. No enum addition needed; just need the new
`conference_registration_details` block in the conditional schema.

**Backward compat**: this is a brand-new detail block; doesn't affect
existing meal/transport/lodging/airfare paths.

---

## `supporting_conference_doc` (per-doc, T3 extractor, NO `common` block)

NEW per-document JSON pattern. Supporting documents (program PDF, paper
schedule PNG, conference papers PDF, etc.) are not expenses — they don't
belong as transaction lines. They contribute structured context that
reduction aggregates into general_information fields.

The shape has NO `common` block (no date/amount/expense_type — these
docs aren't expenses) and NO transaction-line counterpart in
`expense_report_model.rs`. The deserialized struct is a sibling of
`ExtractedReceipt`, not a flavor of it.

Each field is independently optional — a paper-only PDF fills
`papers_listed` and maybe nothing else; a program PDF fills
`scheduled_dates` + `venues_mentioned` + `workshops_listed`. Reduction
collects whatever's populated across all supporting docs in the
upload.

| Field | Type | Tier | Reasoning |
|---|---|---|---|
| `conference_name_as_printed` | string (Wrapped, with `_meta`) | **T3** | Open format; extractive. Helps the synthesis layer canonicalize names across documents. |
| `doc_kind` | enum (`program`/`schedule`/`papers`/`other`) (Wrapped) | **T3** | Closed enum but classification is judgment-based. LLM picks one based on document content. |
| `scheduled_dates` | bare array of ISO date strings | **T3** | Open format (every program prints schedules differently); extractive (parse explicit "Schedule for Sunday, March 22" lines into ISO dates). Bare array — no per-entry `_meta` (cost-effective for potentially long lists; reduction doesn't need per-entry confidence). |
| `venues_mentioned` | bare array of strings | **T3** | Open format; extractive. Captures named rooms / venues / cities mentioned anywhere in the document. |
| `papers_listed` | bare array of `{title: string, authors_string: string}` | **T3** | Open format (paper listings vary by program style); extractive. The `authors_string` is left as raw text (e.g. "Rubens Lacouture (Stanford), Nathan Zhang (Sambanova), …") — synthesis-layer reasoning will parse author identity for participant_role inference. |
| `workshops_listed` | bare array of strings | **T3** | Open format; extractive. Workshop names from program documents. |

**Why bare arrays instead of `_meta`-wrapped per-entry**: same rationale
as lodging's `nightly_rates` and airfare's `segments`. These are
collections of small structured items; per-entry confidence would balloon
the schema and the model's output budget without earning its keep. Reduction
treats the array's emptiness as the confidence signal (empty → unknown;
non-empty → trust the entries).

**Detail-block leaf count (Wrapped fields only): 2** (`conference_name_as_printed`,
`doc_kind`). Single-call extractor. Bare arrays don't count toward the
ceiling.

**Backward compat**: brand-new per-doc shape. Existing extractors and
the `ExtractedReceipt` struct unchanged. The reducer needs to learn to
read this new file pattern (a sibling type), but the existing
deserialization path for `ExtractedReceipt` is untouched.

---

## `synthesis_conference_bundle` (T4 narrow output)

Output of `scripts/synthesize_conference_bundle.py`. A SINGLE LLM call
fed all `conference_registration_*` and `supporting_conference_doc_*`
per-doc JSONs from one upload, asked to emit a small set of fuzzy
fields that no rule could produce.

**Strict design discipline**: the response_schema contains ONLY fuzzy
fields. Anything that could be derived from per-doc JSONs by a rule
(date range, venue union, cost sum) is structurally absent from the
schema. The LLM is incapable of emitting them. Reduction handles those
deterministically from the per-doc JSONs.

| Field | Type | Tier | Reasoning |
|---|---|---|---|
| `canonical_event_name` | string (Wrapped) | **T4** | Fuzzy semantic join across "ASPLOS" / "ASPLOS 2026" / "ASPLOS '26" mentions in different docs. No rule can do entity canonicalization correctly across format/spelling variants. Validation: post-synthesis check that some token from this name appears in some input JSON (catches hallucination). |
| `participant_role` | enum (`attendee`/`presenter`/`organizer`/`other`) (Wrapped) | **T4** | Inferred from "is the registrant in any supporting doc's `papers_listed[].authors_string`?" Author-name matching is fuzzy ("N. Sobotka" vs "Nathan Sobotka" vs "SRIRAM ADITYA MR" style); LLM handles it; rule-based matching would be brittle. |
| `business_purpose_what` | string ≤120 chars (Wrapped) | **T4** | Generative composition: "attending ASPLOS 2026" / "presenting paper at ASPLOS 2026". No rule can write a sentence. |
| `business_purpose_why` | string ≤200 chars (Wrapped) | **T4** | Generative composition: e.g., "to present the paper 'Streaming Tensor Program' on dynamic parallelism and engage with the architecture community." No rule alternative. |
| `source_filenames` | bare array of strings | **provenance** | List of per-doc JSON filenames the synthesis read. Used by the workbench to render "synthesized from 5 documents: …". Not a tier'd output value — just a citation. |
| `synthesis_confidence` | enum (`high`/`medium`/`low`) | **provenance** | Synthesis's overall self-rating. Drives the `needs_review` default on the lifted general_information fields. |

**Fields intentionally NOT in synthesis output (rationale):**

| Candidate | Why excluded |
|---|---|
| Conference date range | T2 — `min`/`max` of supporting docs' `scheduled_dates`. Rule-aggregated in reduction. Hallucination-proof. |
| Venue list | T2 — union of `venues_mentioned` across supporting docs. Rule-aggregated. |
| Total registration cost | T2 — sum of receipt amounts (already covered by `total_usd`). |
| Workshop list | T2 — union of `workshops_listed` across supporting docs. |
| Paper titles | T2 — flat-map `papers_listed` from supporting docs (we already have them; no synthesis needed). |
| Conference website URL | NOT extracted at all; could be added to per-doc extraction later if useful. |

**Total Wrapped fields in synthesis output: 4.** Tiny. Trivial single-call.

**Backward compat**: synthesis is a new optional file. When no conference
docs are present in an upload, synthesis doesn't fire and no
`synthesis_conference_bundle.json` is written. Reduction's lift logic
checks for the file's presence and no-ops if absent. Legacy reports
(meal/transport/lodging/airfare only) are not affected.

---

## Re-tiering of existing schema.yaml fields

Phase 5 changes the producer of several `general_information` fields
that are currently T1 (FA-fills). Re-tiering reflects the architectural
truth that conference uploads now drive these fields.

| Field | Current tier | New tier | Producer when conference docs present | Producer when no conference docs |
|---|---|---|---|---|
| `general_information.event_name` | T1 | **T4** | Synthesis lift from `canonical_event_name` | Falls through unfilled; FA fills (T1 fallback) |
| `business_purpose.what` | T1 | **T4** | Synthesis lift from `business_purpose_what` | T1 fallback |
| `business_purpose.why` | T1 | **T4** | Synthesis lift from `business_purpose_why` | T1 fallback |
| `business_purpose.who` | T1 | **T4** | Composed from synthesis `participant_role` + receipt's `attendee_name` | T1 fallback |
| `business_purpose.when` | T1 | **T2** | Min/max of supporting docs' `scheduled_dates` | T1 fallback |
| `business_purpose.where` | T1 | **T2** | Union of supporting docs' `venues_mentioned` | T1 fallback |
| `business_purpose.key_30char` | T1 | **T2** | Derived from event_name + earliest scheduled date (e.g., `ASPLOS-2026`) | T1 fallback |

**The "T1 fallback" pattern**: a field marked T4 (or T2) in
schema.yaml's `source:` annotation describes its **primary producer
when the architectural conditions are met**. When those conditions
aren't met (no conference docs uploaded), the field stays unfilled and
the FA fills it manually via the portal — same as today.

This is consistent with how `lodging_details.daily_rate` works today:
nominally T2, but only populated when reduction has nightly_rates to
average. When the nightly_rates breakdown is missing, daily_rate stays
unfilled and the FA fills.

**Backward-compat implications of re-tiering**:

- **Validator**: the `required:` flag on these fields stays unchanged.
  They remain required regardless of tier. A null value still flags as
  `MissingRequiredField`. The tier change does not affect the validator.
- **Workbench**: the renderer doesn't dispatch on tier — it just shows
  the value or "—" + needs_review based on `meta`. No render change.
- **Existing reports**: a report uploaded today (no conference docs)
  triggers no synthesis; the T4-marked fields stay unfilled; the
  workbench shows them as missing-fields in the issues rail (same as
  today). Identical FA experience.
- **Reports with conference docs**: the T4-marked fields get
  pre-populated by synthesis with `medium` confidence + `needs_review`.
  The FA reviews and either accepts or overrides — net less manual
  work, same audit chain.

---

## Backward-compat principle (system-wide)

Every Phase 5 code path is conditional on conference data being present
in an upload. The pattern:

- New extractors (`extract_conference_registration.py`,
  `extract_supporting_conference_doc.py`) are dispatched by the FA's
  per-file `kind` choice in the upload form — they only run when the FA
  selects the new dropdown options
- Synthesis (`synthesize_conference_bundle.py`) is invoked by
  `local_app_simple.py` AFTER per-doc extractions complete, and only if
  any `conference_registration_*.json` exists in the upload's
  extractions directory
- Reduction's new aggregations (dates, venues) early-return when no
  `supporting_conference_doc_*.json` files exist
- Reduction's new lifts (event_name, business_purpose) check for
  `synthesis_conference_bundle.json` presence and no-op if absent
- Workbench's new rendering paths (`render_conference_registration_details`
  and the supporting-doc summary card) only fire when the
  corresponding data is present in the report

Net: a report containing only meal/transport/lodging/airfare receipts
goes through Phase 5 code as a series of no-ops. The FA experience
for such reports is identical to today.

---

## Open questions for FA validation

These are design assumptions that are best-guess until an FA conversation
validates them. Captured here as the starting list for the audit pass
when that conversation happens (per the
`project_pre_fa_validation_state` operating-context memory).

1. **Is `participant_role` worth capturing as an enum?** Today's
   inference is "is the registrant in the paper author list?" → presenter
   (else attendee). FA may also care about session chair, panelist,
   organizer, etc. Current enum (`attendee`/`presenter`/`organizer`/
   `other`) is a guess; FA may have a portal-driven taxonomy.
2. **Should `attendee_status` ("ACM Student Member") be structured?**
   Currently in `remarks`. If Stanford's expense policy treats discounted
   registrations differently (e.g., "you used a student discount, are
   you still actively a student?"), this should be a field. FA knows.
3. **Whose name goes in `business_purpose.who`?** Today: the registrant
   (= attendee). For a multi-attendee report (PI + grad student travel
   together), is the answer the report's payee, the registrant, or all
   attendees? Synthesis currently emits the registrant; FA may want
   different.
4. **Is `key_30char` formatted correctly?** Today: `event_name` +
   earliest scheduled date year (e.g., `ASPLOS-2026`). Stanford's portal
   might expect a different format. FA knows the portal's exact
   constraints.
5. **What counts as a "supporting" document?** Phase 5 covers conference
   programs / schedules / papers. Other supporting docs (visa letters,
   conference invitation letters, hotel confirmations that don't have
   folio data) are out of scope today but would extend the
   `supporting_*_doc` pattern. FA can tell us which categories matter.
6. **When does the synthesis confidence = `low`?** Synthesis self-rates,
   but the threshold is undefined today. May need FA input on what the
   `needs_review` queue should look like for synthesis-derived values.

---

## Open architectural decisions for the user

Before Stage 2 starts, I'd appreciate confirmation on:

1. **Schema annotations vs separate doc**: this design walkthrough lives
   here; should the per-field tier/reasoning live in `schema.yaml` as
   comments going forward, or stay in this doc? Either works. I'd lean
   keep this doc as the authoritative reasoning record (browsable,
   doesn't bloat the schema), and schema.yaml just carries the `source:`
   tag.
2. **Detail-block scope for `conference_registration_details`**: the
   trimmed 4-leaf set proposed above (`conference_name`, `ticket_type`,
   `attendee_name`, `registration_system`) keeps us single-call. Sound
   right? If you want any of the deferred fields promoted (attendee_status,
   early_bird), we go multi-call.
3. **`registration_system` enum scope**: today's table has 6 known
   systems plus `other`. Worth adding any I'm missing? (Stripe-direct,
   Square-based, university-portal-driven?)
4. **Synthesis confidence enum vs `needs_review` boolean**: the design
   above uses both. If the boolean is enough, I can drop the enum to
   keep the synthesis output even narrower.
