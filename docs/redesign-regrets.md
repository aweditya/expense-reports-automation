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
