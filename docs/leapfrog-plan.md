# Leapfrog plan — token-id-grounded extraction

Planning doc for the next architectural step in OCR grounding. **No
implementation yet** — this exists so the design decisions are written
down before code lands, the same way `docs/redesign-plan.md` carried
the Phase B redesign.

---

## 1. Why

Phase 6 Stage B shipped a working spot-check halo using post-hoc
text-matching between Gemini's `quote` and Document AI's tokens.
Robustness iterations (B-r1a normalization + tamarine HEIC fix) drove
the residual miss rate from 25.3% down to **0.7% (2 of 304 fields)**
on the 20-receipt cached corpus.

That looks like room to celebrate but it isn't. The remaining 2 misses
are **paraphrase cases that no matching tweak can fix** — Gemini's
quote string and the printed receipt text disagree. The whole
text-matching layer has a structural ceiling: it bridges two
independent systems (Gemini, DocAI) that don't know about each other,
and inevitably they will sometimes disagree about how to spell or
tokenize the same printed text. Every additional fallback tier we add
to the matcher fights this divergence with diminishing returns; the
neighborhood-search attempt (B-r1b) added 50 lines and rescued zero
cases because the audit data was clustered on a single broken file
(tamarine), not on a matching-algorithm problem.

The leapfrog is the architectural answer to the divergence class:
**eliminate the text-matching layer entirely** by giving Gemini
Document AI's tokens up front and asking it to cite which token IDs
it grounded against. Bbox resolution becomes a dict lookup instead of
fuzzy substring search. Hallucinated text becomes impossible (Gemini
either cites a valid token ID or it doesn't).

The motivation is not "lift the audit numbers from 66% to 95%." It is
"stop fighting an unfixable bug class."

---

## 2. What

### Current architecture

```
Gemini (image)  ──►  values + verbatim quote
                            │
                            ▼
  DocAI (image)  ──►  tokens (id, text, bbox)
                            │
                            ▼
              text-match quote ↔ tokens          ← brittle bridge
                            │
                            ▼
                         bboxes
                            │
                            ▼
              persisted JSON: _meta.evidence[].bboxes
```

### Leapfrog architecture

```
  DocAI (image)  ──►  tokens (id, text, bbox)
                            │
                            ▼ (formatted as numbered text list)
Gemini (image + numbered token list)  ──►  values + token_ids
                            │
                            ▼
              token_id → bbox dict lookup       ← deterministic
                            │
                            ▼
                         bboxes
                            │
                            ▼
              persisted JSON: _meta.evidence[].bboxes   ← unchanged
```

Two arrows reverse direction (DocAI now runs first, feeds Gemini),
and one fragile box (text-matching) becomes a deterministic one
(dict lookup).

---

## 3. Containment

**All changes are in `scripts/`. Nothing else touches.**

| Layer | Change | Why |
|---|---|---|
| Persisted JSON shape (`_meta.evidence[].bboxes`) | None | Same field, same units, same downstream consumers. |
| Rust types (`src/draft.rs::EvidenceReference`) | None | Reads the same JSON. |
| Reducer (`src/reduce.rs`) | None | Passes evidence through. |
| Validator (`src/validator_typed.rs`) | None | Doesn't read bboxes. |
| Workbench (`src/workbench_simple.rs`, `workbench_spotcheck.*`) | None | Reads `bboxes`. |
| Generated Rust + YAML | None | Source schema unchanged. |
| Cloud Build / Dockerfile | None | No new system deps. |

This is the cleanest reason to do this work. The blast radius is one
language, one directory, one team's worth of code to review.

---

## 4. Concrete changes by file

| File | Change | Est. LOC |
|---|---|---|
| `scripts/evidence_bbox.py` | Add `ocr_document(path)` exposer, `format_tokens_for_prompt(doc)` numbered-list formatter, `resolve_bboxes_by_token_ids(record, doc)` resolver, and a Levenshtein verifier. Keep existing `populate_bboxes` unchanged as the fallback path. | +100 |
| `scripts/extractor_lib.py` | Restructure `run_extraction`: DocAI first; format tokens as text context; pass into `single_call`; after Gemini returns, resolve token_ids → bboxes; fall back to `populate_bboxes` for entries missing token_ids. Update `single_call` to accept an extra-context string. | +50 (incl. ~20 refactored) |
| `scripts/extract_meal.py` | PROMPT addition: "Also return `token_ids` from the provided numbered token list, one set per evidence entry." | +10 (text) |
| `scripts/extract_transport.py` | Same PROMPT addition. | +10 (text) |
| `scripts/extract_airfare.py` | Multi-call wiring: run DocAI once, share its output across the three parallel `single_call` invocations. PROMPT additions for the main/aux/extras prompts. | +30 |
| `scripts/extract_lodging.py` | Same multi-call wiring across two parallel calls. PROMPT additions. | +20 |
| `scripts/generate_response_schema.py` | Add optional `token_ids: array of integer` to the evidence sub-schema. Auto-flows into all generated response_schemas. | +10 |
| `schema.yaml` | Documentation-only addition to the `_meta_convention.evidence` description — `token_ids` is a Gemini-call artifact, not persisted. | +5 |
| Tests (Python `tests/`) | New unit tests for the resolver + verifier, with a small fixture DocAI document. | +60 |

**Production total: ~295 lines across 8 files. All Python.**

Spike artifacts (separate, may stay as `scripts/spike_*.py`): ~200 lines.

---

## 5. The hallucination problem + verifier

Research (web search 2026-05-18) found no published implementation of
exactly this design — VLM returning OCR-engine token IDs for bbox
lookup. The closest validated work (DocVLM, arxiv 2412.08746) feeds
OCR signal into a VLM via a *learned encoder*, not raw IDs in the
prompt. Empirical reports on LLM citation hallucination (arxiv
2512.12117, arxiv 2603.08274) suggest **expect 5-15% invalid IDs**.

If we don't catch invalid IDs, we will draw halos in the wrong place,
silently. **Silently-wrong halos are worse than missing halos** — they
mislead the FA.

### Verifier design

For each evidence entry where Gemini returned `token_ids`:

1. Concatenate the text of the claimed tokens (in id order).
2. Compute Levenshtein distance between that concatenation and the
   original `quote` Gemini also returned.
3. If distance / quote-length > threshold (start at 0.30), the
   token_ids are likely hallucinated; **discard them and fall back to
   `populate_bboxes` text-matching** for that entry.

Both the threshold and the per-entry fallback are knobs the spike
needs to validate. Cheap to run; runs in-process; no extra API call.

---

## 6. Spike phase

Implement BEFORE any production stage. Goal: prove the design works on
real receipts and quantify the hallucination rate.

**Scope:**
- New file `scripts/spike_leapfrog.py` (flagged here; gitignored
  output to `.scratch/audit/leapfrog_spike.txt`).
- Pick 5 representative receipts: 1 PDF airfare, 1 PDF hotel folio,
  1 PDF Uber, 1 image meal receipt, 1 PDF receipt with multi-line
  addresses (any of the lodging confirmations).
- Hand-craft a Gemini prompt that:
  1. Asks for the meal/airfare values (kind-agnostic for the spike)
  2. Includes the numbered token list from DocAI
  3. Asks for `token_ids` alongside the verbatim quote (BOTH, so we
     can run the verifier and compare against today's matcher).
- Run the spike against the 5 receipts, capture per-entry:
  - Whether `token_ids` were returned at all
  - Whether they validated (verifier passed)
  - Whether the resulting bboxes matched today's matcher output (sanity)
  - Token-list size in prompt characters (cost/latency proxy)

**Exit criteria — go ahead with production stages:**
- ≥85% of entries return `token_ids`
- Verifier passes for ≥90% of returned `token_ids`
- Extraction quality on values is unchanged (no regression vs current
  prompt) — spot-check the 5 receipts manually
- Average token-list size < 30k chars (fits comfortably)

**Exit criteria — abort:**
- <60% of entries return `token_ids` (Gemini struggles with the format)
- Verifier rejects >25% (hallucination rate too high)
- Extraction quality regresses (values get worse with longer prompt)

Spike is ~1 day of work including review of outputs. If it fails, we
file findings in `docs/redesign-regrets.md` and the matter is closed.

---

## 7. Migration plan (only if spike succeeds)

Each stage independently committable + rollback-able.

| Stage | Description | Files | Risk |
|---|---|---|---|
| **L.1** | Response-schema additions: add `token_ids` as optional field in `generate_response_schema.py`; regen. Doesn't affect runtime behavior — just allows Gemini to fill the field if asked. | `generate_response_schema.py`, `generated/response_schema_*.json` | Low |
| **L.2** | Helper functions in `evidence_bbox.py`: `ocr_document`, `format_tokens_for_prompt`, `resolve_bboxes_by_token_ids`, verifier. Not wired to extractors yet. Unit-test these in isolation. | `evidence_bbox.py`, `tests/` | Low (no behavior change) |
| **L.3** | Wire ONE extractor (suggest `extract_meal.py`) end-to-end. PROMPT update + flow change. Side-by-side compare halo coverage on cached meal receipts vs current. | `extractor_lib.py`, `extract_meal.py` | Medium — first real behavior change. Easy to revert. |
| **L.4** | If L.3 looks good after spot-check, wire `extract_transport.py`. Same shape. | `extract_transport.py` | Low (mirrors L.3) |
| **L.5** | Wire multi-call extractors (`extract_airfare.py`, `extract_lodging.py`). Their parallel `single_call`s share one DocAI result. | `extract_airfare.py`, `extract_lodging.py` | Medium — multi-call parallelism is the trickiest piece. |
| **L.6** | Push. Cloud Run verdict on real FA upload. | — | Standard. |
| **L.7** | If no halo regressions after N weeks of FA use, retire the text-matching code path in `evidence_bbox.py` (the existing `populate_bboxes` body). Until then it stays as fallback. | `evidence_bbox.py` | Low; deletes legacy code only after stability proven. |

Each L.x is a commit. Each Cloud Run deploy can roll back to the
previous commit by reverting just that L.x.

---

## 8. Cost / latency

**Prompt size**: typical 2-page receipt has 600-1000 DocAI tokens.
Numbered format (`[t142] FRANCISCO `) adds ~12 chars/token → ~10k-15k
chars per receipt. Gemini 2.5+ context is 1M tokens (≈4M chars) — this
is rounding error.

**Token cost (Vertex billing)**: each character is roughly 1 token in
the prompt counter. 10k extra prompt tokens at current Gemini 2.5 Pro
pricing ≈ $0.00125 per receipt. User confirmed unlimited Gemini
credits for this project; cost is not a constraint.

**Latency**: DocAI now serializes BEFORE Gemini instead of running
post-hoc. DocAI takes ~1-3s per page; Gemini takes ~5-15s for a
typical receipt. Total latency adds 1-3s per receipt — acceptable at
FA volume (each upload contains ~5-20 receipts).

---

## 9. Failure modes + rollback

| If this breaks… | Recovery |
|---|---|
| Gemini stops returning `token_ids` reliably after a model update | Verifier rejects them; existing `populate_bboxes` fallback runs unchanged. Coverage drops back to current 66%. No data loss. |
| Token list inflates the prompt past Gemini's working context | Add a per-page token-count cap; for over-cap pages, fall back to text-matching for that page only. |
| Extraction values regress (the longer prompt confuses Gemini on the actual fields) | Spike catches this. If it sneaks past the spike: revert L.3 (one commit); other extractors unaffected. |
| Verifier threshold is wrong (too strict → fallback fires too often; too loose → wrong halos pass) | Single config constant. Tune from the spike output; revisit after FA use. |
| Cloud Build deploy fails on a new dep | Already-burned lesson: `deploy/requirements.txt` co-edited with `pip install`. No new deps expected in leapfrog (`evidence_bbox.py` already imports `google-cloud-documentai`). |

Persisted schema is **never** touched, so there is no data migration
to undo. Every rollback is a `git revert` of one L.x commit.

---

## 10. Non-goals

These problems remain after leapfrog ships:

- **HEIC/iPhone images uploaded with wrong MIME** — task #46 in the
  upload handler, orthogonal to grounding.
- **Receipts that aren't in DocAI-supported languages** — out of scope.
- **Gemini hallucinating values themselves** (vs hallucinating token
  IDs) — pre-existing, unchanged by leapfrog.
- **Receipts where the right field isn't visually present** (e.g.
  Gemini infers expense_type from context not from a printed phrase)
  — these correctly have `system_generated` evidence today and stay
  that way.
- **Handwritten receipts** beyond DocAI's OCR capability — out of scope.

---

## 11. Open questions

- **Token-ID format on the wire.** Integer (`142`), string label
  (`"t142"`), or JSON object (`{"id": 142}`)? Integer is most compact
  but loses self-description; string is robust to Gemini reformatting.
  Spike should try both.
- **Keep `quote` AND `token_ids`, or just `token_ids`?** Keeping both
  preserves backward-compat for any consumer that reads `quote`
  (today: workbench renders it as italic provenance text); also lets
  the verifier do its Levenshtein check trivially. Probably keep both.
- **Page-id format.** Multi-page docs need token IDs that disambiguate
  by page. Either prefix (`p1.t142`) or scope to (page, token_id)
  tuples in the schema. Spike should pick one.
- **When verifier rejects, do we render no halo, or run `populate_bboxes`
  fallback?** Probably fallback — graceful degradation matches Stage B
  ("no halo" is the silent-fail mode and is acceptable). Lock in
  during L.2.

---

## 12. Success criteria

The leapfrog is successful if, after L.5 deploys and N weeks of FA
use:

1. Halo coverage on per-doc evidence ≥ 90% (currently 66%).
2. Verifier rejection rate < 10% across the corpus.
3. Zero FA-reported "the halo is on the wrong text" incidents.
4. Extraction quality (value correctness) unchanged from pre-leapfrog
   on the 20-receipt cached corpus.

If 1+4 hold but 2 is bad, the architecture is right but the verifier
needs tightening. If 1 doesn't hold despite 2 being fine, Gemini is
omitting `token_ids` too often — re-examine the prompt.

---

## 13. Decision pre-conditions

Before starting L.0 (the spike):

- [ ] This doc is reviewed and the open questions in §11 have explicit
      go-with-this-default decisions (or the spike is empowered to pick).
- [ ] Confirm Gemini model version is locked (so spike results
      generalize to production).
- [ ] Cloud Run service stable on current architecture (matching
      robustness work shipped, no in-flight regressions).

All three are true as of 2026-05-19.
