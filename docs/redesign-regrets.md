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
