# Audits

An independent reader (Codex, read-only) audits the engine and core by subsystem; its findings are
claims, verified before anything is fixed. Owner, 2026-11-13/14: audits on every independent system,
after every merged batch and before the build; Codex does no code work. Decision C-041.

**When:** without the owner asking. Audits are cheap and no agent's work is right in one pass, so repeated
passes are the method (owner, 2026-11-15): one after each item of the step list in
`docs/MILESTONES.md`, and one on the merged whole before the cluster verification. Every landed step or merged batch gets an audit of the scopes it
touched (a scope file written for the change, if none fits), started in the background while work
goes on; the whole system is audited before the build and after any change that crosses subsystems.

Everything the loop needs is in this directory, so a new session can run it without the last one.

## The loop
1. **Audit.** `tools/audit/run SCOPE REPO OUT_DIR [NOTE_FILE]` for each scope in `scopes/` (three at
   a time is what the laptop takes). REPO is the candidate's worktree; the `core` scope reads this
   repository. Reports go to `~/claude-artifacts/audits/<date>-<name>/`. For a repeat audit, the note
   file says what the last one found and which commits claim to fix it, and asks for both a verdict
   on each old finding and an attack on the new code.
2. **Triage.** Each report goes to a fresh reader with `briefs/triage.md` (add the repository and the
   report's path at the end): every finding is REAL, NOT REAL, BY DECISION or UNCERTAIN, with
   file:line. Triage files go under `triage/` beside the reports.
3. **Cut line** (owner, 2026-11-15). A REAL finding **blocks the build** only if it would put a wrong
   number in the database or could only be corrected by a full rebuild. The rest are listed in
   `docs/MILESTONES.md` under Open and fixed later by a scoped republish. The owner sees the triage
   at each audit: what blocks, what waits, with before and after numbers where a value moves.
4. **Fix** the blocking ones in small batches, under `briefs/fix-common.md` (one worktree and branch
   per track, a decision-number block each, one commit per finding, each track stating what changes
   on an ordinary day). Delete a track's cargo target directory when its branch is merged.
5. **Real day.** Each merged batch runs a real day on a test droplet (`tools/testbed/tb`) before the
   next batch starts, compared with the last clean day; the differences must be the ones the tracks
   declared.
6. **Audit again** the scopes the batch touched. The engine is ready for the build when an audit of
   the candidate leaves no finding above the cut line, not when an audit finds nothing.

## Failure-surface audits (owner, 2026-11-16)
When a real run exposes a fault, the fault is one point on a surface: the same assumption is
usually made elsewhere. Besides fixing the fault and auditing the fix, run an audit ALONG the
surface: `scopes/failure-surface.md` is the template. State the fault as the assumption that
failed, in general form (not "the replay endpoint returned 500" but "the code assumes a provider
endpoint answers 404 for a date it has no data for"), give the real evidence, and ask for every
other place that makes the same assumption, each marked by what it would do on the build's range.
Worth it for faults found by real days and cluster runs; not for every audit finding.

## Order of authority for a finding
A published standard (OCC, Cboe, exchange calendars, the standard definition of an indicator) decides;
then product intent; then the owner.

## Files
- `run`: one audit. `scopes/`: one file per subsystem (what to read, what is central).
- `briefs/audit-common.md`: the auditor's instructions and the owner's priorities.
- `briefs/triage.md`: the verifier's instructions. `briefs/fix-common.md`: the fixers' rules.
- History: `~/claude-artifacts/audits/` (2026-11-13 failure handlers; 2026-11-14 universe pass 4 to 9;
  `2026-11-14-baseline/`, the ten subsystems and their triage).
