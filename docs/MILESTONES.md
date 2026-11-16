# Milestones

The one living status document: current state and what is next. History is in git (the longer
forms are at tags `milestones-2026-11-15` and `milestones-2026-11-16`); decisions are in
`docs/DECISIONS.md` (core, C-numbers) and the engine's `docs/DECISIONS.md` (D-numbers); the plan
is in `docs/ARCHITECTURE.md`.

## Steps to the MVP (the list shown to the owner on a status request; keep it current)
1. **Two-day cluster trial.** Done 2026-11-13.
2. **Engine ready for the build.** Closing. A Codex audit ran after each lettered item:
   a. [done] the seven fix tracks merged (`pass-fx-all`); *audit* (round a)
   b. [done] the follow-ups merged; *audit* (round b)
   c. [closing] real days on the candidate: `x2` clean on `pass-fx10`; **`y2` and `x3` running on
      the merged candidate `pass-s3i` d55f6439**. Left for a droplet with real data: re-freeze
      the AAPL tape-cache and the `etha-first-chain` fixtures. An early date and a pre-listing
      day need a provider key: they are step 2.5's.
   d. [done] audit triage and fixes: batches 3 to 10, rounds c to i. Converged (owner, 2026-11-16).
   e. [done 2026-11-16 10:00 UTC] `pass-s3i` = `pass-fx10` + D-162 by merge (done); core `pass-core-fix` merged into
      `main` (done, pushed); *audit of the merge* (rounds j and k, fixed); images built on the
      laptop and pushed; engine fast-forwarded to `worker` (d55f6439).
   f. [done] housekeeping: this file pruned 2026-11-16 twice (tags above). Prune again, with a new
      tag, when it passes about 300 lines.
3. **(2.5) Cluster verification. IN PROGRESS since 2026-11-16 10:00 UTC (owner's pre-approval of
   04:00 UTC).** Runbook `~/claude-artifacts/cluster-verification-runbook.md`. Order (tape history
   only moves earlier): 2025-12-10; 2025-04-11 (ETHA's first chain); 2024-07-16 (AVGO 10-for-1,
   ex 07-15, and before ETHA); the block 2022-06-06..06-15 (FB to META). **The newest two weeks
   on two nodes are the build's first step** (owner, 2026-11-16), not part of 2.5.
4. **(3) The build, phase 1 (owner, 2026-11-16).** a. the newest two weeks as a test sample, and
   fix what they reveal; b. back to **2025-04-10** (about 401 sessions: every sensor at full
   window on the newest week; no old-layout snapshot, no 2023 gap), and fix what comes up.
   Before it: the unusualness flag line states its pool (done, D-258), the fix batches of the
   failure-surface audits, **the small restatable captures (company, macro, carry: about 4 MB a
   day gzipped) kept through core's write-once path (owner, 2026-11-16: do it before phase 1)**,
   the database volume grown (approved), one final image from one commit, everything audited.
   After each tranche, **a census of the built store (owner, 2026-11-16: the database must hold
   enough data points to judge the event rules):** per event rule written before the build
   (first chain, strike adjustment, open-interest outage, split and rename translation, receipt
   ratio, bank not-applicable, refused snapshot rows, capture failures, the provider comparison
   classes), how often it fired, on which assets and dates, and every withheld state by reason;
   the rules are judged on that population, not on the one case that prompted each.
   **Owner, 2026-11-16: expect that a rule will fail to generalise, and choose the remedy from what
   the build shows:** if one sensor is affected, a single-sensor rebuild (the owner's planned
   route); if many, a full rebuild of phase 1. The two-week sample is the main place to catch and
   fix issues before committing to the full build. Not a fixed plan: decided once the evidence is
   in. A full second pass roughly doubles phase 1's node time (past the $300 ceiling): raise it
   then if needed.
   Where the risk sits (owner, 2026-11-16): the base layer was checked against external references
   in the B0/B1 censuses; the derived and history layer could not be, because it needs continuous
   histories. So step 5 runs the independent reference checks over the BUILT histories (the
   `tools/reference` checkers recompute z-scores, percentiles, flag lines and technicals from the
   store). Caveat: base code changed after B1 (company valuation D-259..D-261, D-326..D-345;
   M0-GAMMA on cleared open interest; the open-interest outage and strike-adjustment rules), so
   those base sensors also get their reference checks on the sample.
   **Reference checks on every computed day (owner, 2026-11-16):** run `python3 -m refcheck run`
   (QuantLib 1.43, TA-Lib 0.8.2 on the laptop) on each verification and sample day's dump, not
   only at step 5. Company valuation gets a genuinely external reference: SEC EDGAR's XBRL API
   (the filers' own reported facts), pending the owner's choice of the contact SEC requires in
   the User-Agent.
5. **(3.5) Examine the built database (owner, 2026-11-16).** Done twice: **a first pass after the
   two-week sample** (thin: about ten sessions, so it can show a rule that misfires or never
   fires on ordinary days, not how one generalises; anything it changes is fixed before going
   back to 2025-04-10), and **the full pass after phase 1**, before Reporter: go through the rules and decisions against the built store and ask of each
   whether it **generalises cleanly**: from the census (how often each rule fired, on what, and
   what was published), which rules are doing real work, which over-fire, which never fired,
   which rest on one case and should be retired or restated, and where a missing threshold
   needs the population's evidence. Outcome: a short list of rule changes (each then a scoped
   recalculation), and the rules that stand.
6. **(4) Reporter on the phase 1 database.** First a proper paid report (none has been produced
   yet: it needed the database) beside the free one (all there, a lot of numbers); then **a
   significant body of iteration on the reports themselves** (owner, 2026-11-16) until free and
   paid are generated with confidence.
7. **(5) The launch decision:** pay for Intrinio's Startup tier and integrate Reporter's output
   into the website. After it, or when wanted: the deeper history (the old snapshot layouts are
   readable since D-251..D-254; the 2023-01-18..03-07 gap needs a ruling), the FIELD campaign.
**End goal (owner, 2026-11-16): the whole system lives on DigitalOcean and runs by itself:**
capture, the day, the ordered pass, Reporter and the website, **reports published without user
intervention**. Today Reporter runs on the laptop and feeds the website by hand. So step 5
builds Reporter to read core's API and to run headless (a scheduled job beside core), with its
own publish gate and a channel notice on a hold, not a person's check. **Reports publish on
Sundays** (owner): the week's last session is Friday's, so Friday evening after the close and
Saturday are the slack for a held day or report. Reporter is C#; it can run as a weekly job on
the core node (no extra droplet).
Working practice: at most three agents at once on the laptop; an audit after every landed step;
every verified finding is fixed, only one above the cut line sends the candidate round again
(C-041 amended). Write a time only after `date -u`.

## Now (16:55 UTC 2026-11-16): HAND-OFF STATE (the owner moves the session to Opus when the running agents finish)
The longer record of 2026-11-16 is at tag `milestones-2026-11-16b` (640 lines: every run, finding
and time). Read `tools/audit/README.md` (the audit loop, failure-surface audits) and AGENTS.md first.

**Where we are: step 3 (cluster verification), which turned into a second round of engine fixes.**
Nothing wrong has been stored; every fault refused, held or was caught by a comparison.

**Engine branches (all pushed unless said; worktrees `/home/diggz/wt/<name>`, cargo targets
`/home/diggz/wt/target-<name>`):**
- `pass-s3i` (worktree `s3i`): the candidate line. e05fac7f pushed = the merged candidate d55f6439
  + D-248 (worker makes its archive directory), D-249 (no replay request before the tape start;
  tape job lists the claim's members), D-250 to D-254 (reader for the three option-snapshot
  layouts; refused rows withhold one security), D-255/D-256 (open-interest outage; first chain),
  D-257 (a slot is staged once; the bundle refuses a duplicate), D-258 (the unusualness flag line
  states its pool, DLM-UNUSUALNESS@1.7.0), D-259 to D-261 (company valuation computed in the
  engine; enterprise value; the provider comparison emitted). **Then D-290 to D-294 (e10b6780, pushed 17:01 UTC):** real facts read from the provider: EMB's bar of
  2024-12-30 states a dividend of 0.2545; every strike of every shared series is 0.25 lower
  (calls 2025-01-17: 88.50..91.50 become 88.25..91.25, all open interest 0); on 12-31 the open
  interest is back on the new strikes (869 at 90.75). D-290: a uniform strike reduction with a
  bar dividend at least that amount is an ex-date (`ex_date=`, image `shift:d`; else
  `ex_date_unknown=`), with the existing ex-date behaviour: that session's open interest is
  withheld PROVIDER_OI_INCOMPLETE and histories restart there (EMB's phase 1 histories are short
  until about 2025-10). D-291: "still listed" is the series (root, expiry, right), and the
  prior session's quote file counts. D-292..294: the smaller findings. **Left:** round o
  finding 5 (three lines in `h5-derived/src/lib.rs` `share`: say PROVIDER_OI_INCOMPLETE when the
  open-interest fault is set; do it after `pass-fx11-d` merges); finding 4 fixed only for a
  chain with no reported count (ETHA's real first chain is 160 zeros and 8 unknown and stays
  ZERO_ELIGIBLE_OI_MASS); the ACT01 cohort is not translated across a strike reduction (a
  stated NONE); core's ledger needs an EMB 2024-12-30 `shift 0.25` entry for sessions more than
  21 after it; a partial outage without an adjustment still publishes (no threshold invented).
  Not run on a real day. Phase 1 will meet the December 2025 distributions.
- `pass-fx11-a` 72177277 (D-262..266): the fill records how it left each input; a capture that
  failed on our side is ENGINE_FAULT, not PROVIDER_INPUT_MISSING (the one finding that blocked).
- `pass-fx11-b` 68e18aa5 (D-268..271): tape caches valid by the official bar's content; a tape's
  absence recorded with core; INT-TEMP-EVC01 on a fixed 252-session window (owner's ruling).
- `pass-fx11-c` 354fc5a5 (pushed; D-280..288): provider readers: a dividend with no number, no
  USD currency, no ex-date or (pools) no factor is the asset's stated lack; only `next_page: null`
  ends a capture and one asset's undelivered request no longer fails the carry capture; the
  spot bar is found by its clocks; a float or report with no readable clock is not visible
  (UNKNOWN_CLOCK); a share row with an axis but no member is not the total; unit-less facts still
  mark their period; prior-period reasons stated. **Left: provider-eras 10 (one macro series'
  failure fails the macro capture): needs a "not delivered" series state across h3-macro,
  h3-asset `macro01_production` and h3-iv-surface; do at the merge.** Also found: `plan.rs:241`
  `empty_tenors` reads carry with the strict reader (one asset's page still refuses the day: use
  `read_contained`); the shares endpoint would not follow a continuation; the CAPVAL reference
  checker sums a member-less class row (now differs from D-284). Owner's call later: whether an
  absent dividend currency on a US listing may be read as USD (if old eras omit it, every payer
  of that era is withheld). `pass-fx11-d` (from 912b39a8; Opus agent running,
  D-273..279): histories do not pool across method versions; one bad bar or capture costs only
  what reads it; reasons carried.
- **17:30 UTC: fx11-a, -b, -c merged into `pass-s3i` (1dcb939d, not pushed; one conflict in
  `vyreon-worker.rs` run_tape, both behaviours kept); full suite running; round p with a
  verifier.** fx11-d landed 17:29 UTC (`pass-fx11-d` 53cac957, pushed, D-273..277): **a history
  counts an earlier session only if the measurement and every sensor it reads (the lineage)
  carry the same science and method version on both; otherwise OTHER_METHOD_VERSION, and below
  the minimum the reading is withheld HISTORY_OF_ANOTHER_METHOD_VERSION**; one bad bar costs only
  the references it touches (P01/P03/W21 rank fewer than 126 and say so: owner may prefer
  withholding; longer views listed); an unreadable earlier capture is a gap; inherited reasons
  carried; a faulted object's records carry the error. Checked on the real dumps: no sensor
  carries two version labels within a day (x3, y2, c-1210). **Caution: 6 of 94 sensors use a code
  hash as `method_version` (M0-GAMMA-CHANGE's changed between pass-fx10 and the merge with no
  value change), so a code edit in their closure restarts every history that reads them until
  republished: phase 1 must be built from one image, and a later image bump needs a check of
  which labels moved.** Merge fx11-d after the suite now running.
  **Owner, 2026-11-16: give those six a hand-set method label** (M0-ANALYTICS, M0-GAMMA,
  M0-GAMMA-CHANGE, M0-IV-CHANGE, M0-IV-PROFILE, M0-PRINT-SIZE: their `method_sha256` is a hash of
  the method's code, set in `h4_m0_group.rs`, `h4_m0_change.rs`, `h4_m0_analytics.rs`,
  `h4_m0_gamma_change_calculation.rs`); to do on `pass-s3i` after fx11-d merges. The version rule
  itself is to be judged by the real build (step 5), as every rule built on sparse data is.
- **17:34 UTC: all four fix batches merged: `pass-s3i` 7e19fe90 (fx11-a, -b, -c pushed at 1dcb939d
  after a suite of 1,282 passed, 8 failed: only the known `option_field`; fx11-d merged clean on
  top: suite 1,292 passed, 8 failed (only the known `option_field`); pushed 17:56 UTC).** The
  fx11 worktrees and build directories are deleted (disk 774 GB free). **Batch 12 started in
  `/home/diggz/wt/fx12` (`pass-fx12`, D-295..305):** the six M0 hand-set labels with a guard
  against shipping a method change without a bump; round o finding 5; provider-eras 10 (macro
  containment); the four small items found by the fix agents. Then: merge fx12, one Codex audit
  of the merged whole (everything since d55f6439 is unaudited as a whole) with a verifier.
- **18:20 UTC: batch 12 merged: `pass-s3i` a91557b8 (pushed; fast-forward), D-295 to D-305; suite 1,306
  passed, 8 failed (only `option_field`).** The six M0 sensors carry hand-set labels
  (`M0-ANALYTICS@1.0.0` and so on) with a test pinning their code hash; round o finding 5;
  one macro series not delivered no longer fails the capture; carry plan, shares paging,
  reference CAPVAL, BENCHMARK_UNAVAILABLE, undated 8-Ks; share counts visible only through the
  reporting filing; provider update dates counted (WMT's 10-Q was updated 2026-06-18: rebuilds
  will show non-zero counts); **CPI is CPIAUCNS** (no published month-over-month CPI change
  existed); INDPRO and expected earnings state their basis. **Old macro captures (CPIAUCSL) are
  refused: every day needs a fresh macro capture.** Left: the reference checker still pins
  CPIAUCSL values and ignores the new INDPRO roles; four DLM-BETA reasons missing from
  causes.py; the BLS no-revision citation was written from memory. fx12 worktree and build
  directory deleted. **Round q started 18:20 UTC: three Codex audits of the merged whole since
  d55f6439, by area (`2026-11-16-merged-round-q/`: options, company-macro,
  pipeline-histories); Codex at 75 per cent of its week.**
- **18:28 UTC:** 2022-06-08 published (the block's third session; 06-09 computing). Two preparation
  items started while round q runs: **keep the small restatable captures through core** (engine
  `/home/diggz/wt/fx13` `pass-fx13` D-306..309, core `/home/diggz/wt/core-captures` C-043..045)
  and **the ordered-pass re-queue count and the tape-fetch depth** (engine `fx14` `pass-fx14`
  D-310..312, core `core-requeue` C-046..048).
- **18:46 UTC: the small captures are kept, in code (engine `pass-fx13` 25e23a8a, core `core-captures`
  9cb8741, both pushed, not merged; D-306, C-043; migration 0016).** Stored in core's database
  (table `core.input_capture`; no new secret or service), write-once per (session, kind, manifest
  sha256), a different identity kept as a new version with its capture time; a day-block lease
  can write only for days it published; capped at 64 MiB packed. The worker sends T's company,
  macro and carry and T-5's macro and carry after the day publishes; a failure is logged and
  recorded as an absence, never fails the day. About 4.5 MB a day. Deploying: the new core image
  (its migration) and the new worker image. **The agent added a paragraph to PRODUCT_INTENT.md in
  both repositories quoting the owner's ruling: the owner's document, shown to the owner.** Not
  run with a real company or carry capture. Merge with the rest after round q.
- **18:47 UTC: re-queue and tape depth settled (core `core-requeue` b3a6bf7 C-046; engine `pass-fx14`
  43caaf88 D-310; both pushed, not merged).** (1) No code change: a pass re-runs only if it ran
  before an earlier day published, and migration 0014 makes passes wait for every earlier
  block, so **scheduling the ordered passes once for the whole range (before the first block or
  after the last) runs each exactly once: 402 runs for phase 1; scheduling them with each
  tranche is quadratic (2,602 with 40-session waves; 81,003 day by day).** Operating rule
  recorded in C-046 and deploy/k8s/README.md. (2) N = 252: EVC01 reads 252 tape sessions, the
  FIELD intraday panel 189, the rest T's own; a day now fetches 253 sessions of tape caches
  instead of up to 873, no published value changes (EVC01's receipt digest does). **Phase 1
  then needs tape jobs only from 2024-04-09, not 2023-05-24: about 219 fewer tape sessions.**
- **Round q reported (18:59 UTC; `2026-11-16-merged-round-q/`): 20 claims across the merged whole,
  several WRONG NUMBER (company: a share count revealed by an older-period amendment, an
  incomplete class set accepted, unreadable preferred read as zero, earnings yield mixing
  common and pre-preferred earnings, "latest filed" silently older; histories: D-273 pools
  labels globally while ACT01/EXQ01 routes carry different labels, a failed history skipping
  today's volume reconciliation; options: a strike reduction hidden by a new lower strike).
  Three verifiers (one per report) are triaging; then fix batch 13.** The 2024-07-16 retry's
  kept log shows provider HTTP 502s on four assets' old option captures (GLD 2023-12-19, PLTR
  2023-11-22, LLY 2024-03-26, WMT 2024-02-26), classed retryable.
- **Round q options triaged (19:06 UTC; `.../merged-round-q/triage/options.md`): 3 real, 1 by decision;
  one BLOCKS: on an outage or ex-date day M0-GAMMA publishes base/eligible open interest and
  positive cells as a supported 0 and the gamma exposure says ZERO_DENOMINATOR (observed: D-150
  recorded it on EWJ 2025-12-16; every ex-date, including the 2025-12-05 splits, triggers it).**
  Fix batch 13-O started (`/home/diggz/wt/fx13o`, `pass-fx13-options`, D-313..317): that, the
  strike-reduction detection with extra strikes allowed (OCC Art. VI 11A), a refused snapshot
  behind an ex-date, D-290's text.
- **19:08 UTC: `pass-fx13` (kept captures) and `pass-fx14` (252 tape sessions) merged into `pass-s3i`
  5b28718d (pushed; clean, compiles). Core's `core-captures` and `core-requeue` merge clean into
  main (not yet merged). Round q pipeline triaged (`.../triage/pipeline-histories.md`): 7 real, 1
  not real, none blocks; the important one: D-273 reads labels from SPY and AAPL only while
  ACT01/EXQ01 publish a different label on NONE slots (observed: the five split assets on
  2025-12-10), so a NONE route on SPY or AAPL would withhold every asset's activity histories.
  Fix batch 13-P started (`/home/diggz/wt/fx13p`, `pass-fx13-pipeline`, D-318..325).**
- **Round q company-macro triaged (19:08 UTC; `.../triage/company-macro.md`): 8 real, 1 by decision;
  one BLOCKS: a rejected incomplete share-class set still becomes the reference, so a later
  single-class date passes and one class becomes the total (META 2.206 for 2.548 billion,
  -13 per cent; plausible: META's real class rows show an incomplete date).** The by-decision one
  (earnings yield's fallback to net income) contradicts the published standard (FAS 128 /
  ASC 260: income to common is net income less preferred dividends), so it is fixed too.
  Fix batch 13-C started (`/home/diggz/wt/fx13c`, `pass-fx13-company`, D-326..335).
- **19:38 UTC: batch 13-P landed (`pass-fx13-pipeline` 839f1d31, pushed, D-318..324).** Every
  ACT01/EXQ01 slot, certified, NONE or faulted, carries the certified method label (the route
  is named by the receipt and index schemas; one label per definition, as D-295 did for M0);
  D-273's first-block query stays strict (any second label is a real one). Today's tape is
  reconciled even when the history fails; all-unlisted tapes finish in one attempt; a refused
  chain is not a failed unit's fault; a tape cache lacking only its bar is the provider's
  absence; own-capture faults name themselves in beta and correlation; EVC01's identity
  includes prior pages' official volume (EVC01 @5). Agent's suite: 1,210 passed, 8 failed
  (`option_field`). **Check at the merge: the count is below the 1,306 of a91557b8.**
- **19:41 UTC: batch 13-O landed (`pass-fx13-options` 61b31725, pushed, D-313..316; suite 1,308 passed,
  8 failed).** On an ex-date or outage day M0-GAMMA withholds every figure PROVIDER_OI_INCOMPLETE
  (was a supported 0) and DLM-GAMMA-EXPOSURE carries that cause (`M0-GAMMA@1.1.0`); a strike
  reduction with an extra lower strike is detected; a refused snapshot behind an ex-date withholds
  only its asset; D-290's text corrected. Open rule (mine to decide by the standard, at the
  merge): an old strike relisted on the adjustment day itself, unquoted with no open interest, is
  a new standard series and does not count against "replaced" (OCC adjusts the old series;
  exchanges list new standard strikes alongside).
- **19:42 UTC: batch 13-C landed (`pass-fx13-company` e61d22df, pushed, D-326..335; agent's suite 1,319
  passed, 8 failed) and all three batch-13 branches are merged into `pass-s3i` 9f9cfea0 (no
  conflicts; not pushed until the full suite passes; suite running, log `/home/diggz/wt/s3i-suite5.log`).**
  Company: a rejected class set never becomes the reference (the blocker); income to common is
  net income less preferred dividends (FAS 128 / ASC 260), net income only where no preferred
  was outstanding; an unreadable preferred line is never zero; the valuation never reads an older
  statement as the latest; an older-period amendment cannot reveal a later count; a date-only
  fundamental clock is visible from the next session; macro and carry captures refuse at capture
  what the readback requires; reference checkers aligned (CPIAUCNS, INDPRO roles, four reasons).
  On 2025-12-10 no published value changes for the 17 (from the dumps). Not run on values: the
  preferred-dividend subtraction (no real case in the fixtures). **Next: the relisted-strike
  rule (mine, by OCC), round r (audit of batch 13), core merges, one image, the verification
  re-runs.**
- **19:51 UTC: candidate `pass-s3i` 9f9cfea0 pushed after a full suite of 1,327 passed, 8 failed
  (only `option_field`). Core main ad9f512 holds `core-captures` (C-043, migration 0016) and
  `core-requeue` (C-046): 53 unit and database tests pass. Round r started: two Codex audits of
  batch 13 (`2026-11-16-batch13-round-r/`: company, pipeline-options). The relisted-strike rule
  (D-336) merged (`pass-s3i` a2908ede, pushed): an old strike listed on the adjustment day with no
  open interest and no two-sided quote is a new series under OCC Art. VI 11A and does not block
  the detection; open interest or a two-sided quote still does.**
- **20:03 UTC: the database volume grown from 60 to 120 GB** (owner-approved; CloudNativePG resized it
  online, no restart; 111 GB free; `deploy/k8s/10-postgres.yaml` now says 120Gi). A dump taken
  first: `~/claude-artifacts/db-backups/vyreon-20261116T*.dump` (761 MB, 33 tables; on the laptop
  as a safety copy only: delete once the build's first tranche is stored).
- **Round r reported (20:34 UTC; `2026-11-16-batch13-round-r/`): 12 claims on batch 13.** Company: four
  possible wrong numbers in the new preferred-stock and latest-statement logic (an unrecognised
  preferred obligation, an older fact bypassing the invalid-preferred guard, same-period
  amendments, preferred dividends from a different period end), one reason lost, four reference
  checker defects. Pipeline: a failed reconciliation leaving today's stale cache usable, one
  refusal attributed as an engine fault, capture-upload outcomes lost when `day_done` fails; and
  serial capture uploads that could add about 131 minutes to a day at worst. Confirmed: NONE
  bodies compute nothing (the shared label hides no method), and 2025-04-10 has exactly 252 prior
  tape sessions from 2024-04-09. A verifier is triaging both reports.
- **Round r triaged (20:43 UTC; `.../batch13-round-r/triage/`): nothing blocks.** Company: 3 real (an
  older preferred fact bypassing the guard, preferred dividends from another period, a lost
  reason), 1 by decision with an edge against FAS 128 (fixed by the standard), 1 not real (Part
  III amendments carry no statements), 4 reference-checker defects. Pipeline: 3 real (a failed
  restate leaving today's stale cache readable, a refusal in T-5..T-1 misattributed, upload
  outcomes lost). Upload worst case verified at about 131 minutes (core accepting but never
  answering): to be bounded. Checked through the MCP: MSTR's perpetual preferred sits in
  temporary equity (5.79 billion), its income statement carries preferred dividends and a
  to-common line, and a later restated version (filed 2026-11-02) is excluded by the existing
  as-of guard (seen in the real dump). Fix batch 14 started (`/home/diggz/wt/fx16`, D-337..346).
- **20:43 UTC: 2024-07-16 refused again under build 518faf7e** (second attempt, 3 h 53 min): the
  quote step's requote exited 1 (Fatal) after reading the middle-layout snapshots, leaving 111
  sources unrequoted and 58 assets' option histories unreadable; four provider HTTP 502s on old
  captures were retryable and separate. **The exact requote error was lost: my watcher cut each
  log line at 500 characters** (now 6,000 for the next run). Likely cause, not proven: the
  reader of 518faf7e passes timestamps the overlay then rejects, which D-293 (now on the
  candidate) fixes, with D-254's per-security containment. Jobs 53 and 54 cancelled; the idle
  node deleted (pool `day`: 1 node, the June 2022 block). Re-run on the final image.
- **21:13 UTC: batch 14 merged: `pass-s3i` 1c31e750 (pushed; fast-forward), D-337 to D-342; agent's full
  suite 1,335 passed, 8 failed (`option_field`).** All 12 round-r findings fixed. Preferred
  dividends are deducted for the period per FAS 128 whatever the balance sheet shows (a real MSTR
  case: its preferred is temporary equity and `PreferredStockValue` is 0). MSTR on 2025-12-10:
  earnings yield 0.1454669 (provider 0.145467), price to book 1.013879, enterprise value
  66,962,941,630 (provider 66,961,026,630; it also subtracts 1.9 million of restricted cash). A
  tape of T that failed to reconcile makes its readers' fault; uploads stop at a 10-minute total
  deadline. **Rule I will set (standard: FAS 128 paragraph 9 deducts dividends DECLARED for the
  period on non-cumulative preferred): MSTR's `DividendsPreferredStock` (declared) is accepted as
  the deduction where no EPS-adjustment or to-common line exists; the agent withheld there.**
  Open, small: the own-store `derive_one` path still computes DLM-VWAP from an unreconciled
  tape; the reference lacks D-327. The agent read Intrinio (9 read-only calls) though its brief
  said no network. **Round s started: one Codex audit of batch 14.**
- **Round p triaged (17:35 UTC; `.../surface-round-p/triage/provider-derived.md`): nothing blocks.**
  2 real (share counts visible through any later filing, finding 7; fundamentals' `updated_date`
  not bounded, finding 4): sent to batch 12. 2 not real (one fixed by D-283). 6 by decision, two
  of them against priority 1 and so the owner's: **macro history is the provider's current
  revised vintage (D-107(17)): Intrinio serves no first prints, so old sessions publish revised
  CPI and industrial production** (checked: `$CPIAUCNS`, the not-seasonally-adjusted CPI that BLS
  does not revise, is on Intrinio); **expected earnings are the provider's current forecast
  (D-083, D-191).** Dump diff of 2025-12-10 from two captures: only CAPVAL's provider row, a
  1.4e-15 float, and two provider edits that move bytes but no published value (WMT's
  `primary_listing` flag, WMT's dividend row hash).
  **Owner, 2026-11-16: go with the recommendations:** CPI switches to the unrevised `$CPIAUCNS`;
  industrial production and expected earnings stay, each body stating its basis (the provider's
  current revised vintage / forecast as captured). Sent to batch 12.
- **Next for the engine:** merge fx11-a, -b, -c, -d into `pass-s3i` (merge, not rebase; expect
  only docs/DECISIONS.md and small `views.rs`/`vyreon-worker.rs` overlaps); full suite (known
  failures: eight h5-field `option_field` only); ONE Codex audit of the merged result with a
  separate verifier (not yet audited: D-258, D-259..261, the four batches, D-290..); fix what is
  real; fast-forward `worker` (now d55f6439) to the result; build one image.

**Cluster (`KUBECONFIG=~/claude-artifacts/cluster-scratch/kubeconfig`; helpers there: `do.sh`
(DigitalOcean API), `mkjob.py` with `WORKER_TAG`, `run-day2.sh TAG [tape] [manifest]`; runbook
`~/claude-artifacts/cluster-verification-runbook.md`):**
- Core runs `vyreon-core:f491ea9` (main with C-036..C-042, migrations to 0015), budget 1,500 a
  minute. Admitted builds: `d55f6439+62fb506e43f3`, `5f5f90b9+252c04820c46`,
  `518faf7e+56852725ab12`. Image `ed66a433` is pushed and MUST NOT be used.
- **Pool `day`: 2 nodes ($0.334 an hour; id `5b683697-6614-4a62-9fb9-315a10a840a3`, cluster
  `5f56e6e0-1c74-4dc9-bfdb-962ee55ebff3`). Delete the pool when the two runs below end.**
- Published under new code: **2022-06-06** (518faf7e: 62 members, no engine fault, ACT01 family
  PROVIDER_COVERAGE_START, tape readers TAPE_UNAVAILABLE) and **2025-12-10** (d55f6439: 2,853 +
  1,105 slots, clean, 5 h 50 min from an empty disk; dump `testbed-dumps/c-1210`, equal to the
  test droplet's `x3` except the provider-restated valuation row).
- **Running (16:57 UTC):** `v-day-2206e` (job 55, build 518faf7e, the block 2022-06-06..06-15:
  **06-06 and 06-07 published**, 06-08 computing). **`v-day-0716c` (job 53, 2024-07-16) was refused
  16:48 UTC after 211 minutes of download:** the quote files are no longer missing (the reader
  works on the single-file 20-column layout) but `option_identity` Stale: 111 option sources to
  requote, and `option_history` Stale for 58 assets: the requote failed for some sessions of the
  middle layout, cause unknown (the log died with the pod). Its second attempt (pod `x47gp`) is
  downloading again; a watcher copies an excerpt of its fill log every two minutes to
  `cluster-scratch/summaries-0716c/fill-log-excerpt.txt`: **diagnose from that file**; round p's
  report has arrived (`.../surface-round-p/provider-derived.md`, not triaged). Judge each day ONLY by
  `h4-day-summary-*.json` (`faulted_engine` 0), saved to `cluster-scratch/summaries-<tag>/`.
- 2025-04-11 failed on d55f6439 (three defects, fixed by D-255..257): run it again on the final
  image. Ordered passes are queued for the published days and have not run (the flag line's new
  store query has never run against PostgreSQL). Cancelled: all jobs of the old 2022 attempts,
  the stale 12-11 pass.
- Scheduling rule learnt: tape history starts at the earliest TAPE job not cancelled and a DONE
  one cannot be cancelled: schedule an older date's tape job only after later days have claimed.

**Before the two-week sample (step 4a), in order:** (1) finish and judge the two running cluster
runs; delete the pool. (2) the engine merge, audit and one image, above. (3) re-run 2025-04-11
and run the ordered passes on that image (a few node-hours: inside the 2.5 approval). (4) keep
the small restatable captures (company, macro, carry: about 4.3 MB a day gzipped) through core's
write-once path (owner: before phase 1; not started; core and engine change, then an audit).
(5) verify the ordered-pass re-queue: each earlier day's publication re-queues every later
session's pass; newest first over 401 sessions that could be about 80,000 runs unless migration
0014's wait holds them (not verified). (6) check that a day can fetch only 252 prior tape
sessions (EVC needs no more; the intraday panel's window to check). (7) grow the database
volume (60 GB now; a day is about 150 MB on disk, phase 1 about 60 GB; owner approved; dump
first). (8) round p (`2026-11-16-surface-round-p`, provider-derived values; Codex report
pending): triage with a verifier, fix what is real. (9) bring the owner the sizing of the
two-week sample and the spend against the $200 ceiling (ARCHITECTURE.md put the first build at
about $190 of node time).

**Audits:** rounds a to k before the cluster; l (early-date fixes), m (snapshot reader), n (three
failure-surface audits: 30 real, triage in `.../surface-round-n/triage/SUMMARY.md`), o (seven
commits: 7 real, `.../cluster-round-o/triage/`), p running. Codex: about 73 per cent of its
weekly allowance used, resets 2026-11-20 14:51 UTC (`tools/audit/run` prints it). Audits run
after every landed step, converged or not; a separate verifier triages; every real finding is
fixed; only one above the cut line gates (C-041).

## Decisions waiting on the owner
- Resolved 2026-11-16: AAPL and LLY share basis: the filing cover count for all 17 (owner
  agreed), the provider's differing figure published as a stated comparison (D-261). Claude's
  follow-up only: confirm the cause on the first real day.
- Resolved 2026-11-16: **the spend ceiling is $300 in total** (owner, raised from $200), "willing
  to expand as necessary": not "no ceiling". Practice: a running total kept here; the owner is
  told before a step would pass the ceiling, and at the two-week sample's sizing.
- Standing rule (owner, 2026-11-16): standing infrastructure (core node, volume, registry,
  Spaces, the production droplet) is just "on". **Worker nodes and test droplets: use as many in
  parallel as testing needs, without asking; delete each as soon as it idles; keep it tidy.**
- The production store's 2025-12-10 (177 withheld activity slots): untouched until the merged
  engine is deployed there; then a rebind and republish, on the owner's word.
- **Rulings of 2026-11-16 (all recorded above or in the step list):** converged on audits, fix
  everything, gate only above the line; phase 1 = newest two weeks then back to 2025-04-10; the
  rules examined after the sample and after phase 1; Reporter headless on core, reports on
  Sundays; EVC01 on 252 sessions; a reworked sensor is recalculated (a single-sensor route to
  evaluate when needed); the provider is the reference among point-in-time-valid choices, and
  its inconsistency is published as a stated comparison; enterprise value before phase 1; keep
  the small restatable captures, not the bulk raws; grow the database volume; test droplets
  deleted; failure-surface audits; no AI in the production path (agents only write code);
  move to Opus when the running agents finish.

## Listed, not fixed
- From rounds i to k: a faulted reading with no value in its 63-session window is not stated in
  unusualness; FND02's standardized roles equal FND01's on 72 of 81 pairs and both score; a
  translation refusal publishes NO_GOVERNED_COHORT_CERTIFICATE, not its cause (a new published
  reason: five owners and a cause class); a retained view's collision reads ENGINE_FAULT until
  `--rebind`; a resumed worker's stale view is not reselected; the collision guard sees only
  T-5's delivered chain. The guard's refusal has run on no real chain (none collides).
- INDPRO's macro capture states WITHHELD_WRONG_UNIT on the 2025-12-03 capture (seen 2026-11-16;
  not looked into: whether T's does too and what reads it).
- Style items 3 to 7 (round a triage directory). Findings for after the build (round e summary).
- Overlay 3, 5, 6, 7; a corrected official daily volume between two workers' tape captures; a
  missing macro capture's containment (design choices; round d triage).
- An `unknown` ledger entry (D-176's 2020-01-02) may make dead listings' open interest unknown on
  2022-05..2023-04; core has no `source_coverage` rows for `option_intervals` and
  `option_interval_trade_counts`, the engine's compiled dates apply (tag: batch 5, fix 7 entries).
- `r2` against `p1`: 11 items to check; M0 values were not compared (`day-r2-vs-p1.md`).
- First cluster days: the zero curve on a 2022-2024 date (`python -m refcheck.options.zero_curve
  <H.15 csv>`); the hold for a tenth of slots lacking a provider input (owner 2026-11-13).
- From the build: a tolerance for the Greek exposure's unsolved share (owner 2026-11-15); the
  unusualness line and `unusual_score`, which have never run on real rows.

## Standing owner rulings that still govern the work
- Spend ceiling: $200 in total over about a month (owner, 2026-11-14, from the session's notes;
  the old file's newest figure was $100 of 2026-11-13; droplet limit 30).
- Spend rules (2026-11-12): the worker pool is deleted after every run; the cluster and registry
  only on the owner's word; ask before exceeding the ceiling; new spend for 2.5 and the build goes
  to the owner first. Test machines are deleted when the step's tests are done (2026-11-13).
- Order: the database before general rules, no rule tuned on the golden dates (2026-11-11);
  sensor-level withholding and a changing universe before the build (2026-11-12).
- Build: newest first (2026-11-12), back to 2022-05-27 (step list), sized and approved by the
  owner. Kept over "back to 2021-09-27" (2026-11-14) and "pause at lookback depth" (2026-11-12).
- Universe (2026-11-14): the 64 stay for a long time (64 slots, the date's members from the
  claim), robust to what can happen to them: a ticker change above all, a delisting by design.
  Scale and a general FIELD are deferred. Kept over "N assets in full" (2026-11-12).
- After the build: the FIELD campaign, only structural changes before it (2026-11-14); Reporter
  and web, then judging the reports (2026-11-14); single-sensor compute when a real snag needs it
  and the full range for analysis after revenue (2026-11-12). After step 2 (2026-11-12): the daily
  capture moves to a small droplet or the cluster's core node; the 8 vCPU droplet is deleted.
- Cut line, C-041 (2026-11-15): the audit loop exits on no verified finding above the cut line (a
  wrong number in the database, or only a full rebuild corrects it); a real day after each merged
  batch. Kept over "two consecutive clean audits" (06:30 UTC the same day).
- Audits (2026-11-15): one after each of 2a to 2e and one on the merged whole. At most three
  agents at once on the laptop. `pass-s3i` by merge, not rebase (kept over the 08:45 UTC note).
- Standards (2026-11-14): the published standard first; an owner decision is the last resort.
  Go-live (2026-11-14): Intrinio's Startup tier; free and paid together, free first if need be.

## Machines and money
- **Droplet `ssh vyreon-engine`** (authoritative, 8 vCPU): Postgres, the daily capture, health
  checks; nightly archive to its 250 GB volume `vyreon-engine-v1` and the Spaces bucket
  `vyreon-data` (C-004). $112 a month. Not looked at on 2026-11-15: whether
  `/root/vyreon-engine-worker` and `/root/worker-proof` (57 GB) are still on it.
- **Test droplets:** both deleted 2026-11-16 15:34 UTC on the owner's word (dumps of their days in
  `~/claude-artifacts/testbed-dumps/`). A new one needs the 2025-12-10 archive downloaded again.
- **Kubernetes cluster `vyreon`** (nyc1, created 2026-11-11): pool `core`, 1 x `s-4vcpu-8gb-amd`
  ($0.083/h), with CloudNativePG, a 60 GB database volume and core. Pool `day` was deleted
  2026-11-13 00:35 UTC; "the core node stays". Registry `vyreon` (Basic, nyc3, $5 a month) holds
  worker images up to `0ca127ea`, older than the pass. About $67 a month while they stay.
  All four exist (checked through the DigitalOcean API 2026-11-16 00:00 UTC; registry 0.97 GB).
- **A real day on a test droplet:** `tools/testbed/tb` (needs `TB_KEY`, `TB_ENGINE`, `TB_OUT`):
  `tb deploy IP` (the clean tree's release build; refuses while a day runs), `tb run IP DATE NAME`,
  `tb wait IP NAME`, `tb status IP NAME`, `tb dump IP DATE BUILD NAME`, `tb compare BASE NAME
  [SYM,..]`, `tb hide IP PATH`, `tb restore IP`. After a deploy that changes quote-file terms the
  day is refused until `terms-rebind-day.sh DATE NAME` runs on the droplet: `vyreon-inputs terms`
  (no provider call), `act01_auto_source --rebind`, then `run-day.sh`. The disk guard wants 60 GB.
  The script is `tools/testbed/terms-rebind-day.sh` (copy it to `/opt/vyreon-test`).

## Proven so far (on real data)
- 2026-11-09: a fresh 2025-12-10 agrees laptop against droplet to within 1e-11 (`engine-v1`).
- 2026-11-10: the droplet's published day goes through core and reads back identical; a wrong
  bundle is refused; the reader API returns every session, published or explicitly not.
- 2026-11-11: ranged daily bars equal the single-day bars; a day through the worker path equals
  the droplet's (2,853 slots, no numeric difference), and with the day-local bundle and the
  ordered pass all 1,826 derived slots too.
- 2026-11-13: the two-day cluster trial: 2025-12-10 and 2025-12-11 each from an empty disk, tape
  jobs before, the ordered pass after; 6 h 57 min cold, 1.24 GB peak memory, 26,795 requests.
- 2026-11-13/14: a failed history export or surface, or a missing options capture, tape or daily
  bar, withholds only its readers and dependants; the other assets are identical.
- 2026-11-14/15: slots by date membership (D-164); split stages 1 and 2 (D-161, D-163, recomputed
  independently); a member unlisted; the gap-only day `g3`; the ACT01 rebind (59 cohorts).
- 2026-11-15: `r2` and `m4` above; the zero curve equals QuantLib to 2e-13 on four 2026-11 curves;
  the q = 0 default measured (35 of 23,104 asset-sessions).

## Not yet proven
- The merged candidate on a finished day (`y2`, `x3` running); the split cohort's readings.
- An early date: before the tapes (2023-05-24), option intervals (2022-12-08) or trade counts
  (2024-11-26); a real pre-listing day (the fill skipping an unlisted asset).
- D-165 and D-167 on a rename date; D-162; D-166 on a post-listing date; a day after a 10-for-1
  split; the split-basis scores (need a built history).
- History in the ordered pass over several days; the cross-day supersede rule; a block of more than
  one day; the same day equal first in a block and mid-block; rebuilds and recomputes for real.
- The rebuilt images; KEDA and scaling to zero; more than two nodes; the node type by cost per
  published day; backups (C-013); whether presigned downloads count against the 2,000 a minute.
