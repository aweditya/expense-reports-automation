# Durable-store plan

**Status**: design only, not implemented. Drafted 2026-05-22 during Stage 19
cleanup. Companion to `docs/SPEC.md` (current architecture) and
`docs/redesign-plan.md` (the spec for the in-memory-state version
we have today).

**Audience**: us, future-us, anyone evaluating whether to invest in
this. The FAs haven't asked for it; this is forward-looking
infrastructure work.

---

## 1. Why consider this

Three classes of pain we live with today, all symptoms of the same
underlying choice ("in-memory + container-ephemeral disk"):

1. **Container recycle wipes everything mid-flight.** Cloud Run with
   `--max-instances=1` recycles instances after ~15 min idle, on
   every deploy, and on any health check failure. When that happens:
   - The `JOBS` dict (live progress) vanishes → in-flight uploads
     return `phase=lost` (Stage 11 added this graceful path, but the
     FA still has to re-upload).
   - The `.scratch/uploads/<id>/` directory vanishes →
     **any workbench the FA was editing is gone**. A 24-hour-old URL
     just 404s. This is the real loss; the FA's review work is
     unrecoverable.

2. **Single-device, single-browser.** Form drafts live in the FA's
   browser localStorage (Stage 11b). Switching from laptop to phone,
   or Chrome to Safari, loses the draft. No way for two FAs to
   collaborate on the same report.

3. **No history / no queries.** Every upload is a sealed island.
   "Show me last quarter's foreign trips" isn't answerable. "How
   many ASPLOS receipts did you file?" isn't answerable. Today's
   answer is "the FA reviews each report individually and trusts
   their memory."

These are real but not currently blocking the FAs. They're
forward-looking — adding a durable store would unlock workflows we
can't support today.

---

## 2. State inventory: what's ephemeral today

| State | Where | Survives request? | Survives container restart? | Survives deploy? |
|---|---|---|---|---|
| `JOBS` dict (live upload progress) | Python module-level in Flask process | ✓ | ✗ | ✗ |
| Uploaded raw files | `.scratch/uploads/<id>/files/` (container disk) | ✓ | ✗ | ✗ |
| Extracted JSONs | `.scratch/uploads/<id>/extractions/` | ✓ | ✗ | ✗ |
| Reduced `report.json` | `.scratch/uploads/<id>/reduced/report.json` | ✓ | ✗ | ✗ |
| Workbench HTML | `.scratch/uploads/<id>/workbench.html` | ✓ | ✗ | ✗ |
| Edited values (Stage 7) | Written back to `report.json` on disk | ✓ | ✗ | ✗ |
| FA form drafts | Browser `localStorage` (Stage 11b) | ✓ | ✓ (client-side) | ✓ (client-side) |
| FX rate cache | In-memory dict per `fx_enrich` subprocess | ✗ (subprocess-scoped) | ✗ | ✗ |

The pattern is clear: everything *except* the client-side draft
disappears whenever Cloud Run decides to recycle the container.

---

## 3. What to persist (priority order)

In order of FA-visible impact:

1. **Edited `report.json` + the workbench**. Highest value. The FA
   spends real time reviewing + correcting; losing that to a
   container recycle is catastrophic. Once persisted, a 24-hour-old
   workbench URL just works.

2. **In-flight upload state (`JOBS`)**. Medium value. Lets the FA
   resume a mid-extraction upload from a different device, or
   reload the progress page after a network blip. Without this, the
   FA has to re-upload from scratch — annoying but recoverable
   (browser still has the localStorage draft for the form).

3. **Server-side form drafts**. Low-medium value. Cross-device draft
   sync. Already works per-browser via localStorage; server-side
   would let FA fill on laptop and finish on phone. Nice but not
   urgent.

4. **Historical reports + queries**. Low value today, potentially
   high later. Enables "show me last quarter's foreign trips" or
   "all my ASPLOS receipts." Mostly an enabler for future analytics.

5. **FX rate cache**. Lowest value. Cross-upload cache saves a few
   hundred ms per duplicate (currency, date) lookup. Real savings
   only if many FAs file trips with overlapping dates.

The first two are the actual reason to do this work. The rest are
free side-effects once the infrastructure is in place.

---

## 4. Storage options

Stanford's Cloud project (`soe-agile-agents`) is already on GCP, so
all the relevant options are first-party. Comparison for *our*
load shape: ~10 uploads/day, ~20 transaction lines each, ~100 KB
per workbench HTML.

### Option A: Firestore (Native mode)

Document store. Serverless. Auto-scales. ADC-friendly.

- **Schema fit**: a "document per upload" maps naturally to our
  per-upload `report.json` shape. Firestore documents max out at
  1 MB; our typical `report.json` is ~50 KB, so we have headroom
  for a 20×-larger report (or we can split out lines into
  sub-collection if it ever happens).
- **Latency**: ~10ms read, ~50ms write within same region.
- **Cost**: $0.06 per 100K reads, $0.18 per 100K writes, $0.18/GB
  storage. At our load: ~$0.10/month.
- **Pros**: cleanest fit for our data; native GCP; same project; no
  network setup; ADC works out of the box; multi-region replication
  default; transactional updates per-document.
- **Cons**: query model is restrictive (no JOINs; composite indexes
  must be declared); 1 MB document size cap; pricing scales with
  read count (SSE polling at 0.5s × N minutes × M FAs adds up).

### Option B: Cloud SQL (Postgres)

Relational DB. Always-on. VPC connector required from Cloud Run.

- **Schema fit**: would model uploads as a table with a `report`
  JSONB column. Could query the JSON with PG operators.
- **Latency**: ~5ms read in same region (once connection is warm).
- **Cost**: ~$7/month minimum for the smallest tier + per-query.
- **Pros**: familiar; transactional; rich queries (JSONB ops);
  schema migrations via standard tools.
- **Cons**: always-on cost even when no traffic; need VPC connector
  for Cloud Run-to-CloudSQL networking; more ops overhead (backups,
  patching, connection pooling); overkill for our load.

### Option C: GCS JSON blobs

Object storage. One JSON file per key.

- **Schema fit**: `gs://expense-reports-state/uploads/<id>/report.json`
  mirrors our current `.scratch/uploads/<id>/report.json` shape
  almost 1:1.
- **Latency**: ~50ms per read (cold), <10ms (warm with CDN).
- **Cost**: ~$0.02/GB storage + ~$0.05 per 10K ops. At our load:
  ~$0.01/month storage + ~$0.10/month ops.
- **Pros**: cheapest; existing GCS in project; trivially backed up;
  works with `gsutil` for ad-hoc debugging.
- **Cons**: no transactions (concurrent edits to the same
  `report.json` race; last-writer-wins); no queries (have to list
  + load); no built-in concurrency control.

### Option D: Memorystore (Redis)

In-memory key-value.

- **Cost**: $35/month minimum.
- **Pros**: <1ms latency.
- **Cons**: pricy for our load; ephemeral by default (need RDB/AOF
  for persistence); needs VPC connector.
- **Verdict**: overkill. Skip.

### Option E: Hybrid (Firestore metadata + GCS blobs)

The big-rich-content pattern: small structured records in Firestore
(upload metadata, JOBS state, edit timestamps), large artifacts in
GCS (the rendered `workbench.html`, the per-receipt extraction
JSONs, the source PDFs).

- **Cost**: ~$0.20/month combined.
- **Pros**: avoids Firestore's 1 MB document limit; lets us keep
  large artifacts cheap; structured metadata still queryable.
- **Cons**: two-system complexity (atomic updates across Firestore
  + GCS need manual orchestration); more code.

---

## 5. Recommendation

**Start with Option A (Firestore native)** for the structured state
+ small artifacts. Add Option C (GCS for source PDFs) only if
storage cost becomes meaningful. Defer Option E (full hybrid) until
we have a use case it genuinely solves.

Reasoning:
- Firestore alone handles 95% of what we need at <$1/month for our
  load shape. `report.json` is ~50 KB, well under the 1 MB doc cap.
- GCS for source PDFs is a small marginal cost win and a
  small marginal complexity cost.
- Splitting `workbench.html` into GCS adds complexity for no real
  benefit — the workbench is re-rendered from `report.json` by the
  Rust binary, so we can rebuild it on demand instead of storing it.
- Don't introduce VPC complexity (Cloud SQL) for a small project.

---

## 6. Migration plan (phased)

Each phase ships separately. Each phase is reversible — if Firestore
proves wrong-fit, we can roll back without losing data (always
dual-write during transitions).

### Phase 1: JOBS → Firestore

**Scope**: replace the in-memory `JOBS` dict with a Firestore
collection `jobs/{upload_id}`.

**Schema**:
```
jobs/{upload_id} = {
  phase: "extract" | "reduce" | "fx" | "render" | "done" | "error" | "not_found" | "lost",
  files: [{name: str, kind: str, status: str, error?: str}],
  current: int,
  error?: str,
  created_at: timestamp,
  updated_at: timestamp,
  ttl: timestamp (auto-delete after 7 days)
}
```

**Implementation**:
- Replace `_set_job` / `_get_job` with Firestore client calls
  (gated behind a `USE_FIRESTORE_JOBS` env var so we can ship + roll
  back cheaply).
- SSE handler polls Firestore every 0.5s instead of the in-memory
  dict — pricing-aware: at $0.06/100K reads, a 5-min upload with
  600 SSE polls × N FAs adds up. May need to throttle SSE poll
  frequency or use Firestore real-time listeners (server-side
  subscriptions instead of polling).
- Cost watch: 10 uploads/day × 600 SSE reads × 30 days = 180K
  reads/month = ~$0.11/month. Acceptable.

**Win**: `phase=lost` becomes rare. Container recycle no longer
loses live progress. FA can switch tabs / devices and reconnect.

**Effort**: ~3-4 hours. Mostly client setup + ADC integration.

### Phase 2: report.json → Firestore + GCS for source PDFs

**Scope**: persist the reduced report + edits in Firestore; persist
uploaded source files in GCS.

**Schema**:
```
reports/{upload_id} = {
  report: {<the full report.json>},
  fa_input: {<the form fieldset>},
  rendered_workbench_url: "/uploads/<id>/workbench.html",
  created_at: timestamp,
  updated_at: timestamp,
  ttl: timestamp (90 days?)
}
```

Source PDFs go to `gs://soe-agile-agents-expense-reports/uploads/<id>/files/{name}`.

**Implementation**:
- `_run_pipeline_in_background`: after extract → reduce → fx_enrich,
  write the report to Firestore; after render, write the workbench
  HTML to GCS (or skip, since re-rendering from report.json is
  cheap).
- `POST /uploads/<id>/edit`: write the new value to Firestore first
  (transactional), THEN to the local disk for the in-flight render.
  On next page load, render from Firestore.
- Workbench page: when the local file doesn't exist (cold start),
  fall back to reading report.json from Firestore + re-rendering.

**Win**: workbench survives container recycle. 24-hour-old URLs
just work. Cross-device review possible.

**Effort**: ~6-8 hours. The transactional edit path is the tricky
bit; need to think through concurrent-edit semantics.

### Phase 3: form drafts → Firestore (optional)

**Scope**: server-side form drafts keyed by IAP-authenticated user
email.

**Schema**:
```
form_drafts/{user_email} = {
  fields: {<all fa_* form fields>},
  updated_at: timestamp
}
```

**Implementation**:
- New endpoint `POST /form-draft` that writes the user's draft
  server-side (in addition to localStorage)
- On `GET /`, look up the user's draft (via IAP-injected email
  header) + pre-populate the form

**Win**: cross-device form drafts.

**Effort**: ~3 hours.

**Skip unless an FA asks.** Stage 11b's localStorage already covers
the single-device case which is 95% of usage.

### Phase 4: historical queries (optional)

Once reports are in Firestore, queries become possible. Things like
"all reports where category=expenses_foreign in the last 90 days"
are trivial composite-index queries. Defer until an FA actually
asks ("can you show me Olivia's Singapore trip from April?").

---

## 7. Cost estimate

At current load (~10 uploads/day, 20 lines avg, 5 receipts avg):

| Component | Volume | Unit cost | Monthly |
|---|---|---|---|
| Firestore document writes (JOBS updates + report saves) | ~1,000/month | $0.18/100K | $0.002 |
| Firestore document reads (SSE polling + workbench renders) | ~50,000/month | $0.06/100K | $0.030 |
| Firestore storage (~100 KB × 300 docs) | ~30 MB | $0.18/GB | $0.005 |
| GCS storage (source PDFs, ~200 KB × 1,500 files) | ~300 MB | $0.020/GB | $0.006 |
| GCS Class A operations (write PDFs) | ~1,500/month | $0.05/10K | $0.008 |
| GCS Class B operations (read PDFs for re-render) | ~3,000/month | $0.004/10K | $0.001 |
| **Total** | | | **~$0.05/month** |

At 10× load (~100 uploads/day): ~$0.50/month.

At 100× load: ~$5/month.

Well within the noise of the existing Cloud Run / Gemini costs.

---

## 8. Failure modes

| Scenario | Today | With durable store |
|---|---|---|
| Container restart mid-upload | `phase=lost`, FA re-uploads | `phase` reflects truth, FA can resume |
| Container restart mid-edit | Edit may be lost if disk write hadn't synced | Edit committed to Firestore first; disk is a cache |
| 24-hour-old workbench URL | 404 (container recycled, dir gone) | Loads from Firestore re-render |
| Firestore outage | N/A | SSE falls back to "service degraded" message; reads use local disk if present; new writes queued in-memory and retried |
| GCS outage | N/A | Source PDFs not retrievable; existing reports still work |
| Concurrent edits to same report | Last-writer-wins (no protection) | Firestore transaction or ETag check — return 409 to the second writer; client retries with merge |

---

## 9. Out of scope

Things this plan deliberately does NOT cover:

- **Authentication/authorization beyond IAP**. We rely on IAP to
  gate access; we don't model per-user permissions inside the app.
- **Real-time multi-FA collaboration on the same report**. Possible
  with Firestore listeners but adds significant complexity. Not
  asked for.
- **Audit logging beyond Cloud Logging**. Firestore docs would
  carry created_at + updated_at; we don't need an immutable audit
  trail.
- **Encryption at rest beyond GCP defaults**. Firestore + GCS
  encrypt by default; we'd add CMEK only if Stanford compliance
  requires it.
- **Backup beyond Firestore's built-in PITR**. Point-in-time
  recovery covers 7 days; ad-hoc exports to GCS for long-term
  archive can wait until we have a real retention policy.

---

## 10. Open questions

To resolve before Phase 1 ships:

1. **TTL policy**: how long do we keep upload state? Reports?
   Default proposal: JOBS expires 7 days after `phase=done`,
   reports expire 90 days after last edit. Both configurable.

2. **Multi-FA collision**: today only one FA per upload (upload_id
   is unique). With server-side drafts (Phase 3), do two FAs
   editing different uploads ever share state? No (different
   upload_ids = different Firestore docs). What about Phase 3
   form-drafts keyed by user email — would two browser tabs on
   the same device race on the draft? Yes; last-writer-wins is
   probably fine for a form draft, but worth thinking about.

3. **Local dev story**: do we run a Firestore emulator locally,
   or fall back to the in-memory dict via the same
   `USE_FIRESTORE_JOBS` env var? Probably the latter — emulator
   is heavy and the in-memory implementation must keep working
   anyway for tests + local-only flows.

4. **What about the existing `.scratch/uploads/<id>/` directories?**
   Pre-migration uploads exist on Cloud Run instance disk + don't
   exist in Firestore. On migration day, do we (a) ignore them
   (they'll 404 after container recycle), (b) write a one-shot
   migration script that lifts them into Firestore, or (c) leave
   them as a transitional ghost? Recommend (a) — the FAs will
   re-upload anything they care about, and old URLs would just
   404 a few days earlier than they would have anyway.

---

## 11. Decision criteria for "do we build this"

Build Phase 1 + 2 when ANY of these are true:

- FA reports losing edits to a container recycle (proves we
  actually have the pain).
- FA asks "can I review yesterday's upload?" → we say no, container
  recycled (proves the 24-hour URL gap is felt).
- We start running multiple Cloud Run instances (today
  `--max-instances=1`; if we ever scale up, in-memory JOBS becomes
  a correctness bug, not just a UX one — different requests would
  hit different instances + see inconsistent state).
- A second FA joins (today only one FA per upload; with two FAs
  uploading concurrently, even Stage 12's concurrency-tested in-
  memory state holds, but cross-device review starts to matter).

Until then: deferred. The code is unchanged; this doc is the
artifact. We can come back to it when one of the criteria fires.
