# Pipeline Redesign — Regrets & Lessons

Running log of mistakes I've made or almost made, what I learned, and the rule
I'm holding myself to going forward. Add an entry every time a mistake is
caught — by me or by the user.

## Format

Each entry:

```
### YYYY-MM-DD — short title

**What happened:** one or two sentences on the mistake.
**Why it was wrong:** the harm or the rule it violated.
**Rule going forward:** the specific behavior change.
```

Keep entries short. The point is recall, not narrative.

## Entries

### 2026-05-18 — trusted the file extension; tamarine was HEIC bytes with a .png name

**What happened:** The Stage B robustness audit revealed that 11 of 13 residual matching-misses were all on `tamarine.png` — Document AI returned a 400 "Invalid image content" on that one image. Spent several rounds proposing increasingly clever matching-algorithm tiers (neighborhood search, etc.) trying to lift the audit numbers. None of it helped because **DocAI returned zero tokens, so there was nothing to match against in the first place**. Eventually ran `file` against the image: it's HEIC/HEIF (iPhone format) with a `.png` extension. Gemini happened to content-sniff and accepted it; DocAI was strict about declared MIME vs actual bytes. Scanned the rest of the corpus — only this one file had the mismatch.

**Why it was wrong:** Two layered mistakes. **First**, I never validated the actual bytes of receipt images before trusting their extensions — the whole pipeline (DocAI MIME map, audit, matching algorithm) silently assumed `*.png` means PNG bytes. **Second**, when the audit data clearly clustered the residual misses around one specific receipt, I kept proposing matcher tweaks instead of asking "is this one file actually broken?" The data was screaming for a per-receipt look-at-the-actual-file and I treated it as an algorithm problem instead.

**Rule going forward:** When audit data clusters tightly around a single receipt or single failure mode, **investigate the receipt before fixing the algorithm**. Cheap diagnostic first (`file`, `pillow.Image.open()`, compare to known-good neighbors) — algorithm changes second. For the pipeline itself: trust content-sniffing, not extensions. iOS sets the extension; the bytes can be anything.

### 2026-05-18 — added a runtime dep to local .venv but forgot deploy/requirements.txt

**What happened:** During Phase 6 Stage B.0 I installed `google-cloud-documentai` into `.venv` with a single `pip install` so I could call Document AI from the helper scripts. The helper and the extractors imported it cleanly locally, cargo tests passed, the local workbench eyeballed correctly, and I pushed to main with high confidence. Cloud Build's deploy succeeded — its Python test stage doesn't actually import the extractors (it only checks `cloudbuild.yaml` shape). On the first real upload to the deployed app, the extractor crashed with `ModuleNotFoundError: No module named 'google.cloud'` because `deploy/requirements.txt` never gained the line.

**Why it was wrong:** This is **the exact pattern `docs/deploy-cheatsheet.md` already warns about** ("The local `.venv/` accumulates packages that the Cloud Build container doesn't have. A green local Python suite isn't proof Cloud Build will pass; the deploy gate is the proof."). I read that warning while writing the cheatsheet myself and still walked straight into it. The deploy gate proves Docker BUILDS, not that the container RUNS — runtime imports are only validated when something exercises them, and our test suite doesn't.

**Rule going forward:** Any time `pip install X` is run for the project (not for a one-off spike), the immediate next action is `grep X deploy/requirements.txt || echo "ADD ME"`. Treat the requirements file as a co-edit with `.venv`, not a separate concern. Bonus hardening for later: add an import-smoke test to the Python suite that imports every `scripts/extract_*.py` and `scripts/extractor_lib.py` — would have caught this at the Cloud Build stage instead of at runtime.

### 2026-05-18 — local corpus filenames were lying about the content they referenced

**What happened:** During the corpus rename task (driven by the Phase 6 Stage B spot-check halo work needing files to actually exist where the cached JSONs pointed), a background agent inspecting each receipt for date/vendor/amount discovered that several files had names that didn't match their content:
- `mels1.jpeg` was actually MJ Sushi, not Mel's
- `Hotel-McNamara_2023_01_14.pdf` was an Uber ride, not a hotel folio
- `Hotel-Northville_2023_01_11.pdf` was also an Uber ride
- 7 more "Hotel-Warren / ride_report_* / Uber-*" files in the same wrong-category situation
- `invoice_..._d2b7ef23.pdf` was a Kiwi.com Spirit Airlines ticket, not a generic invoice

The wrong filenames had propagated into `scripts/acceptance_check.py` expectations — the test harness was asserting "lodging" facts against files that were transport. The local corpus had been lying about itself for some time, undetected.

**Why it was wrong:** This is the same shape of lesson as the 2026-05-14 "rules/prompts are pre-FA-validation guesses" entry, but extended one level out: **filenames are pre-FA-validation guesses too**. When a file gets dropped into the corpus with a hasty name, the name accretes into test harnesses, comments, and expectations — and nothing in the pipeline catches the mismatch because the pipeline only knows what the filename says, not what the content is.

**Rule going forward:** When auditing or onboarding new receipts, treat the filename as a hypothesis to verify, not a fact. The cheap verification is to open the file (or run the extractor) and confirm category + date + vendor match the name before wiring the file into test expectations.

### 2026-05-18 — "use git mv" rule didn't account for gitignored directories

**What happened:** Instructed the background agent to use `git mv` for every rename. `receipts/` is in `.gitignore` (line 12) because the actual receipt files are private FA data not committed to the repo. `git mv` failed with "fatal: not under version control" because there was nothing for git to track. Agent correctly fell back to plain `mv` after running `git ls-files receipts/` to confirm zero tracked paths, but it was a constraint the rule didn't anticipate.

**Why it was wrong:** Not a real mistake, more a rule-precision gap. The intent of "git mv" is to preserve history when renaming tracked files. When the files are gitignored, there is no history to preserve — `mv` is correct and `git mv` is structurally impossible. Worth being explicit so future agents don't waste cycles trying to make `git mv` work in this directory.

**Rule going forward:** "Use `git mv` for renames" implicitly means "for tracked files." For gitignored paths, plain `mv` is the right tool; verify nothing is tracked with `git ls-files <path>` before reaching for `mv`.

### 2026-05-16 — used a Python heredoc for the Document AI smoke test instead of a real script

**What happened:** Verified Document AI auth + API shape by piping a multi-line `python3 <<'PY' ... PY` heredoc into Bash. The smoke worked (got tokens + bboxes back from an Air India PDF) but the test isn't reproducible without retyping the heredoc, and it inherently violates rule #4 ("if it's a script, it lives in `scripts/`").

**Why it was wrong:** The heredoc was a script in everything but its filesystem location. The "is this a script?" test isn't line count — it's "does it have non-trivial logic that someone might want to re-run or audit later?" Multi-step API verification absolutely qualifies. Inline scripts are exactly the artifacts that disappear and force re-typing.

**Rule going forward:** Anything more complex than `python -c "import x; print(x.version)"` lives in `scripts/`, flagged before creation. For verification-only scripts that won't ship, prefix the name with `spike_` so the convention is obvious (e.g. `scripts/spike_pymupdf_search.py`, which followed the rule). The DocAI smoke should have been `scripts/spike_docai_smoke.py`.

**Update 2026-05-18:** This rule was violated again during Stage B.2 — used multi-line `python -c "import json ... walk(rec) ... print(...)"` Bash heredocs for ad-hoc JSON inspection (verifying the retrofit only added `bboxes` and didn't mutate other fields). Same anti-pattern as the DocAI smoke: non-trivial logic embedded in a Bash command instead of a tracked script. User caught it. **Hardened rule:** when I want to inspect JSON structure ad-hoc, the choice is `jq` (one-liner, idiomatic) or a real `scripts/spike_*.py` file. There is no third option called "just paste some Python into the shell." If the inspection is so trivial it doesn't seem worth a file, it's probably already expressible as `jq`.

### 2026-05-14 — production eyeball caught a validator edge case the local corpus didn't exercise; broader: rules/prompts are pre-FA-validation guesses

**What happened:** Phase 4 Stage 6 deployed cleanly (build SUCCESS in 155s, revision live, image SHA matched). I declared Stage 7 ready for the user's verdict. User uploaded the smallest realistic FA workflow — 2 receipts: Air India BOM→SFO (foreign by currency) + Southwest SFO→PHX (USD-domestic). The Category card came back with a green high-confidence dot but a red border. Cause: `check_category_country_consistency` in `validator_typed.rs` warned "report category is foreign but every line's country_of_activity is United States or null"; JS added `has-issue` class; CSS painted red. My local cargo tests + 19-receipt acceptance suite + workbench eyeball did not surface this. The 19-receipt corpus had multiple lines with various country values and the 2-line shape (with US-destination foreign-currency airfare) wasn't exercised.

The validator rule was structurally too narrow — it treated "non-US country" as the sole signal of foreignness, missing that "non-USD original_currency" is an independent and equally-valid signal. For a BOM→SFO ticket the destination is US (per the airfare prompt: "country of furthest destination = trip's purpose-end") but the currency is INR. Two foreign signals that disagree by design; the rule only honored one. Hotfix in `d0fbdfe` extended the rule to a disjunction; corpus still flagged correctly when no foreign signal at all is present.

The broader observation, raised by the user during the post-mortem, is more important than the specific bug: **most of the pipeline's prompts and validator rules encode educated guesses about FA workflow, none of which has been validated by an actual FA conversation.** Examples just from Phase 4:

- "country_of_activity = trip purpose-end" (airfare prompt — my call)
- "if foreign category, expect non-US country" (validator rule — pre-existing)
- "stanford_travel_egencia value implies Egencia is the booking origin" (booking_method enum — schema author's call)
- "ticket_amount in printed currency, line_amount_usd derived" (split between extractor and reduction — my call)
- "remarks should be one short sentence summarizing the trip" (every extractor prompt — convention)

Each is plausible. Most haven't met an FA. The Air India case is one example where two such guesses collided into a visible bug; there are almost certainly others not yet exercised by the corpus.

**Why both layers were wrong:**

1. *Specific* — I treated "local 19-receipt suite passes + workbench renders + push succeeds" as deploy-ready. The minimum realistic FA upload (1–3 receipts) is a different shape than 19, and rules with hidden assumptions can fire differently. I should have explicitly tested the minimal-corpus case.

2. *Broader* — building features against a self-curated corpus and self-written rules creates a closed loop where each piece confirms the others without external grounding. The system "works" because everything in it agrees with everything else — but the FA isn't in the loop, so we can't know whether the agreement is right.

**Rule going forward:**

- *Specific:* For any new validator rule or new extractor prompt, run the local verification on at least one minimal-corpus shape (1–3 receipts of the new kind) IN ADDITION to the full suite. Edge cases hide in the small inputs.

- *Broader:* The current prompts and validator rules are explicitly **pre-FA-validation**. Treat them as best-guess defaults until an actual FA review happens. When the FA conversation lands, schedule a focused audit pass: walk every conditional rule, every prompt's reasoning rules, and every enum's value names against what the FA actually does. Until then, every "this rule fires unexpectedly" finding (like this one) is data — capture it instead of just hotfixing, so the audit has a concrete starting list.

### 2026-05-13 — FX is a hardcoded mock, not real-time

**What happened:** Phase 4 added foreign-currency support for airfare (the corpus's first non-USD receipts; Air India ticket in INR). Reduction needs to fill `common.line_amount_usd` for foreign tickets where the extractor leaves it null. Built `mock_usd_rate(currency)` in `src/reduce.rs` as a 4-entry constant table (INR, EUR, GBP, JPY → ~2024-2025 averages). `apply_mock_fx` calls it with a single argument: the currency code. Date is ignored.

The mock unblocks the demo end-to-end — Air India's ₹80,896 ticket converts to $970.75 and the workbench renders correctly with provenance. But the rate has no relationship to the actual FX rate on the transaction date (Sep 2 2024). The FA reviewing a converted amount has no way to verify it against a real source.

The correct implementation is **date-aware FX lookup**, signature `lookup_usd_rate(currency: &str, date: &IsoDate) -> Option<f64>`. Three viable backends: a free FX API like `frankfurter.app` (supports `GET /2024-09-02?from=INR&to=USD`), a local snapshot of ECB/IMF historical rates (~10 KB JSON for major currencies × major dates), or a Gemini grounded-search call (more expensive but consistent with the existing extraction pattern).

**Why it's worth flagging instead of just shipping:** The mock has the right *shape* — extractor defers, reduction fills, workbench surfaces the origin (`reduce.fx.mock` → "converted via mock FX rate (placeholder; real-time tool TBD)"). When the real tool lands, the change is one function body, no architectural lift. But the surface is misleading: the FA sees "$970.75 medium confidence needs review" with the placeholder caveat, but they have no way to know HOW wrong the rate could be. For a $970 ticket the drift is ~$5; for a $10K booking it could be hundreds.

**Rule going forward:** When reduction depends on external data (FX, conference dates, GSA per-diem, …), the origin code must explicitly mark the source as a placeholder if it is one. The workbench's `human_readable_origin` string is the FA-facing surface; today it says "placeholder; real-time tool TBD" which is honest. That string is the contract — don't drop it once a real tool exists, *replace* it with the new source's description ("converted via frankfurter.app on 2024-09-02"). Date-aware FX is captured as a Phase 5 follow-up; it's small enough to be one focused commit when the placeholder is no longer good enough.

### 2026-05-13 — Schema conditional rules need a periodic audit; stale references and wrong-scope expressions accumulated silently

**What happened:** Phase 4 Stage 4's workbench eyeball surfaced a "MISSING FIELDS (85)" count in the issues rail — most of which were false positives. Investigation revealed two distinct schema-rule problems:

1. **Wrong-scope conditional expressions.** Five common-block fields (`original_currency`, `original_amount`, `exchange_rate`, `country_of_activity`, `foreign_activity_type`) had `required:` expressions scoped to the *report-level* (`general_information.category == expenses_foreign`) when the rule's intent was *per-line* ("this line's expense_type is foreign"). For a mixed-currency report (1 INR + 18 USD receipts), the report tipped foreign overall, so all 18 USD lines got falsely flagged for missing original_amount/etc. — even though those USD lines correctly had null values with `origin: not_applicable_for_domestic`.

2. **Stale rule references.** Two ConditionalRule expressions reference enum values (`international_lodging`, `international_meals`) that don't exist in the master `expense_type` enum — leftovers from an older schema. They never fire (no value can match), so they're dead weight. Discovered while grepping conditional rules; would never have surfaced in normal use.

I initially framed (1) as "the validator needs new capability" — looking at the code revealed the validator's reference resolution is already scope-aware (12 other rules use per-line expressions correctly). The bug was purely in the schema author choosing `general_information.category` when `expense_type` was the right reference. Fix was a 5-line schema.yaml edit (Stage 4.5A, commit `16835e4`); issues count dropped 85 → 13 with no validator code change.

**Why both happened:** schema.yaml conditional rules don't get exercised every codegen run. They sit silently until a corpus or a manual eyeball surfaces the disagreement. Wrong-scope rules were probably correct when the corpus was uniform (all-domestic or all-foreign); the airfare phase introduced the first mixed-currency report. Stale references presumably survived a schema rename without grep-for-references.

**Rule going forward:** When adding/removing/renaming an enum value in `schema.yaml`, grep `validation_rules.rs` AND `schema.yaml` itself for references to the changed value. When writing a new conditional rule, default to the most-local scope reference (`expense_type` from the line, `meal_details.has_alcohol_on_receipt` from the line, etc.); promote to a report-level reference (`general_information.category`) only when the rule explicitly applies report-wide. Stage 4.5A added a regression test (`original_currency_not_required_on_domestic_line_in_foreign_report` in `validator_typed.rs`) so this exact scope confusion can't reappear silently. Audit pass for the two stale `international_*` references is captured as a follow-up cleanup commit.

### 2026-05-13 — Phase 4 Stage 2 contract-pair was incomplete (missed render_airfare_details)

**What happened:** Phase 4 Stage 2 added `scripts/extract_airfare.py` + dispatcher wiring in `local_app_simple.py` + acceptance harness fixtures + the contract-pair commit message claimed it was atomic. It wasn't — I forgot `render_airfare_details` in `src/workbench_simple.rs`. The data was being extracted correctly into per-doc JSON and reduced into the report; the renderer just had no per-kind subsection for airfare lines. Workbench showed only the 7 common-block cards for airfare receipts and zero of the 9 airfare-specific cards (Airline, Departure, Destination, Class, Round Trip, Traveler, Ticket Number, Ticket Amount, Booking Method).

Caught only when the user opened the workbench during Stage 4 verification. Fixed in a separate commit (`606f2d6`) with a 28-line `render_airfare_details` function mirroring `render_lodging_details`.

**Why it happened:** my mental model of "the contract pair" was the data path (Python emits → Rust deserializes → Rust reduces → JSON shape unchanged). I missed that the *display* path is also part of the contract for a new kind — the FA needs to see those fields, not just have them sit in JSON. The SPEC.md §7 "add a new kind" recipe lists workbench rendering as step (5), but I read past it under "renderer + validator walk for the new detail block" without internalizing that it meant *adding a new render function*.

**Rule going forward:** SPEC.md §7's "add a new kind" recipe needs to be more explicit: step (5) should be split into "(5a) add `render_<kind>_details(html, <kind>, path)` in `workbench_simple.rs`; (5b) add the corresponding branch in `render_transaction_line`; (5c) add the validator walk for the new detail block." When implementing a new kind, mentally walk the FA's experience: "I upload a {kind} receipt; do I see all the {kind}-specific fields on the workbench?" If no, the contract pair isn't complete. Update committed in this docs sweep.

### 2026-05-13 — Vertex's Schema validator has a property-count ceiling that breaks above ~5 detail-block leaves

**What happened:** Lodging needed six T2/T3 fields in its detail block (hotel_name, location, check_in_date, check_out_date, booking_method, is_shared_lodging). The generated response_schema combined those with the eight `common` leaves and the `extras` block; every leaf inlines the full `_meta` subtree (~20 properties for confidence/evidence/needs_review/flags). Vertex rejected the schema with a generic 400 INVALID_ARGUMENT. A bisection (run via a one-off mutation harness, now deleted, that drops fields one at a time from the lodging schema and probes Vertex with each variant) showed: drop any one of the six detail fields ⇒ passes; strip `_meta` from all six ⇒ passes; descriptions and the booking_method enum were NOT the trigger. Best estimate: Vertex has a transitive property-count ceiling around 320–340 inlined properties, which six `_meta`-wrapped detail leaves push over.

We tried `$ref` to collapse `_meta` once at the schema root: dead on both code paths. The typed `response_schema` (`types.Schema` Pydantic) rejects `$ref` outright with `extra_forbidden`. The raw-JSON-Schema `response_json_schema` parameter accepts the dict client-side, but Vertex's underlying validator returns the same 400. The codegen comment "no `oneOf` / `anyOf` / `$ref` (limited SDK support)" is now backed by direct evidence.

The fix is a two-call architecture for lodging: schema is split across two parallel Gemini calls per folio (`common + lodging_details` in one, `extras` in the other), each well under the ceiling, merged in Python into the same per-doc JSON shape a single call would have produced. Reduction (Rust) sees no difference.

**Why this matters as a lesson:** The ceiling applies to *every kind*, not just lodging. Any future kind whose detail block has more than ~5 `_meta`-wrapped fields will hit the same wall. Meal (4) and transport (3) fit comfortably; airfare and conference-registration will need to be sized carefully — if either grows past 5 fields we either drop the multi-call pattern in (cheap, the orchestration shape is already in `extract_lodging.py`) or land a field. There is no third option from the SDK side until Google ships proper `$ref` support.

**Rule going forward:** When adding a new kind, count its detail-block T2/T3 leaves before designing the schema. ≤ 5: single-call is fine (meal/transport pattern). ≥ 6: multi-call from the start. `scripts/probe_response_schemas.py` runs the generated schemas through Vertex with a 1×1 PNG in well under 10 seconds — wire it into the local dev loop so the ceiling is caught at codegen time, never at first upload. Document the per-kind leaf count alongside the schema definitions so the cost of adding a field is visible.

### 2026-05-12 — shipped lodging extractor without ever asking Vertex if it accepted the schema

**What happened:** Phase 3 added `scripts/extract_lodging.py` + `generated/response_schema_lodging.json`. Updated `acceptance_check.py` with 5 lodging fixtures but didn't `--run` it before pushing. First production upload returned a generic Vertex 400 — Vertex was rejecting the schema itself, before Gemini ever looked at the folio. A 30-second local probe (one `generate_content` call against the new schema with a 1×1 PNG) would have surfaced it immediately. Meal and transport had this rail by accident: the iterative Phase 2 prompt-tuning always ran `--run` on real receipts, so any schema rejection would have shown up in that loop. For lodging I developed "by inspection" (check that cargo tests pass, check that the schema looks structurally similar to meal) and skipped the live call.

**Why it was wrong:** Rule 6 says "CLI testing is sanity-only; the deployed Cloud Run site is the verdict." I read it as "skip CLI, go to Cloud Run." It actually means CLI is the sanity check FIRST, Cloud Run is the FINAL verdict — sequential, not alternatives. Skipping CLI turned Cloud Run into a debugger, and the debugger output is "the FA can't upload."

**Rule going forward:** Adding a new extractor kind is a contract pair (new prompt + new generated schema). Both halves verified locally against Vertex before pushing — minimally via a fast schema probe (`scripts/probe_response_schemas.py`), ideally via `acceptance_check.py --run` against one real receipt of that kind. Cargo tests passing is not evidence that Vertex accepts the schema.

### 2026-05-12 — shipped a hotfix on an unverified hypothesis

**What happened:** After the lodging 400 surfaced, I hypothesized "Vertex rejects leaf-wrapped arrays" (the `{value: array, _meta: {...}}` shape on `nightly_rates`), unwrapped it to a bare `Vec<NightlyRate>`, deployed in `e17d437`. Cargo tests passed. The same 400 returned on the next upload. Bisection an hour later showed the offending region is somewhere inside `lodging_details`, not in `extras`. I had not run the bisection BEFORE pushing the hotfix.

**Why it was wrong:** Hypothesis-driven debugging is fine. Hypothesis-driven *shipping* is not. The hotfix went out on plausibility, not evidence. The probe that would have falsified the hypothesis is the same probe I should have run before the original deploy.

**Rule going forward:** A hypothesis fix for a production bug must include a test that would FAIL without the fix and PASS with it, run before the push. For schema-rejection bugs: probe with the pre-fix schema (must fail) and the post-fix schema (must pass). If I can't construct that test, the fix is a guess and doesn't ship.

### 2026-05-11 — schema rule blew the model output budget on the largest receipt

**What happened:** UI Polish Stage 6 added `confidence_reason` as a
required field on every `_meta` block. Every leaf — ~13 per transport
line, more for meals — now needed a justification sentence. I shipped
the change without measuring how the new rule interacted with the
existing `max_output_tokens=32768` cap. First production upload of
uber1.pdf (a 2-page Uber receipt) exhausted the combined thinking +
output budget; Gemini's response truncated mid-JSON; `json.loads()`
correctly refused; the FA saw a "FAILED to parse JSON" friendly error
page.

The pipeline's error handling worked as designed (truncation caught
cleanly, diagnostics written to disk, friendly page rendered). The
**design** was wrong: the schema rule had a per-leaf cost I never
multiplied out against the corpus's largest input.

Hotfix bumped `max_output_tokens` to 65536, dropped `confidence_reason`
from the schema's `required` list (so partial outputs parse), and
relaxed the prompt to "required only for medium/low" — which gave the
FA the workbench-visible reasons but lost the audit signal on
high-confidence values.

A subsequent design pass (same day) walked back the "high may omit"
rule. The user pointed out that confidence_reason isn't only an FA
UX feature — it's a debugging tool: if you can't audit why the model
called something high, you've lost half the value of the field. New
rule: required for every leaf, ≤5 words for high, ≤15 for medium/low.
Display surfaces high-confidence reasons in unobtrusive gray; medium/
low keep the prominent amber.

**Why both decisions were wrong:**
- The original Stage 6 was budget-blind. I treated "add a per-leaf
  justification" as a free schema rule when it wasn't.
- The hotfix overcorrected. I optimized for "no truncation" by
  amputating a feature instead of dialing it down.

**Rule going forward:** Schema or prompt changes that affect output
volume must be measured against the largest input in the corpus
*before* shipping. For new such rules, run `acceptance_check.py
--run` on the largest-page-count receipts (uber1, uber2 today) to
catch the budget interaction. The Gemini API costs a couple of
cents per call; the alternative is shipping a feature that breaks
the moment a real upload tests it. Cloud Run is the verdict, but
local pre-flight on the corpus's worst case is still cheaper than
a deploy + revert + hotfix cycle.

### 2026-05-05 — Phase 1 Chunk 1 split a contract pair across commits

**What happened:** Phase 1's first chunk shipped only the schema half
of the enum collapse — `expense_type` lost its `_with_alcohol`
variants in the Rust types, but the Python extractor was still
emitting them. Cargo tests passed locally because the inline JSON
fixtures were updated in the same commit, but the live system would
have failed on every new upload: Python writes `business_meal_with_
alcohol`, the new Rust lib refuses to deserialize that variant. I
caught this only because I happened to think about it while preparing
the next chunk, and we reverted via `git revert` and re-staged as
"Pair A" with the schema change AND the prompt change in one commit.

**Why it was wrong:** I conflated "small commits" (Rule 3) with
"small per-file changes." The right unit is a *self-coherent
contract*, not a *narrow diff*. Schema changes that affect what the
extractor emits are atomic with the extractor prompt, and shipping
one without the other moves the system through a broken intermediate
state that the test suite happens not to catch (because tests use
fixed fixtures, not live extraction). If the user had been less
attentive, the next upload after the first chunk's deploy would have
failed mid-pipeline with a cryptic Rust serde error.

**Rule going forward:** Before splitting a change across commits,
ask: "if the first commit deploys but the second doesn't, can the
live pipeline still complete an upload?" If no, the change is a
contract pair and must ship atomic. The cargo type system enforces
this for Rust↔Rust contracts, but Python↔Rust contracts (every
schema change) are by-eye. Specifically: schema change + extractor
prompt + acceptance-check predicates = always one commit.
Re-staging Phase 1 into Pair A/B/C + Standalones D/E gave us five
clean atomic units instead of seven half-deployable ones.

### 2026-05-05 — shipped Mermaid diagrams without rendering them, twice in a row

**What happened:** Wrote `docs/SPEC.md` with five Mermaid diagrams,
sanity-checked the syntax with grep ("no raw `<>`, no hyphenated IDs"),
declared it good, committed, pushed. The user opened it in GitHub and
the sequence diagram failed with a parse error — I'd used `&lt;id&gt;`
HTML entities inside a `participant ... as` label, and Mermaid's
sequenceDiagram parser doesn't decode entities. Switched the
placeholders to `{id}`, pushed the fix. The user opened it again and
the *component* diagram now failed — `{` in a flowchart edge label is
diamond-node-opener syntax, different parser context. Two pushes, two
broken diagrams, both caught by the user.

**Why it was wrong:** Rule 7 ("eyeball the artifact before claiming
code is done") explicitly applies to anything that produces a
user-visible artifact, and Mermaid diagrams are exactly that. My
"sanity check" was running grep over the source for known gotchas —
that's the equivalent of running a linter and skipping the actual
build. For diagrams, "render in the actual viewer" is the build step.
And: each Mermaid block has its *own* parser context (sequenceDiagram
≠ flowchart), so syntax that's safe in one fails in another. A single
gotcha-list isn't enough.

**Rule going forward:** For SPEC.md or any other Mermaid-bearing doc:
either (a) render every diagram via `mmdc` or GitHub's preview before
claiming complete, or (b) flag explicitly in the commit "diagrams
unrendered, please verify" so it's clear the artifact wasn't checked.
Default is (a). Codified into CLAUDE.md as Rule 11 (architecture
changes must update + render SPEC.md diagrams).

### 2026-05-04 — declared "no Cloud Build trigger configured" by checking the wrong region

**What happened:** First time the deploy story came up, I ran `gcloud
builds triggers list --project=soe-agile-agents` and saw "Listed 0
items." From that I concluded "no GitHub trigger exists; deploy is
manual." I built `scripts/deploy.sh`, wrote a regret about inline
gcloud commands, updated CLAUDE.md to say "deploy = scripts/deploy.sh
until we wire a trigger," and ran four manual deploys today. Then the
user pointed at the GCP console and there was a `deploy-on-push`
trigger for the repo, in `us-west1`, that had been firing on every
push the whole time. Every commit I pushed today auto-deployed AND I
re-deployed it manually — double-build, double-tarball-upload.

**Why it was wrong:** Cloud Build triggers can be created either
globally or in a specific region. `gcloud builds triggers list`
defaults to the global region and silently omits regional triggers.
`gcloud builds list` has the same default — which is why none of the
trigger-fired builds in us-west1 showed up when I checked recent build
history either, reinforcing the wrong conclusion. Two commands lying
the same way looked like agreement; really they were both blinkered.

**Rule going forward:** When checking for the absence of cloud
infrastructure, pass `--region=...` (or `--regions=-` to list all)
explicitly before declaring "none configured." This applies to
triggers, builds, Cloud Run services, secrets — anything that has a
regional namespace. CLAUDE.md and `docs/deploy-cheatsheet.md` updated:
deploy gesture is `git push origin main` (the trigger does the rest);
`scripts/deploy.sh` stays as a manual escape hatch but is no longer
the primary path.

### 2026-05-04 — claimed Cloud Run auto-sets `$GOOGLE_CLOUD_PROJECT` (it doesn't)

**What happened:** In M7.e.1 I changed `spike_extract.py` to read the
project from `$VERTEX_PROJECT_ID or $GOOGLE_CLOUD_PROJECT` and added a
comment saying "Cloud Run sets the latter automatically via the metadata
server." Both the cloudbuild.yaml and the Cloud Run service config got
no env var for the project. Result: the deployed app threw "project
required" the first time the FA tried to upload a receipt — extracted
from the user-facing error page, not even from logs.

**Why it was wrong:** That's true for App Engine and Cloud Functions.
Cloud Run sets `$K_SERVICE`, `$K_REVISION`, `$K_CONFIGURATION`, `$PORT`
— not `$GOOGLE_CLOUD_PROJECT`. I wrote the assertion in code and in a
code comment without verifying. Locally I always set `VERTEX_PROJECT_ID`
explicitly so the bug couldn't surface; same for the `docker run`
smoke test where I passed `-e VERTEX_PROJECT_ID=...`. Production was
the first env where neither was true.

**Rule going forward:** Never write a code comment asserting cloud-
runtime behavior I haven't directly tested in *that* runtime. If the
script runs only when an env var is set, prove the env var is set in
every place the script will run — locally (shell), in `docker run`
(`-e`), in Cloud Run (`--set-env-vars` on the deploy step). The
cloudbuild.yaml deploy step now passes
`--set-env-vars=VERTEX_PROJECT_ID=$PROJECT_ID` so future deploys carry
the value; the lying comment in spike_extract.py is removed.

### 2026-05-04 — ran `gcloud builds submit` as an inline command instead of a script

**What happened:** When push-to-main turned out not to trigger Cloud
Build (no GitHub trigger configured in the project), I ran
`gcloud builds submit --config=deploy/cloudbuild.yaml --project=...
--substitutions=COMMIT_SHA=3356407` straight from a Bash tool call. Did
the same again after fixing a bad `_PROJECT_ID` substitution. Two
separate inline runs of a real production deploy command.

**Why it was wrong:** Rule 4 says "no inline scripts." Deploys are
exactly the kind of action that needs to be reproducible — anyone
(including future me, including the user) should be able to deploy by
running a single named file, not by copy-pasting flags from a
conversation. Inline commands also rot: the next time I need to deploy
I'd reconstruct the flags from memory and probably get one wrong (which
I literally just did with `_PROJECT_ID`).

**Rule going forward:** Captured deploy as `scripts/deploy.sh` —
resolves COMMIT_SHA from `git rev-parse --short HEAD` automatically.
For any operation that talks to shared infrastructure (cloud builds,
deploys, GCS writes, etc.), the first such call gets a script before
the second. No "I'll just run it once" exceptions.

### 2026-05-04 — passed `_PROJECT_ID` to a build that uses the built-in `$PROJECT_ID`

**What happened:** First Cloud Build submit failed with `key
"_PROJECT_ID" in the substitution data is not matched in the
template`. I'd passed `--substitutions=COMMIT_SHA=...,_PROJECT_ID=...`,
but `deploy/cloudbuild.yaml` references `$PROJECT_ID` — Cloud Build's
auto-populated built-in for "the project the build runs in" — not a
user substitution that needs `_PROJECT_ID=`.

**Why it was wrong:** I added the substitution defensively without
reading the yaml. `$PROJECT_ID`, `$BUILD_ID`, `$COMMIT_SHA` (when
trigger-set), `$REVISION_ID`, etc. are all built-ins. User
substitutions need a leading underscore; built-ins don't. Confusing
the two costs you a full source upload (~230 MB tarball) and an error
before any compute starts.

**Rule going forward:** Before passing `--substitutions`, read the
cloudbuild.yaml and pass only the keys it actually templates. The
deploy script now passes only `COMMIT_SHA`, which is what the yaml
references that isn't auto-set when submitting manually.

### 2026-05-04 — local Python suite missed Pillow-import failures the cloud build caught

**What happened:** M7.e.2 dropped `Pillow` and `pillow-heif` from
`deploy/requirements.txt` (the new pipeline doesn't need them).
Locally, `python -m unittest discover` passed because my `.venv/`
already had Pillow installed from earlier work — pip never uninstalls
on its own. The cloud build, which `pip install`s into a fresh
container from `requirements.txt` only, hit 16 import errors in the
two old-pipeline test files.

**Why it was wrong:** "Local CLI testing is sanity-only; the deployed
service is the verdict" (Rule 6) caught it, which is the rule working
as intended. But the gap exists: the local venv has accumulated old
deps that the production image doesn't, so a green local suite isn't
proof the cloud build will be green.

**Rule going forward:** When the change *is* the dependency surface
(adding/removing a requirement), do the verification in a clean
environment — either rebuild the venv or run the suite in the Docker
image — before relying on a local pass. The deploy is also when to
catch these; today's was caught by the deploy gate, exactly where
Rule 6 says it should be.

### 2026-05-04 — created a log file in `.scratch/` without flagging it

**What happened:** During step 5 I started the Flask server in the
background with output redirected to `.scratch/local_app.log` without
first naming the file in a "I'm about to create X" message. The file is
tiny and gitignored, but Rule 10 ("flag every new file before creating
it, even small one-off scripts") doesn't carve out an exception for log
files.

**Why it was wrong:** The rule's spirit is "no new files appear without
the user knowing they're coming." Log files are exactly the kind of
incidental artifact that accumulates if I don't think about them.

**Rule going forward:** Server logs go in the upload-dir or a per-run
subdirectory under `.scratch/`, named in the flag-then-create message
that precedes server startup. No more bare `.scratch/local_app.log`.

### 2026-05-04 — wrote a regret about bundled commits without un-bundling the commit

**What happened:** Commit `72f557e` was supposed to be a clean "remove
dead amount_confidence" but I `git add`-ed without checking the working
tree, and three new helpers (`confidence_floor`, `derived_meta`,
`category_confidence`) rode along. I noticed, wrote a regret entry
(`d09751f`) confessing to it, and moved on. The bundled commit stayed
bundled.

**Why it was wrong:** A regret entry is not a fix. Future-me reading
`git log` will see "Remove dead amount_confidence variable" and trust
that's what landed. The history lies; the regret is in a separate file
that may or may not get read. The honest move would have been to
`git reset HEAD~1`, split the commit, and re-commit with accurate
messages — at the cost of 5 extra minutes.

**Rule going forward:** When I notice I've bundled commits, the first
move is to un-bundle (reset + split), not to confess in regrets. Regret
entries are for things that can't be fixed retroactively (deployed
bugs, lost work). Bad history is fixable while it's still local.

### 2026-05-04 — `meta: Default::default()` placeholder eventually surfaced as a visible red dot

**What happened:** In M6.2.e I wrote three derived-field assignments in
`reduce.rs` like `meta: Default::default()` because "we'll fix the meta
later." `Default::default()` for `FieldMetadata` is `confidence: Low,
evidence: [], needs_review: false, flags: []`. The reduction shipped that
way for several commits without anyone noticing — there were no callers
yet that *displayed* the meta. M7.d.2 wired the workbench to show
confidence dots, and the user saw `category: expenses_domestic ●` (red
dot) on a confidently-derived value and reasonably asked "why is this
low confidence." The deferred placeholder had become a visible bug.

**Why it was wrong:** `Default::default()` for a meaningful semantic value
is a lie. The default confidence isn't "I haven't decided" — it's "Low,"
which the workbench then rendered as "this value is questionable." The
TODO I had in my head ("we'll fix the meta later") never made it into the
code as anything grep-able, so the gap stayed invisible until the
downstream UI surfaced it.

**Rule going forward:** When a derived value's `meta` isn't immediately
honest, write `meta: todo_meta("reason")` (a small helper that returns a
default-but-flagged FieldMetadata with a known origin string), so future
greps for `todo_meta` or for the origin string surface the gap. Or just
fix the meta inline at the time of writing — usually faster than the
placeholder.

### 2026-05-04 — bundled two logical changes into one cleanup commit

**What happened:** Step 1 of the M7.d.2 cleanup was supposed to be
"delete dead amount_confidence." I ran `git add src/reduce.rs` without
checking that the working tree also had three other added helpers
(`confidence_floor`, `category_confidence`, `derived_meta`) from the
earlier confidence-fix work that hadn't been committed yet. The cleanup
commit (`72f557e`) ended up containing both the deletion AND those new
helpers — two logical changes in one commit.

**Why it was wrong:** Frequent commits with one logical change each is
the rule precisely so that history reads cleanly and reverts target the
right thing. A "cleanup" commit that secretly contains a feature
addition undermines both.

**Rule going forward:** Before `git add -A` or `git add <file>`, run
`git diff --cached <file>` and `git diff <file>` and confirm what's
actually about to land matches what the commit message will say. The
slow-down-on-visual-iteration rule extends to commit boundaries too.

### 2026-05-03 — UI mistakes from rushing through visual iteration

**What happened:** During M7.d.1 I made a string of small UI mistakes that
the user caught: misread "move highlight to the issue clicked" as the
*issue card* when the user meant the *field card*; left the `:target` CSS
rule in place after switching to JS-driven highlighting, which caused
fields to highlight on bare page loads with no user click; wrote test
assertions that matched substrings in the inlined CSS instead of the
actual rendered elements. Each was caught and fixed within minutes, but
they came in a cluster.

**Why it was wrong:** The pattern wasn't any single mistake — it was
*pace*. I was making changes, rendering, sending the URL to the user,
and moving to the next change before fully thinking through implications.
The :target slip especially was sloppy: removing one trigger of a
behavior (the JS that added the class) without removing all triggers
(the CSS pseudo-class that auto-applied) is exactly the kind of cleanup
miss that more deliberate work would catch.

**Rule going forward:** When iterating on UI, slow down on the "between"
moments. Before sending a refresh-and-look message: re-read the diff,
ask "did I leave any stale state that could trigger the old behavior?",
and trace through what the user is about to see. Visual code earns one
extra beat of deliberation per cycle. Speed up only when the change is
mechanical.

### 2026-05-03 — committed visual code without eyeballing the output first

**What happened:** Wrote `src/workbench_simple.rs` + CSS for M7.b, ran the
unit tests, and immediately wrote a long commit message claiming the
renderer was done. Only when the user asked for a preview did I render
the actual HTML — and it had three real visual bugs (line cards getting
cropped, an orphan `▸` in the wrong position, tooltip not appearing on
hover). The user had to catch them.

**Why it was wrong:** Unit tests verify the code runs without crashing
and produces the expected substrings. They do NOT verify the rendered
output looks right. For any code whose deliverable is a visual or output
artifact (HTML, JSON, a generated file, an image), "tests pass" is not
the same as "it works." The artifact has to be looked at.

**Rule going forward:** Code that produces a user-visible artifact gets
the artifact rendered and inspected by me (or shown to the user) BEFORE
the commit lands. The check is: "did I open the output and look at it?"
If no — don't commit yet. Particularly applies to: HTML renderers, JSON
serializers with new shapes, anything generating a script the user will
run.

### 2026-05-03 — added a binary without flagging it

**What happened:** Created `src/bin/render_workbench_preview.rs` as a
preview tool for M7.b without first telling the user "I'm adding a one-
off binary." Surfaced it in the response after the fact.

**Why it was wrong:** "No code bloat" + "explain why before changing"
both apply to new files, even small throwaway ones. Sneaking in an
unflagged file under "I'm just making something work" is exactly how
the codebase accretes one-offs.

**Rule going forward:** Any new file gets named in the plan or the
preceding message before it's created. Even one-off scripts. Especially
one-off scripts — those are the ones that live forever uncalled.

### 2026-05-03 — used `/tmp/..` in a heredoc cleanup

**What happened:** Tried `cat > /tmp/.. /dev/null 2>&1 || true` as a no-op.
The literal `/tmp/..` resolves to `/`, so it failed harmlessly with "is a
directory", but I still typed `/tmp` into a command with the rule explicitly
forbidding it.

**Why it was wrong:** The rule is "never write to `/tmp`," not "never write
to a real `/tmp` file." Even bogus paths under `/tmp` violate the spirit
because they normalize the habit of typing the forbidden path.

**Rule going forward:** `/tmp` doesn't appear in any command I run, ever,
even as part of a longer path or a placeholder. Use `./.scratch/` for any
path that even gestures at temp space.

### 2026-05-02 — trusted a piped command's exit code

**What happened:** Ran `cargo test 2>&1 | tee … | tail -10; echo "EXIT: $?"`.
The `$?` captured `tee`'s exit code (0), not cargo's. The harness reported
"exit 0" so I almost concluded tests passed when actually 4 tests failed.

**Why it was wrong:** A pipeline's exit status is the last command's by
default. Hiding the real exit behind `tee`/`tail` makes failures invisible.

**Rule going forward:** Either run the command without piping, or set
`set -o pipefail` (and check `${PIPESTATUS[0]}` for the real exit), or
inspect the output file directly for FAILED markers before declaring success.

### 2026-05-02 — wrote a log file to `/tmp`

**What happened:** Piped `cargo test` output through `tee /tmp/cargo_test_after_regen.log`
to capture a copy of the log alongside the background-task output file.

**Why it was wrong:** Workflow rule is explicit — never use `/tmp`, use the
project directory. Even harmless intermediate files belong inside the repo so
they're discoverable and so the project owns its scratch space.

**Rule going forward:** If a command needs a captured log, write it under
`./.scratch/` (gitignored) or read from the harness-provided task output file.
Never `/tmp`, never another machine-wide location. Same applies to any
mktemp, /var/folders, system temp dir.
