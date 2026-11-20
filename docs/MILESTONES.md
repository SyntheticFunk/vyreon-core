# Milestones

The one living status document: current state and what is next. History is in git (the longer
forms are at tags `milestones-2026-11-15` and `milestones-2026-11-16`); decisions are in
`docs/DECISIONS.md` (core, C-numbers) and the engine's `docs/DECISIONS.md` (D-numbers); the plan
is in `docs/ARCHITECTURE.md`.

## Steps to the MVP (the list shown to the owner on a status request; keep it current)
**DATED COMMITMENT: the paid pilot (live on www.vyreonlabs.net/early-access, read 2026-11-16).**
One customer has paid USD 10 for four weekly email issues on Sundays **2027-01-10, 01-17, 01-24,
01-31** (sales close 01-09; automatic full refund if issue 1 is not sent by 23:59 ET on 01-10;
refunds on request to 02-07). Promised: all 64 securities; readings flagged when far from the
security's typical level AND in the top or bottom tenth of its last 63 sessions (the engine's
`unusual.rs` WINDOW = 63, robust z: matches); "what could not be measured, and why"; data
through the latest session; no signals or forecasts. So by 2027-01-09 we need: (1) history
back at least 63 sessions before 2027-01-08 (phase 1 covers it); (2) **daily go-forward runs**
computing each new session from the build onward (built 2026-11-17, branch `core-daily`, C-051:
core queues T at 07:30 New York the next day, CronJob `daily`, pool `day` to min 0; not deployed;
runbook `deploy/k8s/README.md` "Daily runs"); (3) Reporter producing the paid issue from
core's API, sent through beehiiv. **Owner, 2026-11-16:** the deadline will be met; target free
reports live in **December 2026**. Intrinio's Startup tier (USD 333 a month, scaling to USD 999)
is required for redistribution rights and covers free and paid reports, so it starts before the
first publication of either; ideally free and paid are ready together. Google Ads all-time:
1.58M impressions, 156k clicks (CTR 9.88%).

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
   **Process additions (owner, 2026-11-16, all approved):** (a) the sample's pass criteria written down and
   approved before it runs; (b) every new image re-runs one real day on the cluster and a script
   diffs its store against the previous image's dump, each changed value tied to a decision (the
   laptop diffs; heavy compute stays on the cluster for disk reasons); (c) a refusing worker uploads its full log and fault
   list to core before it exits (observability; built with the image); (d) the sample's raw
   downloads kept in Spaces for fast re-runs, deleted when phase 1 starts; (e) **planned hand-off
   to a fresh session after the two-week sample**, before phase 1, recorded here first.
   **Rule census in step 5 (owner, 2026-11-16):** "with agent driven development on things of this
   scale ... we just sort of accumulate rules over time and the result can be an unfortunate agent
   stuck in spaghetti constraints". The engine has 324 recorded decisions (12,081 lines; 257
   commits to DECISIONS.md since 2026-11-09). On the built phase-1 database, for every rule in
   force: how often it fired, on which assets and dates, what it changed (value or absence),
   and what evidence it was written from. Never fired, fired only on its origin case, or
   contradicted by the population → retire or merge; the agents' instructions are pruned the same
   way. **No rule tracking is added (owner, 2026-11-16: "feels like complexity that will cause
   problems ... If rules are being applied on some days but not others then we shouldnt be doing
   it").** The census reads the outputs instead: absences by reason code, per sensor, asset and
   date; continuity of each series. Rules required by a published standard for real events (OCC
   adjustments, splits, filing dates) fire only on those events by nature; a science rule or
   heuristic that changes some days and not others is a candidate for retirement.
   **Backups (owner, 2026-11-17): "given that recomputation has a known time and money cost its
   actually WORTH backing shit up."** CloudNativePG's scheduled backups to Spaces (base backup plus
   write-ahead log), one tested restore. **Set up when phase 1's first tranche passes its census**
   (owner: before that there is nothing worth keeping). **Weekly full backup** (owner: daily data,
   daily fulls cost more for nothing) plus continuous log archiving, so any moment is still
   restorable; about 30 days kept; a bucket and key only for backups. The droplet's capture and archive keep running meanwhile.
   **Security before launch (owner, 2026-11-17), part of step 7:** least-privilege tokens replace the
   full-access DigitalOcean token once the system's shape is settled (it stays for now); the
   reader API stays inside the cluster (Reporter runs as a container beside core; its output is
   pushed out, nothing is called in); backups to Spaces with one tested restore; a read-only
   exposure check (public ports, token scopes, secrets in git history, dependencies) and an
   attack-surface audit before launch. Owner's phone is the one recovery factor: offline backup
   codes and port-out protection are the owner's to set.
5. **(3.5) Examine the built database (owner, 2026-11-16).** Done twice: **a first pass after the
   two-week sample** (thin: about ten sessions, so it can show a rule that misfires or never
   fires on ordinary days, not how one generalises; anything it changes is fixed before going
   back to 2025-04-10), and **the full pass after phase 1**, before Reporter: go through the rules and decisions against the built store and ask of each
   whether it **generalises cleanly**: from the census (how often each rule fired, on what, and
   what was published), which rules are doing real work, which over-fire, which never fired,
   which rest on one case and should be retired or restated, and where a missing threshold
   needs the population's evidence. Outcome: a short list of rule changes (each then a scoped
   recalculation), and the rules that stand.
6. **Verification on vyreon-web (owner, 2026-11-18: "we definitely need to expose what we are doing
   in the marketing. Major verification and checking areas should all be highlighted on
   vyreon-web eventually").** A methodology/verification page listing each external check (Cboe
   volatility indices, SEC EDGAR, OCC volume, FRED, a second vendor's prices, QuantLib/TA-Lib
   recompute, Cboe DataShop sample): what is compared, the declared band, the latest result, and
   the failures and their fixes. Within the plain-voice rules (no "accurate", "audited",
   "institutional-grade"): specific, dated, failures shown. Naming Cboe's indices waits on Cboe's
   permission reply; check each provider's naming terms (Tiingo, OCC) before publishing.
   **(4) Reporter on the phase 1 database.** First a proper paid report (none has been produced
   **Weekly comparison idea (owner, 2026-11-17):** per sensor, plot this week against last week on
   one chart. State readings (IV term structure, skew, open interest by strike, levels) compare the
   week's closing reading (Friday against last Friday); flow readings (volume, activity, intraday
   profiles) compare the week's total or daily mean. The weekly aggregates are **derived-layer
   measurements** (owner: that is why the derived layer exists), each method stated, computed from
   stored readings in the history pass; Reporter only draws them. **Built right after the
   two-week sample** (owner, 2026-11-17: "its quick to do"); the sample's two full weeks are its
   first test data.
   One mechanism per kind of quantity (owner): state → the week's close; flow → week sum or daily
   mean; ratio of flows → recomputed from the week's summed parts (never an average of daily
   ratios); state curve → the week's closing curve; flow curve → summed per bucket on a common grid;
   already relative (percentiles, z-scores, unusualness) → not aggregated, feeds the paid review. A
   short table assigns each sensor's values to a kind. **Presentation research pass (owner, 2026-11-17), when the first real reports
   exist:** the inventory (all prior design work is Reporter's `docs/publication-design/`, dated
   2026-09-25, before the pivot) leaves these gaps: week-over-week overlays and flow curves (G05);
   an open-interest heatmap by strike × expiry (C04 has only coarse 5×5 bins); the volatility surface
   (C06 ruled out a smooth surface: revisit with real data); mapping today's sensors to chart types;
   competitors' actual chart forms; citable sentences for answer engines; shareable chart images
   and social cards. Owner: "This is direct reader value... this
   is what i would want." 
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
- **17:44 UTC: 2025-04-11 PUBLISHED on 1a532d8b (refused three times on older builds): 2,684 of 2,688
  measure slots and 512 of 512 day-layer slots complete, 0 engine faults;** 4 ETHA readings withheld
  as EMPTY_DELIVERED_POPULATION (its prior week had no options yet). 2025-12-10 still computing.
  **My error: `delete-idle.sh` matched pods by name (v-day/v-tape/v-pass), missed `v-sample`, and
  deleted the sample's two nodes 4 hours into its downloads.** Fixed: it now selects pods by the
  label `app=vyreon-worker`. Pool back to 3; the sample's jobs (1380, 1381) re-claim as attempt 2
  when their leases lapse, starting their downloads again (about $1.3 and 4 hours lost).
- **19:10 UTC Nov 18: gap found, owner decision: the reference check runs on the worker, before the
  raws are deleted.** `refcheck run` recomputes from each day's raw inputs, and workers delete them
  once the day publishes, so none of the sample's published days can be checked (criterion 4).
  Owner: raws cannot be stored for all of phase 1, so the worker runs refcheck between publishing
  and pruning, and the report goes to core. To build before phase 1: refcheck in the worker image
  (QuantLib 1.43, TA-Lib 0.8.2), its report recorded per session in core, a disagreement as one
  notice; its time per day measured (it adds node-hours). The sample's criterion 4 (and 7, the
  image diff) then from one sample day re-run on the new image. The owner's key file
  `spaces-only-key.txt` is gitignored (the repo is public). Owner: the cost is worth it; and
  "we have done the ten day spot check without actually checking any of it against external
  references": so far the sample shows the system runs (10 days, 0 failures), not that its
  numbers are right. **19:13 UTC: the bucket-limited Spaces key works** (`spaces-storage-access-key.txt`,
  gitignored: lists `vyreon-data`, refused the account's bucket list); cluster Secret
  `vyreon-core-spaces` created from it (key-id, secret; not printed).
  **Owner (19:20 UTC): the sample is salvaged by two external checks, EDGAR (company figures
  against the filers' XBRL facts, data.sec.gov companyfacts) and Cboe's volatility indices (VIX/SPY
  existing in `refcheck.outside`; add VXN/QQQ, RVX/IWM, GVZ/GLD, OVX/USO, bands committed before
  comparing): "If those pass within tolerances then the system has passed what I would consider a
  major verification."** Both read the sample from core (not day dumps). The step-5 count of
  published and withheld slots is the "coverage count" (not the B0/B1 census, which included
  the reference checks). Then phase 1's first tranche runs with refcheck in the worker, gated.
  **19:23 UTC: Cboe check PASSES on the 8 published days** (bands committed first, 4cabd03,
  `tools/external/`; report `~/claude-artifacts/checks/2026-11-18-cboe-sample-8days.json`).
  Index minus our VS-IVS01 iv30, points (band -1..8), and correlation over the 8 days: VIX/SPY
  3.3..7.6, 0.998; VXN/QQQ 2.9..6.1, 0.997; RVX/IWM 2.3..4.8, 0.996; OVX/USO 3.8..5.3, 0.997;
  GVZ/GLD 1.9..3.3 (GVZ moved only 1.8 points, so its correlation, 0.69, is reported, not
  judged). Widest gap 11-05 (VIX 28.2): the skew premium widens with the level, as expected,
  but sits near the band's top.
  **19:42 UTC: EDGAR check (declared f56347d before comparing, tolerance 1e-9; tool eb3b17d;
  report `~/claude-artifacts/checks/2026-11-18-edgar-sample.json`, 9 dates) FAILS under its rules,
  only on the two 20-F filers.** 15 domestic filers: every comparable figure passes, 1,925 of
  1,926 PASS bitwise equal to the as-first-filed SEC facts; filing clock 153 of 153; no
  restatement or look-ahead. BABA and TSM (26 FAIL): the provider's "as reported" USD is its own
  conversion (local fact / the filer's convenience rate, unrounded), labelled USD; level
  differences ~1e-7 relative; single-period ratios equal the local-currency ratios. **Real defect:
  ROA averages two years' assets converted at two different convenience rates** (vs the
  local-currency ROA: BABA FY2023-26 -3.9%, -2.5%, -0.25%, +2.5%; TSM 2022-24 -4.4%, +0.2%,
  -3.1%). To fix in the engine before phase 1: cross-period company ratios in the filer's
  reporting currency, and a converted amount stated as converted. Not comparable (137): TSM's
  FY2025 20-F not yet in companyfacts; BABA's capex element; per-class-only share counts (COIN,
  META, MSTR, PLTR, GOOGL). To investigate: GOOGL's `reported_shares_outstanding` is dated
  2016-02-01 on every date (stale?). 11-13 added when it publishes.
  **19:46 UTC: external-reference research done** (`~/claude-artifacts/research/2026-11-18-external-references.md`;
  94 sensors: A same quantity 16, B band 7, C independent recompute 39, D internal only 15,
  E nothing 17). Four pilots ran on stored days (those days are no longer blind for them).
  **Lead to investigate before phase 1: our option volume omits adjusted roots** (EWZ 2026-11-12:
  `2EWZ` 61,400 contracts, 38% of the day; OCC put share 0.606 vs ours 0.672; standard roots
  0.1-1.1% below OCC, put share within 0.003): engine rule or Intrinio delivery, unknown.
  Shortlist: OCC Volume Query (free, history) vs VS-O06, DLM-PCR-VOLUME, DLM-OPTIONS-VOLUME;
  Cboe iv30 bands to 14 assets (VXAPL VXAZN VXGOG VXEEM VXEWZ VXTLT VXD VXSLV VXFXI; pilot saw
  AAPL 2025-04-11 gap 8.2 and an EEM expected-move -1.3); FRED, EDGAR 8-K times, CIK (FRED pilot
  exact); Tiingo Commercial $50/mo (prices, technicals, FIELD recompute); OCC per-series open
  interest captured daily from now (current-day only). Massive out (business $1,999/mo options;
  personal plans barred). DataShop (owner's quote: $300 SPY 13 months; all symbols $2,520): one
  month, April 2025, SPY + TSLA + a thin chain, about $70. Terms to confirm: Cboe (index data
  beyond personal use: permissions@cboe.com), OCC (terms page blocks automated reads).
  **DataShop purchase list (given to the owner 19:50 UTC):** Option EOD Summary with calcs, whole
  calendar months: SPY, TSLA, UUP April 2025 (stress; UUP the thinnest chain, 30 point quotes on
  2025-04-11 vs EMB 82); XLK December 2025 (2-for-1 split ex 2025-12-05, per Intrinio; XLB XLE XLU
  XLY split the same day); ETHA October 2026 (1-for-3 reverse split 2026-10-06: adjusted
  contracts, the EWZ question). Our closing quotes are Intrinio's whole-market snapshot at 15:55
  ET (D-071, D-072); for the comparison Intrinio's 15:45 snapshot is fetched to match Cboe's 15:45
  fields exactly; volume and open interest compared exactly, IV as a declared band.
  **Phase-1 tranche gate (agreed with the owner, 19:31 UTC; "the consequences of a wrong number
  are unfortunately quite large"):** each tranche passes, before the next starts, (a) refcheck in
  the worker on every day, (b) the Cboe bands, (c) EDGAR; and, once 63 sessions of history exist,
  (d) a flag-agreement test, declared and committed before phase 1 runs: when a Cboe index is in
  the top tenth of its own last 63 sessions, our matching iv30 is flagged too. A failed check
  stops phase 1: diagnose, fix, recompute the affected days. Bands are not moved after the fact.
  Phase 1 opens two days after VIX's 2025-04-08 peak (~52): the SPY band's top (8) meets real
  stress in its first tranche.
- **03:40 UTC: round x's other twelve fixed and merged; no audit (owner).** Core main 58cf628 (#1 the
  archive outcome checks its place; #2 cancelling a repair opens NOT_DERIVED; #7 digest tally in one
  statement; #8 a "not packed" beside KEPT is not listed; #9 a block archives only days it marked
  done; #12 a 5xx from Discord is an unknown outcome; #17 migration 0024, one record per report;
  #19 one `wait=true`; 24 unit + 74 DB tests). Engine `pass-s3i`/`worker` 9bc02f2d (#3 finish.log
  after the deadline loop; #10 packing diagnostics redacted; #14 leftovers removed; #15 a failed
  child's group killed; #16 D-360's bound is 15 min 45 s; 1,388 passed, 0 failed). Stated limits:
  a day republished by hand after its block marked it done is still credited to that block; history
  problems round v resolved falsely stay closed (none known); a stalled worker disk after the
  deadline can still block cleanup. Images pushed (03:05 UTC): `vyreon-worker:9bc02f2d` (build
  `9bc02f2d+90a50956a51d`, inventory `cluster-scratch/inventory-9bc02f2d.json`; register and admit
  at phase 1's start) and `vyreon-core:01ca9d7` (deploy after the sample).
- **Sample: 2026-11-02 and 2026-11-09 published (02:20, 02:38 UTC; 237 and 258 min with the
  window download); 2026-11-03 published 06:08 UTC in 226 min (2,688 exported, 0 failed).** Later
  days are not much faster than the first. 11:27 UTC: 6 of 10 published (11-10 252 min, 11-04 282,
  11-11 272). Block 2's stages per day (11-02 / 03 / 04, min): input preparation 65 / 61 / 75,
  intent A 78 / 77 / 85, intent B 68 / 64 / 89, export index 3 / 3 / 10, FIELD 13 / 13 / 14, the
  rest about 5. The next day's download (47–52 min) overlaps compute, so compute is the bound.
  11-04's longer intent B is the main difference (hypothesis: the session after the 11-03
  midterm election, a busier tape; not checked). 15:14 UTC: 11-05 published in 262 min; 15:34
  UTC: 11-12 in 250 min; 19:34 UTC: 11-06 in 259 min; **19:58 UTC: 11-13 in 263 min: SAMPLE
  COMPLETE, 10 of 10 published, every day 2,688 exported and 0 failed; both nodes deleted (19:36,
  19:58).** External checks over all ten days (20:05 UTC): Cboe PASS (gaps unchanged, level
  correlations 0.993-0.997; `checks/2026-11-18-cboe-sample-10days.json`); EDGAR the same 26 FAIL,
  BABA and TSM only, 1,944 PASS (`checks/2026-11-18-edgar-sample-10days.json`). Pass criteria:
  (1) yes; (2) yes; (3) coverage count not yet run; (4) refcheck impossible on the sample (raws
  deleted): moves to the first tranche's gate; (5) domestic filers exact, 20-F ROA defect to fix;
  (6) plots not yet made; (7) image diff with the next image.
  **Coverage count (20:05 UTC; `checks/2026-11-18-coverage-sample.txt`, every state-bearing element):**
  below 80%: PCR-UACT-I01 0.43, D111-TEMP01 0.71 and a DLM-VWAP role (381 of 640 asset-days)
  all SOURCE_VOLUME_RECONCILIATION_FAILED; SAI-EXQ01 0.35, M0-PRINT-SIZE 0.20, SAI-EXQ01-TEMPORAL
  0.64 all EVENT_SOURCE_NOT_RETAINED (512 of 640 asset-days; the archive is off?); INT-ACT-TV15
  0.64 INSUFFICIENT_ELIGIBLE; CDD-FND01 0.785 and SAI-G01 mostly structural NOT_APPLICABLE. Being
  diagnosed. **Started 20:00 UTC (owner: "start working on those fixes"):** engine `pass-fx40`
  (20-F ROA currency D-361.., adjusted-root volume, GOOGL shares); refcheck in the worker
  (engine `pass-fx41` D-370, core `core-refcheck` C-054); free gate checks (Cboe 14 pairs, OCC
  volume, FRED) in `tools/external`. Owner bought DataShop ($300: SPY TSLA UUP Apr 2025, XLK Dec
  2025, ETHA Oct 2026; per-day files; SFTP login verified, host key pinned in ~/.ssh/known_hosts
  (RSA SHA256:4yK/0LgwT2ERct0jZXdP6590zT9Qdhv78xfjWdv91lQ); files not yet written, polled every
  10 min; 30-day retention). **20:16 UTC: core `01ca9d7` deployed (owner: "youre good to
  deploy what u need")**: migrations to 0024 by the init container; first without the webhook,
  1 unsent post marked sent, then with it: notices live, 0 posted at switch-on, no open problem;
  digest 🟡 (1 provider gap 11-13). Perishable archive still off (switched on with the first
  real-day run). Owner setting up Tiingo.
  **Tiingo Commercial key works** (`tiingo.txt`, gitignored, mode 600; SPY 2025-04-08..10 read).
  **Coverage misses diagnosed (20:15 UTC; scripts `scratchpad/vol/`):** (1) SOURCE_VOLUME_RECONCILIATION_FAILED
  is a PROVIDER CHANGE: from 2026-11-06 Intrinio's replay files grew ~2.4x (CTA B 28.8 -> 76 GB)
  and carry 1-10% fewer trades in every 15-minute bin (TIP 11-12: 9,294 prints vs the trades
  API's 9,742, which matches official volume to 0.01%); 11-02..05 fail 3-9 of 64, 11-06..13 56-63
  of 64. The D-089 2% gate is right. Fix (owner's call, series basis): days within the trades
  API's 7-day reach take their trade stream from the API, the replay stays the quote source; raise
  the 11-06 change with Intrinio. Phase-1 dates before 11-06 unaffected. (2)
  EVENT_SOURCE_NOT_RETAINED is the 5-session retention window (`fill::prints_retained`,
  D-103/D-263): the sample ran 11-18, so only 11-12/13 qualified; the archive is upload-only and
  not the cause. The droplet holds the cohort prints from 2026-10-23: a read path (D-263, C-052)
  recovers those days; phase 1 before 10-23 stays withheld for SAI-EXQ01, M0-PRINT-SIZE.
  **Correction: criterion (2) is NOT clean:** EWJ VS-O02 (open interest) ENGINE_FAULT on 11-02 and
  11-03, with PARENT_ENGINE_FAULT in VS-O03, O04, OIJ01, SAI-G01 and FIELD-OPTBR01/SUP01 (one
  object each). **Diagnosed:** two code defects. (A) `h3-o02 history.rs`
  `select_minimal_source_for_asset` never treats a full 200-session window as exhausted, so EWJ
  (open interest withheld PROVIDER_OI_INCOMPLETE ~99 sessions Jan-Jun 2026; 95 eligible changes of
  126) faults instead of publishing; (B) `acq.rs` `before_first_chain` is vacuously true at the
  window's oldest session, so from 11-04 EWJ got a false CAPTURED_HISTORY_START terminal (O04,
  OIJ01 state a false history start). Fixing in engine `pass-fx42` (D-364..), surface check of
  O04, OIJ01, L01. INT-ACT-TV15: structural (a bin pair with zero cohort trades on thin names;
  common support complete); a clearer reason code would help.
  **Owner (20:40 UTC): daily runs take trades from the trades API, quotes from the replay**
  ("especially after launch of the core... some more volatile component is fine"; the API's
  7-day prints kept like the other perishables). Engine D-089 amendment queued after `pass-fx40`;
  request cost measured on 2026-11-17 (pages of 10,000, sequential within a security): NVDA
  4,970,460 trades, 498 requests, 1.04 GB JSON, 25 min; SPY 954,761, 96, 196 MB, 6 min; TIP 13,026,
  2, 2 MB, 3 s. All 64: likely a few thousand requests a day (vs ~39k per day now), several GB of
  JSON, NVDA the long pole unless split by time window.
  **Engine `pass-fx40` done (pushed, not merged; based on 9bc02f2d):** D-361: BABA and TSM send no
  local-currency facts (each amount is Intrinio's USD translation at that period's rate), so every
  quantity combining periods (ROA, yoy differences and their percentiles, derived quarters,
  trailing sums) is withheld REPRESENTATION_INCOMPATIBLE / CURRENCY_TRANSLATED_AT_DIFFERENT_PERIOD_RATES
  and translated amounts are labelled; D-362: true adjusted roots are counted (BABA2), FLEX never
  reaches the engine (Intrinio lists no FLEX tickers), ETHA's fractional volume is D-079's
  standard-equivalent count (owner's call: whole contracts recommended); D-363: GOOGL's share
  count (a zero-share class on the 2016 cover became required) now 12,294,000,000 (2026-10-21); an
  older cover never stands in. Suite 1,390 passed (shared-target interference: one crate re-run
  alone). `pass-fx43` started (D-380..: CPI/INDPRO release table, FEDFUNDS clock, earnings
  evidence, date-only midnight stamps).
  **Tiingo checks PASS (rules 0663e30 first; tool f96d2ad; reports `checks/2026-11-18-tiingo-{field,prices}-sample.json`):**
  FIELD recompute from a second vendor's closes, 165,030 comparisons over 88 quantities, all inside
  (SYNC01 W21/W63 mean correlation within 4e-7; RMT01 leading eigenvalue within 3e-5 relative;
  DEP01 20,160 pairs within 8.5e-4); prices 86,656 comparisons over 108 quantities, all inside;
  Intrinio's raw closes equal Tiingo's on all 4,608 security-sessions. Skipped: random-number
  quantities (RMT01 nulls, GRP01 permutations, SUB01 bootstrap), intraday and option FIELD
  sensors, PATH-L03-L07, VS-P07, CANDLESTICK, VWAP.
  **Owner decision (ETHA / adjusted contracts):** volume, put/call and per-contract volume against
  open interest count WHOLE contracts (as OCC and brokers); exposure-type and OI-weighted
  quantities stay share-equivalent (D-079); "be explicit with where each is used": each role
  states its unit, each row its deliverable factor, and engine D-390 tables every sensor and role.
  Being implemented on `pass-fx40`. FLEX: Intrinio delivers none, so volume is listed standard and
  adjusted contracts, FLEX excluded, stated in the method.
  **Core `core-refcheck` merged (C-054, migration 0025: `core.refcheck_report`, `PUT
  /worker/v1/refcheck/{session}`, `vyreon-core refcheck list|show`, REFCHECK_DISAGREES /
  REFCHECK_NOT_RUN notices); core suite 102 passed incl. database tests.** Engine side `pass-fx41`
  (D-370: worker runs refcheck after archive, before prune; 120-min bound; venv QuantLib 1.43,
  TA-Lib 0.8.2 pinned, build fails on mismatch) merges with the other engine branches. Set
  `VYREON_CORE_REFCHECK_FROM` to phase 1's first session. Never yet run on a real day.
  Owner on its cost: if the real day shows it is large, the fallback is the full check daily on
  the most volatile families and the whole suite weekly ("quite practical"); scaling out with more
  nodes is also acceptable.
  **Owner (21:45 UTC): no time cap on the first real day's reference check, so it is measured**
  ("we capped a time and just kept running into repeated time caps"; expects it under the engine's
  own compute time): the real day runs with `VYREON_REFCHECK_MINUTES=600`; the production bound is
  then set from the measurement.
  **Engine `pass-fx43` done (tip 58f39421, pushed; suite 1,396 passed):** D-380 CPI/INDPRO release
  table from April 2022 (82 dates from BLS and the Fed's G.17; all 107 checked against ALFRED);
  D-381 FEDFUNDS readable the session after the Fed's first business day (54 of 54 months
  confirmed; no phase-1 session was read early; the old lag would have on 2027-01-04); D-382
  `earnings_disclosed_at` is release evidence (recovers NVDA 2025-02-26, AVGO 2025-03-06; AMD, NVDA,
  PLTR's missing 8-Ks: latest withheld, history POSSIBLY_INCOMPLETE); D-383 date-only acceptance
  (`sec_filing.accepted_utc_date`). Phase-1 changes: CPI 23 sessions, INDPRO 14, FEDFUNDS 19 (and
  DBA, UUP surfaces on those dates), EVENTS01 NVDA, AVGO, AMD, PLTR. **Dated reminder: the release
  table ends at November 2027 data; extend it before then.** Core follow-up: EDGAR checker compares
  `accepted_utc_date`.
  **Optimization pass before phase 1 (owner, 21:50 UTC): timeboxed, from the real day's profile;
  byte-identity retired** ("Byte identical is based on the assumption that the current system is
  perfect. We are repeatedly proving it is not"). Image-diff rule for any change: floating-point
  noise (relative ~1e-12) allowed and reported; anything larger ties to a decision; states never
  change silently; any unusual flag that changes is explained; the external checks are the second
  net. Optimizations and value fixes land as separate commits so each changed value is
  attributable.
  **Engine `pass-fx42` done (49d28013, e918525c, 89e45be1; suite 1,390 passed):** D-364 a history
  holding the whole 200-session window counts as exhausted (VS-O02, O04, L01, OIJ01; the
  percentile ranks the eligible changes it holds, from 63); D-365 a history start needs a published
  empty chain before it (also `occ::first_option_session_after`). EWJ 11-02/03 publish; 11-04..13
  lose the false terminal. **EWJ's withheld open interest explained:** OCC cut every EWJ strike by
  $0.16 on 2025-12-16; the provider dates it only for 21 sessions, then its stale pre-adjustment
  listings trip the 1% gate; fixed by a core ledger entry. **Core never built the adjustment
  ledger the engine reads** (`h5-universe/adjustments.json`, VYREON-ADJUSTMENTS@1): `core-ledger`
  started (C-055; entries backed by OCC Information Memos for all 64 since 2022-05-27).
  **Engine `pass-fx40` D-390 done (f781f10b, 47e50889, 8a1f895a):** ETHA 11-12 34,199 contracts, put
  share 0.513; rows carry `deliverable_factor` and `open_interest_contracts`; schema
  CI-OPTION-EOD-ARROW@3 (every option source requoted); VS-O06 volume `contracts_traded`; DLM-
  OPTIONS-VOLUME 1.4.0, DLM-UNUSUAL-CONTRACTS 2.1.0; the unit table in D-390. Before merge: requote
  the shared `workspace/fixtures/etha-first-chain`. Shared-target interference made every
  agent's full suite unclean: the merged tip gets one clean full run.
  **Engine `pass-fx44` done (55d354aa, 3e1bd89e, c634170e):** D-391 quotes must respect
  no-arbitrage bounds at their own stated strike (Hull ch. 11, Merton 1973), S anywhere in the
  session's [low, high] (`underlying_range=`), tolerance 1% of the high; above 5% of a session's
  quotes outside, a strike-label fault (`PROVIDER_STRIKE_LABELS`) clears the session's quotes, OI,
  volume. Verified on DataShop: XLK 2025-12-05 all 2,200 quoted rows mislabelled (2,196 equal Cboe
  at 2x strike), caught; 0 false positives on 424,814 quotes over 106 other security-days. D-392:
  a relisted code's positive first-session OI is unknown (all 120 ETHA 10-07/08 caught; Cboe shows
  0 OI on all 15,798 first-session listings). P-13: no leftover keeps OI or volume after the
  overlay; every published OI equals Cboe's on 414,902 contract-days. P-15: no staleness rule
  (610 of 670 old quotes equal Cboe). Follow-up: ACT01 picks its cohort by raw strike, guarded only
  by `ex_date=`. **All five engine branches done; merging into `pass-fx45` (one clean suite).**
  **Core `core-ledger` merged (C-055; suite 105 passed incl. database):** the OCC adjustment ledger
  (`crates/core-api/data/occ-adjustments.json`, compiled into core, narrowed to each claim's
  members) with 21 entries, every one citing its OCC Information Memo: 13 forward splits (AMZN,
  GOOGL, TSLA, SMH, WMT, NVDA, AVGO, MSTR; XLB XLE XLK XLU XLY 2025-12-05), 4 strike offsets (FXI
  0.15 2023-06-08 #52545, QQQ 0.22 2023-12-27 #53847, EMB 0.25 2024-12-30 #55816, EWJ 0.16 2025-12-16
  #57919), 4 adjusted roots (UNG1, BABA1, BABA2, ETHA1 #59890). A new entry needs a new core image.
  Follow-ups: the engine's QQQ 174.78 `residue_strikes` example is OCC #53847; FXI's date (cash
  06-07, strikes 06-08) for the engine to confirm; record the ledger's hash with each published
  day (provenance). Pre-2022-05-27 adjustments are outside phase 1's 39-month relevance.
  **Engine merged: `pass-s3i` and `worker` fast-forwarded to e329b3c8** (`pass-fx45`: fx42, fx43,
  fx41, fx40, fx44; overlay rules now D-394, so every option source is requoted; refcheck's
  worker-mode decision list gained D-382/383; the ETHA fixture requoted, original kept as
  `etha-first-chain.pre-d390`). Clean full suite with no other build running: 1,413 passed, 0
  failed; reference pytest 34 passed. **Images pushed:** `vyreon-worker:e329b3c8` (build
  `e329b3c8+0368a1d7da4c`, venv QuantLib 1.43 / TA-Lib 0.8.2 verified in `versions.json`; inventory
  `cluster-scratch/inventory-e329b3c8.json`, 4,679 slots) and `vyreon-core:613b336` (ledger C-055,
  refcheck reports C-054). Core 613b336 deployed with the **perishable archive to Spaces ON**; build
  registered and admitted (`evidence-e329b3c8.json`). Kubeconfig had expired: refreshed (7 days).
  **23:07 UTC: real day 2026-11-03 started** (job 1382, Job `v-real-1103`,
  `VYREON_REFCHECK_MINUTES=600`). **nyc1 has no capacity for s-8vcpu-16gb-amd** (pool resize
  refused twice): a temporary pool `day-std` (s-8vcpu-16gb, Intel, same label and taint) runs it,
  so its timings are on a different CPU. `delete-idle.sh` now covers every `day*` pool. **Phase-1
  sizing must account for capacity** (8 nodes of one size may not be available in nyc1).
  **DataShop check (rules 39208e3 first; tool f6fd4da; reports `checks/2026-11-18-datashop-*.json`)
  FAILS on quotes, volume and open interest; faults cluster on corporate actions:** open interest
  100% on SPY/TSLA/UUP (338,784 contracts), same-day dating confirmed; quotes at 15:45 equal on
  quiet contracts except ~0.3% stale far-OTM; volume 96.8-99.4% of traded contracts (Intrinio drops
  prints; daily totals 0.1-1% low, worst -3.4%). **XLK split day 2025-12-05: the snapshot labels new
  contracts at half their strike (0 of 196 agree)**; leftover pre-split contracts inflate EOD volume
  14.8%; ETHA's new contracts carry copied open interest after the reverse split. Registered as
  P-11..P-15 in `docs/PROVIDER_FAULTS.md`. Engine fix needed before phase 1 reaches 2025-12-05:
  quotes that violate no-arbitrage bounds for their stated strike are rejected (a general rule,
  not a date rule), plus the post-split OI and leftover-contract handling. Not tested: busy
  contracts' quotes (they moved after 15:45).
  **DataShop order backed up to Spaces (owner):** `vyreon-data/reference/cboe-datashop/order_000107661/`,
  66 objects (65 zips + SHA256SUMS, each with x-amz-meta-sha256), private (anonymous read 403),
  read-back identical. Laptop copy kept. Owner: a field job may deploy 2026-11-21 (tentative).
  **Owner (21:10 UTC): after the new image's real day and BEFORE phase 1: hand off to a fresh
  session (this one is 3 days old), with all documentation updated, and remind the owner to fill
  in the time sheet and engineering log.** Reporter is its own side quest, started during phase 1's
  compute.
  **DataShop delivered and downloaded (21:15 UTC):** order_000107661, 65 day files (SPY TSLA UUP
  2025-04, 21; XLK 2025-12, 22; ETHA 2026-10, 22), 20 MB, every zip valid, sha256 in
  `~/claude-artifacts/datashop/order_000107661/SHA256SUMS`. Fields per contract: bid/ask and sizes
  at 15:45 and EOD, underlying at both, Cboe IV and Greeks at 15:45, volume, VWAP, open interest,
  `delivery_code` (adjusted contracts). SFTP copy removed by Cboe after 30 days.
  **Free gate checks run (f392ff9; bands 480066e, 7fff827; reports `checks/2026-11-18-{cboe14,occ,fred,edgar-events}-sample.json`):**
  Cboe 14 pairs: all 140 sessions inside band 1; correlation FAILS VXSLV/SLV 0.887, VXFXI/FXI 0.824
  (single-day divergences, cause open). OCC standard roots: 522 of 576 inside, 54 fail (TLT 11-11
  -34%, put share +0.17; XLE 11-03 -11%; NVDA 11-05 -9.5%; BABA 11-12 +16%); ETHA volumes
  fractional. **`2EWZ`/`4SPY` are FLEX, not adjusted roots** (8.27M contracts; true adjusted roots
  0.09M): whether volume includes FLEX is the owner's definition; `pass-fx40` redirected. FRED:
  exact; 2025-04-11 shows February CPI though March was out 04-10 (check). EDGAR 8-K: 765 times
  exact; earnings 8-Ks missing (AMD 2026-11-03, NVDA 2026-08-26, PLTR 2026-08-03); periodic
  `accepted_at` at midnight UTC for 12 of 17 companies, possible one-session look-ahead for filings
  after 17:30 New York (AMD AMZN AVGO PLTR). All under diagnosis. OCC terms: owner reads in a browser.
  20:45 UTC: Cboe permission email SENT by the owner (internal validation use of the 14 indices;
  whether the methodology page may name them). Owner reading OCC's terms. DataShop order still
  being prepared by Cboe; OCC's terms read by the owner: fine for our use. Tiingo checks started (FIELD correlation recompute, prices; tolerances
  committed first).
  **20:50 UTC: gate-check failures diagnosed. NO look-ahead in periodic filings** (31 cases: the
  provider's midnight stamp is the UTC acceptance date; `known_by_close` D-077 and
  `filed_after_close` D-331 admit it only when its date is before T; FND clock 170/170). Engine
  fixes queued (`pass-fx43`, D-380..): (a) CPI and INDPRO release table (`h3-macro`
  `observed_release`) starts at September 2025: earlier months fall back to the 16-day lag, so
  phase 1's CPI is up to 5 sessions stale; extend to 2022-05-27 (D-034(4), D-081). (b) FEDFUNDS +3
  lag admits a value released after the close when the month's first business day is the 4th
  (e.g. 2027-01-04, inside the paid pilot): readable from the session after the first Fed
  business day. (c) Earnings releases: Intrinio omits some 8-Ks (AMD 11-03, NVDA 08-26, PLTR
  08-03); the periodic rows' `earnings_disclosed_at` (exact to EDGAR) is never read: use it as
  evidence; else state the history possibly incomplete (D-073, D-126, D-288). (d) Publish a
  midnight `accepted_at` as a date, not an instant. Provider/reference side: OCC mismatches are
  Intrinio's EOD volume missing large prints (our volume equals Intrinio's exactly; TLT 11-11
  calls 166,717 short, with OCC open interest falling 104k in two series that show ~1k volume;
  NVDA 11-05 puts 495,279 short) or over (BABA deep-ITM single prints); ETHA's fractional volume
  explains 9 of its 10 fails. SLV/FXI correlation: the thin indices' intraday range (2.4-2.9 pts)
  exceeds their day-to-day moves; our surfaces are dense; no engine fix. Sample ends near 20:00–21:00 UTC.
  **Owner (02:45 UTC Nov 19): in phase 1, SLV/FXI correlation is judged on 63-session panels only**
  (declared before phase-1 data, tools/external README a38f03f; threshold 0.9 and band 1 per
  tranche unchanged; the sample's failures stay failures). **Engine D-396 built (pass-fx47, not
  merged: 11fd1a60 code, 1a92b508 tests, a842e5f0 docs; 1,426 tests pass):** option EOD from the
  bulk day files behind `VYREON_OPTION_EOD_SOURCE=api|bulk` (default api until the A/B), new
  source id `intrinio_bulk_options_prices_eod`, page identity binds the file's sha256 and the
  chain's rows, API fallback per asset and date; 64 chains of a day captured in 6.5 s on the
  laptop. **Finding: older dates' provider IV/Greeks differ between file and API** (P-16; prices
  and contracts equal); they feed V02, G01 and M0 native profiles. Daily bars from bulk not built
  (plan in D-396). Snapshot/requote stage of the real day runs one session at a time (~32 s each,
  ~25% of one CPU): an optimization-pass target. Phase-1 spend: owner will approve,
  wants the totals first. Startup with BLACKFRIDAY: owner starts it before Nov 27 (code's end
  date unstated).
  **Owner (05:50 UTC Nov 19): history takes the provider analytics from the bulk files (first
  computation), not the API's recomputation, even where a recomputation looks closer to a
  reference: guards against silent bias.** Re-issued files (stamped well after T) are later
  computations too: their stamps are recorded and counted. **Owner: engine-computed per-contract
  IV/Greeks is needed before phase 1** (those values go into the phase-1 database); scoping running
  (consumers of provider analytics; what the engine already computes). Measuring file vs API
  against Cboe DataShop's 15:45 IV/Greeks (2025-04 SPY/TSLA/UUP, 2025-12 XLK) as a quality check.
  **File stamps (S3 Last-Modified of all 1,448 day files, `~/claude-artifacts/checks/2026-11-19-
  bulk-eod-stamps.json`):** 2021–2023 all rewritten later (bulk rewrites 2024-03-03..09,
  2025-11-21, 2026-08-17). Phase-1 range (2025-04-10 on): 373 of 427 files stamped 1–3 days after
  T; 54 later: 2025-11-01..26 (rewritten 2026-01-05..07), 2025-11-28..12-30 (2026-08-17),
  2026-04-17 (2026-04-23), 2026-08-20..09-28 (5 files, 2026-10-21). A stamp shows the last write,
  not whether the analytics changed. 2025-08-01's file is stamped 08-04 yet differs from the API:
  the API side recomputed. **Against Cboe's 15:45 IV the API's recomputation is closer** where the
  two differ (P-16; a different underlying level in the first computation; both far worse than
  Cboe on call–put agreement). New faults P-17 (2025-12-31 file without closing quotes) and P-18
  (files carry the previous session's expired contracts): pass-fx47 handles neither yet.
  **Engine inventory:** the engine already computes per-contract American IV/Greeks from the 15:55
  snapshot (h3-iv-surface); provider analytics remain only in Q01/Q05 (delivery support), V02 and
  SAI-G01 (internal), M0 IV profile/change native channel and FIELD-OPTBR01's 2 IV mappings
  (served), FIELD-SUP01 (support). **Owner (~05:55 UTC Nov 19): readers get only the engine's own
  IV/Greeks** — the native channel leaves the served set (kept stored as a provider check;
  revises D-101's dual representation), FIELD-OPTBR01's two IV mappings move to the own IV; the
  provider checks stay internal, fed from the files. Before phase 1.
  **In flight at 06:30 UTC Nov 19:**
  real day 2026-11-03 (Job `v-real-1103`, core job 1382, node day-std-3a0sym, image e329b3c8)
  in compute since 06:01 UTC; inputs took 6 h 51 m (API history 3 h 05 m at 4 in flight, snapshots
  ~2 h 15 m sequential, requote 50 m, ACT01 24 m, rest 10 m); then the reference check, then
  publish. When it ends: `cluster-scratch/delete-idle.sh`, then refcheck/problems/diff/profile.
  Engine agent working: PART A on pass-fx47 (P-17 empty-field and missing-row API fallback, P-18
  expired rows dropped, D-396 amendment), PART B on new pass-fx48 off fx47 (readers on own
  IV/Greeks, stale descriptions, new decision). Next: review, merge to `worker`, Codex audit, one
  image (D-395 + D-396 + fx48), A/B 2026-11-03 and 2025-04-11 api vs bulk without publishing over
  core's day, optimization pass, phase-1 totals, hand-off.
  **07:20 UTC: engine merged, `worker` e738b4a5** (fx47: D-396 + amendment, 1,430 passed; fx48:
  D-397, 1,432 passed, reference pytest 34): bulk expired rows dropped (P-18), a chain with a read
  field empty on every row or no listed row goes to the API (DEGENERATE, P-17; also 218 delisted
  quoteless chains such as PWSC, one request each), `bulk-fallback.json` recorded; contracts the
  snapshot shows active that a bulk chain lacks are only measured (`bulk_snapshot_absent` at the
  requote: acting needs a re-capture design). D-397: M0 INTRINIO_NATIVE groups private, FIELD-OPTBR01
  on own IV, rule 18. Open from the agent: overlay 12's spec table (byte-locked) not edited, D-397
  holds the text; native groups still in the raw stream routes (`/stream/events`, as SAI-G01).
  Codex audits started (`audits/2026-11-19-bulk-round-y/`: bulk, ownanalytics) on e738b4a5.
  Image `vyreon-worker:e738b4a5` built and pushed (QuantLib 1.43, TA-Lib 0.8.2). **A/B plan: fills
  only, in a pod `ab-bulk` (label app=vyreon-worker so delete-idle spares its node) on a second
  day-std node (AMD refused again: no capacity); scripts `cluster-scratch/ab/`; nothing goes to
  core.** First bulk fill found a defect: every other day file failed "error sending request"
  (the file client reused a connection the host closes after ~20 s idle; a capture takes ~25 s),
  so that day's 64 chains went to the API. Fixed: `worker` 8de870cf (no idle connection for files,
  resend up to 3 tries; D-396 amended); image `8de870cf` built and pushed; A/B restarted on it (07:30 UTC).
  **Codex audits back (07:40 UTC; reports `audits/2026-11-19-bulk-round-y/`): D-395 is broken at its
  default.** The transport refuses more than 16 connections (`with_connection_limit`: 17..128 →
  InvalidCredential) and every grant reserves MAX_PAGE_BYTES × worker_count against the batch's
  byte budget (1 GiB daily and recapture batches: at 64 every later grant is denied), so at 64 in
  flight daily bars and every API fallback fail (operator confirmed both by reading). The real day
  (image e329b3c8) predates D-395. A/B continues at 16 in flight until fixed. Other bulk findings:
  STRIKE in thousandths on the 2025-12-31 file unchecked against the OSI code, quoted fields,
  duplicate headers, undo errors, D-264 filter skipped, signed URL as Referer on a redirect, and
  more; own-analytics findings: Q01/Q05 provider-analytic counts still exported as support, core
  `read` routes serve full bodies, V03 own-IV change still carries a provider-method
  qualification, FIELD-OPTBR01 method identity unchanged, Reporter copy.
  **Triage (`.../triage/`): bulk 14 real (3 wrong-number, latent), 2 not real, 1 by decision, 1
  uncertain; own analytics 9 real (0 wrong-number; 3 stored labels: V03 own-IV change carries a
  false METHOD_CONTINUITY_UNKNOWN, VS-Q05 provider counts exported to readers, FIELD-OPTBR01 keeps
  its old method label), 1 not real, 2 by decision.** Blocking phase 1: D-395 (transport cap 16,
  byte reservation × worker_count), STRIKE unchecked against the OSI code (P-19: the 2025-12-31
  file states strikes in thousandths), the three stored labels. Fix tracks started 08:00 UTC:
  A `pass-fx50` (bulk + D-395, D-398/399), B `pass-fx51` (own analytics, D-400/401). Open, not
  engine: Reporter's FIELD panel still selects `provider_iv_median.*` and M0 copy says provider
  figures (before issue 1, 2027-01-10); core `read` routes serve full bodies (by decision while
  only Reporter and the owner hold keys).
  **08:30 UTC: fixes merged, `worker` f634f7c7 (pushed); full suite 1,442 passed, 0 failed.** Track A
  (pass-fx50; D-395/D-396 amended): idle pool ≤16 separate from concurrency (1..=128), byte
  reservation per request in flight (+ wait, not deny), OSI disagreement → degenerate chain (P-19),
  quoted/duplicate-header files refused, Last-Modified/ETag recorded, every whole-file failure
  recorded per chain, interrupted bulk attempts undone and re-planned, 403 relists once, per-date
  errors kept to their date, no Referer/redirect on the bulk client, D-264 filter before bulk, O01
  checks the binding's operation. Track B (pass-fx51; D-400, D-397/D-196 amended): V03-MATCHED's
  own-IV change carries no METHOD_CONTINUITY_UNKNOWN (method `…TWO-ENDPOINT@3-D-400`), V03-CURRENT
  no longer reads Q05 (one task fewer per asset), VS-Q05 internal, FIELD-OPTBR01 relabelled
  (`…overlay-12/D-397`, `H5-FIELD-OPTIONS@2`), old provider roles private on reprojection, M0
  serves only `VYR_*` representations, `try_exists` for FIELD members, independent own-IV
  reference. Open: refreeze the retained-* V03 fixtures and re-pin `golden_day.DUMP` from a run of
  this build (two h3-asset tests ignored meanwhile); INT-IVSC01's internal
  `comparison_qualification` still says METHOD_CONTINUITY_UNKNOWN; Reporter's VS-Q05 entry needs
  the internal stub. Repeat audit (round z) started; image rebuilding. Laptop: merged worktrees
  fx46..fx49, fx51 removed, target-s3i (156 GB) and target-fx51 deleted (target-fx50 kept for round z).
  **Round z (repeat audit of f634f7c7; triage `audits/2026-11-19-bulk-round-z/triage/`):** main round-y
  fixes confirmed (64 in flight, P-19 strikes). Bulk 10 real (1 wrong-number, latent: a row with an
  empty or wrong SYMBOL drops out unseen), ownanalytics 8 real. Blocking: bulk 1; bulk 4 (a provider
  404 is classed retryable → ENGINE_FAULT on every API path: the policy matches "HTTP 404", the
  ledger writes "HttpStatus(404)"; pre-existing); ownanalytics 1 (VS-Q01's exported counts are
  post-overlay analytic coverage, not delivery: D-118 item 4; they move with Intrinio's IV) and 3
  (V03 kernel contract still lists VS-Q05). Fix track `pass-fx52` started 09:20 UTC (D-398/399/401).
  Reporter list (before issue 1): FIELD panel `provider_iv_median.*` → `iv_median.*`, M0 copy,
  VS-Q05 entry → internal stub, Q01 analytic rows and "delivered" sentence, decode V03-MATCHED's
  `support_qualification`. Before phase 1: check core holds no PUBLISHED day_revision from a
  pre-D-397 build that phase 1 will not supersede.
  **A/B (fixed image f634f7c7, pod ab-bulk3):** 2026-11-03 options from the API at 64 in flight
  started 08:35 UTC (SPY ~4 min vs 12 at 4 in flight; whole fill ~1 h vs 3 h 05 m); the earlier
  bulk run on 8de870cf processed ~22 s per day file (171 of ~225 days in 64 min; 2025-12-31's 64
  chains correctly DEGENERATE → API).
  **10:00 UTC: round-z fixes merged, `worker` 3b76925f (pushed); full suite 1,449 passed, 0 failed;
  reference 42 passed.** Q01's reader view drops the five analytic rows and two masks (retitled
  "Option-population support on the closing-snapshot basis"); V03 kernel contract parents P01,
  Q01–Q04; bulk rows kept by SYMBOL or code root, mismatch → degenerate; 404/401/403 classified from
  one formatter (D-262 amended); FIELD carrier must match the M0 index digest; bulk undo order,
  attempt lock, synced bodies, SHA-mismatch record, relist on 400, per-date listing failure, prune,
  exact-decimal strike compare; requote diagnostic reads through the index; M0 projection also checks
  derivation class and unit; reference robustness. Open: P01's kernel contract still names VS-Q03
  (same class as V03's; changes P01 pins, fix with the retained-fixture refreeze before phase 1);
  bulk 6 (gate deadline) by decision. **A/B API fill done: 72 min at 64 in flight, 19,890 requests
  (~276/min: not rate-bound; per-asset batches serialise), nothing missing.** Round-aa audit started;
  image `3b76925f` built and pushed.
  **Round aa (third pass on 3b76925f; triage `audits/2026-11-19-bulk-round-aa/triage/`): findings now
  mostly need a local filesystem failure, a parent-only kill or a revoked key.** One blocks phase 1:
  a withheld VS-Q01 has no reader view (projection requires the count matrix; pre-existing; every
  contained asset-day and EMPTY_DELIVERED_POPULATION, e.g. QQQ 2026-09-29), and its headline scalars
  vanish. Conditional: one 404 in an asset batch masks another date's failure (blocking if batch 404s
  appear in ledgers). Fix track `pass-fx53` started 11:00 UTC for every real one, plus the V03 and P01
  kernel contracts. **Codex allowance 96% of the week used (resets 2026-11-20 14:51 UTC): the fourth
  pass waits for it.**
  **11:25 UTC: round-aa fixes merged, `worker` 8e0d831c (pushed); full suite 1,456 passed, 0 failed;
  reference 44 passed.** A withheld VS-Q01 has a reader view (each role WITHHELD with the body's
  reason); each failed capture in a batch classified on its own; space-padded codes; a stuck bulk
  chain is that asset's ENGINE_FAULT (not the whole day Stale) and the fill exits 1; a refused key
  stops the fill; V03 and P01 kernel contracts describe the code (digests V03 14604cef…, P01
  97eb2b5f…); FIELD M0 members need an own derivation class and a 64-hex index digest; golden guard
  by build ancestry; small bulk provenance fixes. **Retained fixtures need a real run of this build
  (`tools/golden/fixture.py`): every P01 and V03 receipt and task key in `retained-2025-12-10`,
  `retained-2026-09-18` and `final-results/h4-m0-measurement-dev1/{spy,qqq}-inline-vs-p01`; their
  tests stay ignored until then.** Image building.
  **A/B 2026-11-03 on f634f7c7 (pod ab-bulk3):** API fill 4,332 s (72 min), 19,890 requests, nothing
  missing; bulk fill 6,253 s (104 min, ~28 s per day file, sequential), 5,255 requests (daily bars +
  66 chains to the API: 64 on 2025-12-31 P-17, QQQ 2026-09-29 no rows, SPY 2026-09-28 sizes/times
  empty), 14,270 chains from 224 files, 1,192 expired rows dropped, nothing missing. **The
  contract-by-contract comparison was lost: my comparer loaded both homes into memory, the node
  ran out and the pod was evicted with its emptyDir (11:33 UTC).** Rerun on the next image with a
  streaming comparer and a memory limit. So far: bulk saves ~14,600 option requests per day but is
  slower in wall time until the per-file capture is parallelised (optimization pass).
  **13:29 UTC: real day 2026-11-03 PUBLISHED (core revisions 51 and 52: 2,853 slots, 7,763 reader
  scalars; derived 1,105 slots).** Image e329b3c8 on the regular Intel node (day-std, s-8vcpu-16gb).
  Profile: inputs 410 min (above); in-day input preparation 100 min (06:01–07:41); intent A history
  nodes 163 min; exports 13 min; intent B (T) 122 min; export index 7 min; FIELD 35 min; derived
  3 min: **compute 7 h 28 m** (the 110–160 min of 2026-11-09 was the AMD node and an older build:
  phase-1 sizing must use measured compute on the node size we can get). Reference check started
  13:30 (deadline 600 min). A/B rerun on 8e0d831c: API fill 4,788 s (in parallel with bulk).
  **14:32 UTC: job ended, node day-std-3a0sym deleted.** Day published in 509 min of worker time
  (inputs 410 + compute ~450 on the same clock: the download log counts from 23:10). Intrinio
  requests by category: captures 25,011, company 2,747, api_json 288, carry 256, macro 86.
  **Reference check CRASHED after 59.5 min (318 CPU min, 8 jobs), report 1 in core:** the intraday
  reference reads `consolidated/<T>/capture/trades.raw`, which a worker never has (tape caches come
  from core; the raw tape is retired under D-159), and one symbol's FileNotFoundError aborted the
  whole suite. Fix track `pass-fx54` started (state unverifiable per symbol, contain per-symbol
  crashes, cost analysis). Problem open in core: "Reference check did not run on 1 published day".
  **Diff against the stored 11-03 (revision 35, build 05837898):** 0 slot-state changes; 2,210 of
  2,853 bodies changed (provenance and labels), VS-O06 method @2→@3 (D-382/383, all 64); reader
  scalars 7,691 of 7,744 identical, 8 withheld (CDD-FND01 BABA and TSM: D-361 foreign-filer
  cross-period withholding, P-07), 26 float-noise (≤1e-14), 19 changed: FIELD-OPTBR01 15 (one more
  member's OI call share supported, support 0.984→1, cross-section stats move ≤4%), VS-IVS01 DBA
  skew30 −0.02288→−0.02388 and UUP +0.2%, VS-V03-CURRENT UUP put IV −0.2%: thin chains requoted under
  D-391/D-394. All tied to merged decisions.
  **A/B 2026-11-03 result (image 8e0d831c, streaming comparer): the bulk files deliver the same
  option history as the API.** 14,336 chains each, 38,326,866 rows each, the same contracts in every
  chain; every price, size, volume and open-interest field equal. Differences: (1) quote and trade
  times are whole seconds in the file (`21:00:02Z`) where the API has milliseconds
  (`21:00:02.396Z`), on ~37.6 M rows: no engine rule compares those times below a second (EOD quotes
  are replaced by the 15:55 snapshot, D-071; occ.rs takes the max per contract); (2) provider IV and
  Greeks differ on 6.5–9.1 M rows, mostly the dates whose files were rewritten (P-16; internal only
  under D-397); (3) exercise style null in the API but `A` in the file on 498,208 rows (all 64 are
  American: same value under D-199). Requests: API 19,890, bulk 5,255 (daily bars + 66 degenerate
  chains). Time: API 72 min alone / 80 min shared; bulk 104 min alone / 148 min shared (per-file
  capture is sequential: optimization target). 2025-04-11 A/B started 14:58 UTC (same pod).
  **Merging `pass-fx54` (reference check: raw tape absent → unverifiable per role, per-symbol and
  per-family crashes contained, `incomplete` and `timings` in the report; D-370 amended).** Owner
  decision needed: verify the tape owners independently by running the reference tape reader on the
  tape worker (which still has the raw tape) and keeping its verdict with the cache, or accept them
  as unverifiable on worker days.
  **15:05 UTC: `worker` 48d87154 (fx54 merged, pushed); full suite 1,457 passed, 0 failed; image
  `vyreon-worker:48d87154` (build 48d87154+fe2ce9137923) pushed: the candidate for the next real day
  and the optimization pass.**
  **Owner (15:15 UTC): go ahead with the Reporter changes; continue with steps 3 and 4, then 5–7.**
  Reporter branch `d397-own-analytics` (worktree outside the checkout) in progress. **15:30 UTC: build
  48d87154+fe2ce9137923 registered and admitted (`cluster-scratch/evidence-48d87154.json`); day-block
  job 1383 scheduled for 2026-11-02..04 as one 3-session block; Job `v-block-1102`
  (`VYREON_OPTION_EOD_SOURCE=bulk`, 64 in flight) on an AMD node (pool `day` resized to 1: nyc1 had
  capacity this time; node day-3az1af).** Purpose: the compute profile of the current build for the
  optimization pass, the cost of the 2nd and 3rd day of a block, AMD vs Intel, and the real-day check
  of the merged batches. Fourth audit pass (round bb, combined, medium reasoning to fit the remaining
  Codex allowance; owner's suggestion) started. A/B 2025-04-11 running on day-std-3a1hrq.
  **Owner (15:40 UTC): the optimization pass runs twice** ("agents never get everything done in one;
  the audits demonstrate this"): pass 1 from the block run's profile, each change its own commit,
  measured on a real day; pass 2 on what pass 1's measurement shows is left. Node choice by a
  controlled benchmark (same image, same day files, same command, AMD and Intel at once, alone;
  then parse-only vs write to separate CPU from disk), not by the first AMD reading (~20 s per
  bulk file vs ~28 s on Intel, confounded by disk, build, dates and neighbours).
  **Reporter D-397/D-400 changes done** (branch `d397-own-analytics` from `engine-v1-remediation`
  326cb88, worktree `/home/diggz/wt/reporter-d397`, not pushed; f926b8a FIELD-OPTBR01 own IV, 3587ab1 M0
  copy and dead INTRINIO_NATIVE code, c7d6d64 VS-Q01 title/labels from the view's axes and withheld
  wording, b26f422 VS-Q05 internal stub, f9e6d7f V03-MATCHED `support_qualification`; tests: unit 194,
  contract 22, rendering 12). Reporter now refuses (fails the asset build) any coordinate in a
  provider-IV unit or a non-`VYR_*` M0 group: fail-closed; before Reporter reads core, no pre-D-397
  revision may stay PUBLISHED in a report's window (core still holds 2022-06, 2025-04-11, 2025-12-10/11
  and the 2026-11 sample from older builds; phase 1 supersedes all but 2022-06). To verify against a
  real 48d87154 revision (the block run) before merging.
  **Round bb (fourth pass, medium reasoning, on 48d87154; `audits/2026-11-19-bulk-round-bb/`): every
  round-aa engine finding FIXED or FIXED WITH A NEW DEFECT; no wrong sensor number, no
  nondeterminism. 6 findings:** two stored-label edge cases (a T 404 relabelled ENGINE_FAULT by
  another chain's retryable failure in the same batch; a whole-fill key stop overwriting recorded
  causes), a presigned-file 403 misread as a refused key (stops later categories), request cost
  after a refused key, and two reference-suite containment/labelling gaps. Fix track `pass-fx55`
  (verifies each first; no separate triage this pass). **Timings so far:** A/B 2025-04-11 API fill
  3,989 s in parallel with bulk; block run (AMD, alone) captured 123 bulk day files in 31 min (~15 s
  per file vs ~28 s on Intel alone: still to be benchmarked under control).
  **17:00 UTC: round-bb fixes merged, `worker` 04e2ed1b (pushed); full suite 1,462 passed, 0 failed;
  reference 53 passed; image `vyreon-worker:04e2ed1b` built.** Left by choice: a refused bulk
  listing falls back to the API (a 403 there may be a missing bulk entitlement; the API's first batch
  stops the fill on a bad key). **A/B 2025-04-11 fills:** API 3,989 s, 18,437 requests; bulk 6,815 s
  (shared node), 4,313 requests; both nothing missing; bulk 13,952 chains from 224 files, 165,909
  expired rows dropped, 342 chains to the API: 181 ETHA and 126 IBIT sessions before their options
  listed (no row: one wasted request each; skip by the listing start), 35 chains on 2024-05-22, a
  second bulk file without closing quotes (P-17 extended). Comparison running.
  **Owner (17:10 UTC): a test census belongs in step 4.** List every ignored or skipped test with its
  reason (105 reference tests skip for the absent golden dump; 33 engine tests ignored, many awaiting
  the retained-fixture refreeze) and every test guarding a retired decision (provider-IV reader
  shapes, old role names, the pre-bulk option path); then refreeze, run where the data is (the
  cluster), or retire each with the reason recorded. Keep the API-path tests (fallback), Reporter's
  provider-value refusal, and the internal provider-check tests.
  **A/B 2025-04-11 comparison (window back to 2024-05-21): NOT equal on older dates.** 14,294 chains
  each; 31,459,100 API rows vs 31,458,604 bulk. Besides second-precision times and analytics (as on
  11-03), real differences: (1) AMD, EMB, META 2024-07-02: the API has 62/50/112 more contracts and
  different prices, volume and OI on the same contracts (AMD 250117C155: close 28.53 vs 28.50, open
  28.45 vs 25.10, volume 6 vs 170, OI 2,941 vs 3,005): one source holds another session's data;
  (2) NVDA 2024-05-21..: two contracts per day coded $100 in the API and $10 in the file (around the
  2024-06-10 10:1 split); (3) "no bid" as null in the API and 0.00 with size 0 in the file on ~460 k
  rows (2024-06/07); (4) value differences on open interest (32,445 rows), volume, close/open/high/low
  (~8 k rows). Per-date breakdown running; which source is right is to be settled against OCC/Cboe
  before bulk is used for those dates.
  **Per-date breakdown (`tasks/…/bl9bz0u2g.output`): 80 dates differ on values.** (a) 2024-06-10 (NVDA
  split ex-date) and 2024-07-02: whole-chain differences in close/open/high/low/volume/OI/quotes, and
  on 07-02 six chains list different contracts; (b) 2024-11-04..2025-03-04: open interest differs on
  22–716 rows every session, never after 03-04; (c) 2024-05-21..06-10: one NVDA chain a day codes two
  contracts at $100 (API) vs $10 (file); (d) 22 dates from 2024-06-21: no-bid null vs 0.00 size 0.
  From 2025-03-05 to 2025-04-11 the two sources agree on every value. Investigation running (which
  source is right, by OI continuity, OCC/Cboe and the split memo; and the engine rule).
  **Investigation done (`checks/2026-11-19-bulk-vs-api-2024.json`): all 240 bulk files in the window
  are original next-morning releases; three patterns are provider faults.** 2024-07-02: the API mixes
  in 07-03 data on seven chains (P-21), the file is right. OI 18724 is a placeholder (P-20): on 23,862
  new listings in the files (API 0, right) and on 978 existing contracts in both sources, including
  AMZN 2025-04-10 (phase 1). NVDA 2024-06-10 split ex-date: both sources mis-scale strikes (P-11
  extended to EOD); before it the file's coding is right. No-bid null vs 0.00/size 0: same meaning.
  External: OCC's volume query refuses dates older than two years; no DataShop overlap. Engine track
  fx56 started (first-session OI > 0 unknown, 18724 a stated absence, post-close stamps fail a chain,
  0/size-0 is no quote, D-391 on NVDA 2024-06-10 EOD); a scan of all phase-1 files for P-19/P-20/P-21
  patterns started on the A/B node.
  **Engine track fx56 merged into `worker` c2a627ac (suite and image in progress):** D-398 (a
  contract's first-session positive OI is unknown unless the snapshot says 0; an EOD 18724 the
  snapshot does not repeat is set aside: a named value, no general rule avoided discarding good rows;
  the local real 18724 SPY260116C00700000 2025-04-08 is a true count repeated by the snapshot and
  Cboe); D-399 (a trade/bid/ask stamped on a later New York day fails the chain: bulk → API, API →
  stated absence; P-01 same-day stubs untouched); D-072 amended (EOD 0.00 with size 0 is no quote:
  19,501 rows on the reachable real data, staleness flips on 51); NVDA 2024-06-10: D-391 checks the
  snapshot's quotes at each EOD row's stated strike, so the chain is withheld either way (bulk file:
  42.6–46.6% of quotes outside bounds). Overlay rules now D-398: every option source requoted; shared
  fixture `workspace/fixtures/etha-first-chain` requoted in place (original kept as
  `etha-first-chain.pre-d398`). **Owner decision: pre-listing sessions in bulk plans** (181 ETHA +
  126 IBIT wasted requests per window): (1) core records each asset's first option session and sends
  it with the claim (recommended), (2) treat a root with no rows as the empty chain, (3) pay the
  requests. **Block run (AMD):** requote 31 min (Intel 50).
  **`worker` c2a627ac pushed; full suite 1,470 passed, 0 failed; image `vyreon-worker:c2a627ac`
  (build c2a627ac+4c19b3d5e622) pushed.** The block run continues on 48d87154 (its profile is still
  valid: D-398/399 change no ordinary-day value).
  **Phase-1 file scan done (all 427 bulk day files 2025-04-10..2026-11-17, 795.9 M rows, 0 errors;
  `/work/scan-phase1.json` on the A/B pod, summary in the task output):** strike ≠ OSI code only on
  2025-11-27, 2025-12-25, 2026-01-01 (market holidays: files exist but no session reads them) and
  2025-12-31 (P-19, already handled); 18724 on 129 rows over 110 days, a few on universe roots
  (D-398's snapshot rule handles them); rows stamped after their day only in the 2026-09-28 file
  (55,171 rows, rewritten 2026-10-21: D-399 sends those chains to the API); rows with no closing
  quotes ~30–38 k a day across the market, a stable set per root (NVDA 2,125, AVGO 988, QQQ ~700,
  MSTR 618: contracts without quotes, not whole chains; the overlay takes the snapshot);
  4.83 M expired rows (dropped, P-18). SYMBOL ≠ code root: none.
  **Step 3 (A/B) done; A/B node day-std-3a1hrq deleted (21:12 UTC).** Results saved:
  `checks/2026-11-19-ab-20250411-{compare,bydate}.json`, `checks/2026-11-19-bulk-phase1-scan.json`,
  `checks/2026-11-19-bulk-vs-api-2024.json`. Verdict: bulk equals the API on contracts and prices
  for 2025-03-05 onward; earlier dates differ where the provider is wrong (P-20, P-21, P-11 EOD),
  the file being right in two of three; the engine now handles all of them (D-398, D-399).
  **Step 4, optimization pass 1 (input stages), `pass-fx57` (D-401; D-395/D-396 amended): merged into
  `worker`, suite and image in progress.** Byte-identical outputs, proven on real frozen data per
  change: bulk day files 3 at a time (`VYREON_BULK_FILES_IN_FLIGHT`, est. 25–50 min saved per window
  on AMD); closing snapshots 8 sessions at a time, written in session order, markers derived in
  parallel (`VYREON_SNAPSHOTS_IN_FLIGHT`; est. 110–135 → 10–20 min if the file host limits per
  connection); marker rewrite in parallel; `h5_surface` longest chains first and T/T−5 surfaces at
  once (est. 10–35 min of the 73–100 min in-day preparation, which is T's input plan plus the two
  surfaces); ACT01 cohorts 8 assets at once (it never used the batch setting: est. 24 → 3–4 min).
  None measured yet (no full-size files on the laptop): the next real day on the cluster measures
  them. Not changed: several assets' API batches at once (one account writer lock per batch process;
  needs a merged multi-asset batch or a multi-writer ledger, not speed-only; matters only for API-
  source days), requote (already 8 sources at once; pass-2 candidates: the double prepare at first
  admission and requote, dropping the A/B-only `bulk_snapshot_absent` re-read, 16 workers).
  **Owner (22:45 UTC): compute should be where the cost is; optimization passes are granular and repeat
  until the expected gain no longer justifies a pass** (the bottleneck will move several times).
  Working stop rule: continue while a pass's measured saving clearly exceeds ~a day of work plus
  verification (phase 1: each 10% of compute ≈ 200 node-hours ≈ 1 day on 8 nodes; daily runs: every
  hour saved publishes every day an hour earlier). Compute passes start from a per-task profile of
  intent A/B (sensor family × asset), not stage totals. **Rough phase-1 totals given to the owner
  (estimates, to be replaced by the block run's measurements):** ~405 sessions; first day of a block
  ~6.2 h on AMD after pass 1, later days ~4.5 h, reference check ~0.7 h; ~5.3 node-h per day,
  ~2,100 node-h (1,700–2,500); 8 nodes ~11 days, 16 ~5.5, 24 ~3.7; ~$290–420 of AMD worker time
  (above the $300 cluster ceiling: owner's approval on final totals). Limits now: AMD capacity in
  nyc1 and the 30-droplet account limit, not Intrinio requests.
  **`worker` 97d0e7d8 (pass 1 merged, pushed); full suite 1,477 passed, 0 failed; image
  `vyreon-worker:97d0e7d8` (build 97d0e7d8+b50e1b7a3e6a) pushed: the candidate for the next real day
  (measures pass 1). Droplets: 4 of 30 (Telemetry-Backend web, vyreon-engine-shared-8vcpu-amd, core,
  one worker): up to 27 workers; retiring the old engine droplet after daily runs move frees a 28th.**
  **Block run day 1 (2026-11-02, AMD, image 48d87154, bulk), measured so far:** inputs 242 min (bulk
  option history ~60, snapshots ~110, requote 31, macro/company/ACT01 ~41; Intel 11-03: 410), in-day
  preparation 73 min (Intel 100), intent A 88 min (Intel 163) while day 2's download ran alongside,
  exports 4 min (Intel 13). AMD runs these stages at 0.54–0.73 of the Intel time.
  **23:54 UTC: block day 1 2026-11-02 PUBLISHED (revisions 53/54; 2,853 slots, 7,818 reader scalars;
  derived 1,105).** AMD compute after "inputs ready": intent A 88 min, exports 4, intent B (T) 72 (Intel
  122), export index 3, FIELD 13 (Intel 35), bundle + derived 3: **185 min** (Intel 348), plus in-day
  preparation 73 (Intel 100): **258 min of compute per day (Intel 448), with day 2's download running
  on the same node meanwhile.** Day 1 total from start: 501 min (8 h 21 m; inputs 242 + compute 258).
  Reference check started 23:55 (fixed suite).
  **Day 2 (2026-11-03) inputs: 75 min (19:35–20:50), entirely overlapped with day 1's compute** (one
  bulk file, one snapshot, requote of the new session, company, ACT01): a later day in a block costs
  ~its compute (~258 min AMD) plus the reference check if it runs in sequence.
  **00:26 UTC Nov 20: day 1's reference check COMPLETED (fixed suite) in 30.8 min wall, 167 CPU min
  (AMD; the crashed Intel run took 59.5 / 318): 92,767 checked, 90,785 passed, 1,982 failed (167
  classes, 26 measurements), 7,155 skipped, 9 reference errors (INT-TEMP-EVC01), 4,288 unverifiable
  (tape owners), incomplete: intraday.** Largest: VS-V02 960, SAI-ACT01-INTRADAY 256 (an ARRAY_REF
  coordinate's support counts: alternative reading), SAI-G01 142 (D-083 B band edges / overlay-
  cleared rows: alternative reading), VS-Q05 82, DLM-* ~260, CDD-MACRO01 64, SURFACE-FORWARD 59, VS-Q01
  44. Many carry the reference's own "alternative reading" annotation. Report saved
  (`checks/2026-11-20-refcheck-2026-11-02-{report.json,show.txt}`); triage agent running (engine
  defect / reference defect / alternative reading / tolerance). Day 1 total: published in 291 min
  of the worker's day clock after its download; reference check runs in sequence before day 2's
  compute.
  **Reference check triage (`checks/2026-11-20-refcheck-2026-11-02-triage.md`): 0 engine defects.**
  1,915 of 1,982 failures match conventions the owner already ruled as the definition (rulings table:
  D-083 B band edges/forward and overlay-cleared rows = MISSING for V02/Q05/Q01/G01; D-110 12 array
  support for ACT01-INTRADAY; D-106/107/110/118 for DLM-*); 53 + 9 reference errors are reference
  defects (R1 frozen post-adjustment listings for the Dec-2025 OCC-adjusted six; R2 EVC priors not on
  today's share basis across splits; R3 EVC crash on a manifest without body; R4 QuantLib LR tree +inf
  at σ floor; R5 TSM parent-equity order; R6 INDPRO base label: the engine's 2017 is right, D-110 2's
  text is wrong); 5 one-ulp OI-sum tolerance (ETHA since D-390); 9 SURFACE-FORWARD uncertain (6 short-
  dated pair scatter; EWJ 2026-12-18 forward 91.15 vs ~92.3 neighbours: possible projected-dividend
  defect, to settle before the build reaches Dec-2025 dates; UUP thin chain, D-095). Fix track
  `pass-fx58` started (R1–R6, tolerance, D-110 2 text, per-class rulings outcome and a
  `failed_unexplained` count in the worker's report, EWJ investigation). Core change to follow:
  REFCHECK_DISAGREES and the digest count only unexplained failures (otherwise the alarm stays open
  every day on ruled conventions). **Owner decision:** extend the 2026-11-04 forward-scatter ruling
  so the price test also applies where a σ half-width exists (scatter only beyond the larger).
  **01:45 UTC Nov 20: reference fixes merged, `worker` ff210b97 (R1–R6, OI-sum tolerance, D-110 2 text,
  per-class rulings outcome, `failed_unexplained` in the account; replay of the 11-02 report: 1,982
  failed, 67 unexplained, 9 after every fix) — full suite 1,478 passed; image `vyreon-worker:ff210b97`
  (build ff210b97+aee419a45e7e) pushed. Core C-056 (migration 0026: REFCHECK_DISAGREES and the
  digest count `COALESCE(failed_unexplained, failed)`; core suite 105 passed incl. database) merged
  (main 9fd433d) and deployed (`vyreon-core:9fd433d`, set image on core and migrate; rolled out).**
  The open REFCHECK_DISAGREES for 11-02 stays until a day is checked by a worker that sends the field
  (the block run is on 48d87154). EWJ 2026-12-18: the engine projects last December's whole 3.0614
  payment, including the 0.157024 OCC classed as special (memo #57919, strikes cut 0.16): D-120's
  open case; explains ~17 bp of the ~131 bp gap (the rest is weak 12-18 quotes). Same gap: FXI
  2023-06-08, QQQ 2023-12-27, EMB 2024-12-30, EWJ 2025-12-16 (only EWJ's is inside phase 1).
  **D-120 amended (engine `pass-fx59` merging; core C-055 amended, main a9acc36, deployed
  `vyreon-core:a9acc36` first as required):** the ledger states OCC's special behind each strike offset
  (EWJ 0.157024 #57919, FXI 0.148983 #52545 ex 2023-06-07, QQQ 0.21584 #53847, EMB 0.254515 #55816);
  the surface projects the ordinary part only (EWJ's December recurrence 3.0614 → 2.904376 on 11-02's
  real records; a paid row equal to the special is the whole special); `H5-SURFACE@22`. Changes on an
  ordinary day: EWJ from 2025-12-16 (forward/carry for expiries after the projected December, ~+17 bp
  on 12-18), EMB to ~2026-01-13, QQQ to 2025-12-19 only if D-149's test flips; other 60 assets bit-
  identical. Days built at @21 (the block run, 48d87154) and @22 are different method versions in
  derived histories: phase 1 starts on one image anyway.
  **Correction (02:15 UTC):** the merged fx59 tip 76169889 failed one test (h3-asset
  `each_m0_label_pins_its_method_code`: M0-ANALYTICS, M0-GAMMA, M0-GAMMA-CHANGE hash the surface
  stage's code), and my build chain pushed `worker` and image `vyreon-worker:76169889` anyway (it ran
  the push after a failed suite): **do not use image 76169889.** Fixed in e1461087 (pins updated,
  labels unchanged per D-295: the method is the same, the moved input is the surface @22) and
  d0abefa1 (D-120 note); full suite with --no-fail-fast running; the build now runs only when the
  suite exits 0.
  **Owner (02:25 UTC Nov 20): approves (1) the forward-scatter rule — the price test (half the
  bid-ask spread) applies also where a σ half-width exists, scatter only beyond the larger; (2) core
  records each asset's first option session and sends it with the claim (bulk plans skip earlier
  sessions); (4) the tape owners verified independently: the tape worker runs the reference tape
  reader while it holds the raw tape and core keeps the verdict with the cache. Phase-1 spend waits on
  the optimization passes' measured totals.**
  **03:00 UTC: `worker` d0abefa1 full suite 1,480 passed, 0 failed; image `vyreon-worker:d0abefa1`
  (build d0abefa1+473867ee3d30) pushed (D-120 specials; needs core ≥ a9acc36, deployed). Forward-
  scatter rule merged (`worker` aed8b73d, reference only, D-370 amended; the 10-30/10-31/11-04 forward
  rulings now recorded in DECISIONS; replay: SURFACE-FORWARD unexplained 9 → 7). First-option-session
  track (`pass-fx61` + core `core-first-option`) in progress.
  **C-057 merged and deployed (core main 69006a5, `vyreon-core:69006a5`; core suite 0 failed incl.
  database):** `core.option_listing` holds each asset's first option session with the provider's
  evidence (ETHA 2025-04-11, IBIT 2024-11-19; frozen answers under test-support), sent in every claim
  as `first_option_sessions`. Engine `pass-fx61` (D-396 amended) merging: a bulk chain before its
  asset's first option session whose file has no rows is the file's empty chain (no API request); not
  dropped from the plan, because history start (C-034, D-365), OCC detection and T−5 read that empty
  chain (dropping it would change ETHA/IBIT values on their first sessions). After the date, a chain
  with no rows is still degenerate and goes to the API. Saves ~307 requests per affected window; no
  published value changes.
  **`worker` 3b342af7 (fx61 merged, pushed): full suite 1,482 passed, 0 failed; reference 76 passed;
  image `vyreon-worker:3b342af7` (build 3b342af7+5d6db5661e39) pushed: current candidate.** Tape
  verification track (`pass-fx62` + core `core-tape-verdict`) started.
  **C-058 deployed (core main 996ad69, `vyreon-core:996ad69`, migration 0028; core suite 0 failed):**
  `core.tape_verdict` beside each tape cache; the tape worker (engine `pass-fx62`, D-370 amended,
  merging) runs the reference tape reader on the raw tape it holds (`refcheck tape-verdict`, est. ~1
  min per session for 64 members, not measured) and sends PASS/FAIL/WITHHELD/NOT_RUN with the reader's
  bins digest; a FAIL opens REFCHECK_DISAGREES; the day check verifies the four tape owners from cached
  bins only under a PASS for the exact files. Unproven until the first tape job of this build runs (no
  raw tape on the laptop). **Owner decision:** caches stored before this build (the tape history from
  2024-04-09) never get a verdict (raw tapes gone), so EVC01's agreeing readings stay unverifiable on
  phase-1 history unless those tapes are downloaded again.
  **`worker` e31ece49 (fx62 merged, pushed): full suite 1,485 passed, 0 failed; reference 83 passed;
  image `vyreon-worker:e31ece49` (build e31ece49+28ab1516eee1) pushed: current candidate (needs core
  ≥ 996ad69, deployed).** Recommended to the owner for historical tapes: verify a sample (~20 sessions,
  about one a month, re-downloaded; a forced-recapture option for chosen sessions).
  **Owner (04:12 UTC Nov 20): spot-check approved** — ~20 historical sessions (about one a month
  across the tape history) re-downloaded and verified; the rest stated "not verified by its own
  verdict, cache code verified by sample".
  **Compute profile (day 2, from result-file times, lower bounds; `checks/2026-11-20-compute-profile-
  day2-mtime.txt`):** intent A is the O02/O04/OIJ01/L01 history endpoints (4 × 12,619 tasks, ~200 past
  sessions × 64 assets recomputed daily; ~82 min) — the block worker never passes D-038's shared
  endpoint-history run; intent B is mostly M0 (M0-ANALYTICS 128 tasks for T and T−5, ~205 task-min,
  SPY 16.6 min per task; IV-CHANGE 46, GAMMA 31, GAMMA-CHANGE 30, IV-PROFILE 15). **Compute pass 1
  (`pass-fx64`, D-402) started:** per-task timing in the index (exact profiles), history endpoints kept
  across a block, M0 T−5 reuse and longest-first scheduling; byte-identical outputs. Spot-check
  mechanism (`pass-fx63` + core `core-tape-sample`) in progress.
  **C-059 deployed (core main e9da254, `vyreon-core:e9da254`, migration 0029; core suite 112 passed
  incl. database):** `vyreon-core tape verify <profile> <build> <session>…` schedules TAPE jobs with
  scope verify: the worker re-captures the session's tapes, recomputes each member's cache, takes
  only the fetch-time fields from the stored cache (listed in the check), compares every other byte,
  runs the reference tape reader; MATCH keeps the verdict beside the stored cache, DIFFER opens
  REFCHECK_DISAGREES (never replaces a cache), NOT_COMPARED says why; `vyreon-core tape checks` and the
  digest line report it. Stored caches: 41,784 at the current method version (`…8cbc8c46…`), 128 at an
  older one. Engine `pass-fx63` merging. Sample (20 sessions: tape history start, first session of
  every second month, NVDA 2024-06-10 and AVGO 2024-07-15 splits, 2025-12-05 ETF splits, 2026-11-06
  replay change) ~0.7–0.8 TB download, ~4,000 requests, ~6–9 min per session on a tape pod: run after
  the engine image is built. A DIFFER's resolution is the owner's call (recheck vs replace).
  **05:00 UTC: `worker` 4276d889 (fx63 merged; 1,489 passed; image `vyreon-worker:4276d889`). Compute
  pass 1 (`pass-fx64`, D-402; D-038, D-007 amended) merged into `worker` 3b85b4f6, suite and image in
  progress:** per-task start/finish/CPU times in a sidecar (`task-times/<intent>.jsonl`) and a
  per-measurement summary in acceptance.log (no published byte); one endpoint-history run per block
  (`<home>/block-history`; `VYREON_WORKER_HISTORY=off` restores per-day) — est. intent A on later
  days 83 → 25–45 min; reused endpoints keep their earlier cutoff so only provenance digests differ;
  M0 kernels start largest chain first. Not done (reasons in D-402): T−5 M0 reuse (needs ≥6-session
  blocks; newest-first build; a rule change), internal M0 parallelism (edits every M0
  method_sha256). Next lever found: history reuse costs ~22 ms per reused task, serial. **Block run:
  day 2 (11-03) published (revisions 55/56; worker day 265 min after inputs); its reference check
  completed (93,087 checked, 2,489 failed in 160 classes on the old suite; replaces the crashed 11-03
  check). Day 3 computing.**
  **05:25 UTC: `worker` 3b85b4f6 suite 1,494 passed; image `vyreon-worker:3b85b4f6` (build
  3b85b4f6+96356aa79bd5) registered and admitted (`cluster-scratch/evidence-3b85b4f6.json`).
  Scheduled: day-block job 1384 for 2026-10-28..30 (3 sessions, block history on: measures input pass
  1 + compute pass 1 with exact task times; Job `v-block-1028` on a 2nd AMD node of pool `day`); the
  repeat of the same block with `VYREON_WORKER_HISTORY=off` (C-001 block-start test) follows when it
  finishes (core refuses an overlapping unfinished block); tape sample verify jobs 1385–1404 (20
  sessions; Job `v-tape-sample`, 3 pods in parallel on a day-std Intel node).**
  **Tape sample, first 14 sessions (06:00 UTC): every recomputed cache equals the stored one byte for
  byte (892 of 892 asset-sessions, 0 differ: the cache code reproduces and Intrinio's old replay files
  are unchanged); ~3–5 min per session (4–45 GB of replay each). The reference reader's verdict
  FAILs on 36 asset-sessions, all on VWAP fields only (63 bin differences, 49 vwap_all + 14 vwap_ps,
  worst relative 5.9e-9; high-priced/active names; 15 on 2025-06-02).** Larger than summation-order
  noise should give, so the cause (price representation, included prints, accumulation) is being
  found before any tolerance changes (`pass-fx65`).
  **Cause found (`pass-fx65` merged, `worker` cdf58be2, reference 87 passed; D-370 amended):** (1) VWAP:
  a rounding tie rule, not noise: the reader rounded trade prices half-to-even, the engine half away
  from zero (D-107 6, the definition); all 86 VWAP differences are whole tie shares moved one step
  (cache always above); the reader now rounds exactly as the engine, and the VWAP tolerance is a cited
  bound γ_2n (Higham §4.2) instead of 1e-10. (2) `conditional_last_rejected`: the engine counts the
  whole session date's rejected conditional prints, the reader only in-bin prints (every bin equal;
  excess 1–10 = prints outside the bins); the reader now counts at the session-date scope (no
  published value reads this count). 42 FAIL verdicts over 16 sessions so far; re-verify 2024-04-09,
  06-03, 06-10, 07-15, 08-01, 12-02, 2025-06-02, 08-01, 12-01, 2026-02-02, 04-01 on the new image.
  **06:17 UTC: test block on 3b85b4f6 EVICTED at 53 min (worker 13.5 GB of the node's 16 GB, during
  the snapshot stage: optimization pass 1's 8 snapshot sessions at once, each deriving markers for
  64 securities).** Optimization regression: in-flight defaults need memory-aware limits. Restarted
  (core requeued job 1384) with `VYREON_SNAPSHOTS_IN_FLIGHT=2`, `VYREON_BULK_FILES_IN_FLIGHT=2` and a
  14 GiB container limit; memory sampled every 2 min to `checks/2026-11-20-block-1028-memory.tsv`
  to set defaults from measurement.
  **07:10 UTC: tape sample done. All 1,276 recomputed caches equal the stored ones byte for byte (20
  sessions, 2024-04-09..2026-11-06). Re-verified on build cdf58be2 (fixed reference): every session
  re-checked now PASSES (2024-04-09, 06-03, 06-10, 07-15, 08-01, 12-02, 2025-06-02, 08-01, 12-01,
  2026-02-02; 2026-04-01 pending).** The four last sessions (2026-06-01, 08-03, 10-01, 11-06) were judged by
  the old reference (same two patterns: VWAP ties, conditional-last count) and are re-verifying
  (jobs 1416–1419, Job `v-tape-reverify2`). The memory figure first sampled was cgroup usage
  including ~12 GB reclaimable page cache; the sampler now records the working set (what eviction
  uses).
  **07:22 UTC: test block evicted again, this time with 2 bulk files and 2 snapshot sessions in flight.
  Not page cache, a leak:** working set flat ~1.5 GB through the bulk files; then one
  `h2-engine live-capture-batch` (started 07:07) grew anonymous memory linearly 0.1 → 12.4 GB in 15 min
  until eviction (`checks/2026-11-20-block-1028-memory.tsv`). The old image (48d87154) never did this,
  so the suspect is audit round y's per-request byte reservation (`live_batch.rs` `byte_grant`, 64
  slots re-asking every 50 ms). Job stopped (core requeues 1384), node day-36n2pj deleted; cause and
  fix in `pass-fx66` with a local real-batch reproduction. Test block re-runs on the fixed image.
  **Cause found (`worker` 4944a182, D-401 amended): not the batch and not snapshot parallelism (the
  first eviction's attribution above was wrong).** OCC detection's per-process chain cache
  (`occ.rs` `eod_chain`) never let go; D-398's `unlisted` (between 48d87154 and 3b85b4f6, verified
  by ancestry) made it read every session's own and previous chain for all 64 securities, so the
  quotes stage kept the whole window: ~83 MB a session × ~150 sessions ≈ 12.5 GB (evicted at 12.4).
  Bounded to 128 chains (least recently read let go); outputs unchanged; real-data test fails without
  the bound. Batches measured flat at 16/64 in flight (byte-reservation path cleared). Defaults 3 bulk
  files / 8 snapshot sessions kept; the Job's 2/2 override is dropped on the fixed image.
  **Image 4944a182 (build 4944a182+56c30ad6ed6d, 1,495 passed) admitted; job 1384 (pinned to
  3b85b4f6) cancelled; test block 2026-10-28..30 rescheduled on it at default in-flight, 14 GiB
  limit, history on; working set and top processes sampled every 2 min from the start
  (`checks/2026-11-20-block-1028-memory-4944a182.tsv`).**
  **08:17 UTC: tape sample COMPLETE: 20 of 20 sessions passed (1,276 of 1,276 asset-sessions
  recomputed byte for byte with the reference tape reader agreeing).** The historical tape caches
  carry "cache code verified by sample" (C-059). Node day-std-36n2pr deleted.
  **09:50 UTC: block 2026-11-02..04 COMPLETE (image 48d87154, job 1383, 1 attempt): published in 291,
  265 and 294 min; reference checks 31–32 min each (166–169 CPU-min); 11-03 2,489 and 11-04 2,146
  failed of ~93k checked.** Node day-3az1af deleted. Test block on 4944a182 past the old leak point:
  fill peak 6.7 GB anonymous (requote stage), then falling: memory fix holds. 11-03/11-04 failure
  classes not seen on 11-02 (24 and 35, 58 and 71 failures; mostly per-expiry forward keys of the
  ruled families; new: DLM-DRAWDOWN on COIN/ETHA/IBIT/MSTR, one ETHA max-pain 1.5e-11, two sigma
  states, one UUP vega) being triaged under the current suite (`pass-fx67`).
  **Triage done (`worker` 5da7a4ac, reference 94 passed, D-370 amended;
  `checks/2026-11-20-refcheck-1103-1104-triage.md`): 0 engine defects.** Drawdown: already ruled
  (D-118 1, 253-close window; the reference's 252 picks the next close); one reference defect fixed
  (AAPL σ on a crossed quote: the reference now requires ask ≥ bid, as the engine and Cboe's VIX
  rule); UUP vega is D-119's ±0.01 reading (named alternative); DBA's long-dated deep-ITM put σ state
  unprovable (surface file deleted), so a failed state now carries quote, inputs and QuantLib prices.
  Expected unexplained under the current suite: 4 (11-03) and 9 (11-04), forward-quality signals and
  one σ state; to confirm on the next real checks. Owner-visible note: DLM-PRICE-LEVELS' 52-week
  range uses 252 bars while drawdown's 1-year high uses 253 closes (no wrong number; consistency
  question).
  **Test block 4944a182, 12:45 UTC:** day 2's inputs ready in 64 min (09:54 → 10:58, overlapped);
  day 1 prep 64 min (73 before), intent A running ~100 min. Memory peak 9.2 GB (day 1 compute + day 2
  fill), now ~4 GB. P-21 extended: the API's QQQ 2026-09-28 chain also holds 09-29 data, so the
  bulk fallback cannot recover it; QQQ's option-history-window sensors are withheld on 10-28 (stated
  absence, as designed). Pass-2 lead: intent A runs 4 persistent workers at ~78% CPU each (load 4.1
  on 8 vCPU) once the overlapping fill ends: half the node idle. **Cause (13:20 UTC): my 14 GiB pod limit, not the code
  path's speed.** h4 admission reads cgroup `memory.max − memory.current`, which counts page cache:
  with a limit, the day's written inputs fill the cgroup with cache and the plan saw 2.2 GB free →
  4 of 8 workers (recorded `admitted_workers 4`). Day 1 intent A took 106 min (88 before), so this
  block's intent-A timings understate pass 1. Fix in `pass-fx68` (working set = current −
  inactive_file, as the kubelet measures). The C-001 history-off run goes on the fixed image.
  **Fix merged (`worker` b4966fc1, D-007 amended, D-401 note; full tests + image building).**
  Correction: the plan was made while day 2's fill still overlapped (anonymous ~9 GB, not ~4 GB;
  cache only 1–2.4 GB), so the fixed measure admits 7–8 there, not a clean 8. Pass-2 lever: admission
  is decided once per intent, so an intent started under the next day's fill stays narrow after the
  fill exits (~11:03); re-plan admission between waves.
  **Test block day 1 (2026-10-28) published 14:19 UTC: compute 265 min (prep 64, intent A 106 at 4
  workers, B 71, tail ~24); 258 before, so pass 1's prep saving (−9) was eaten by the throttled A.**
  Exact profile (task-times sidecars, `checks/2026-11-20-task-times-1028/`): intent A = 62,587 tasks,
  332 CPU-min, all history: VS-Q01 83 CPU-min plus four HISTORY-ENDPOINT measurements (O02, O04,
  OIJ01, L01) at ~62 CPU-min each with identical per-task profiles (12,415 tasks each, 200 dates ×
  62 assets, ~300 ms a task, SPY heaviest): the same per-session read done four times. Intent B =
  3,329 tasks, 449 CPU-min, M0-ANALYTICS 328 of it. Pass-2 levers ranked: (1) the four endpoints
  share one read per (asset, date) (−~185 CPU-min on a block's first day); (2) history reuse
  across the block's days (day 2 measures it); (3) admission at full width / re-planned between
  waves; (4) M0-ANALYTICS.
  **Day 1 reference check: 28 min (155 CPU-min); 2,228 failed, 254 unexplained.** ~240 of them are
  QQQ: history sensors withheld over the 09-28 stated absence (reference computes over the gap) and
  same-day DLM measurements (expected move, max pain, gamma exposure, …) withheld as
  DEPENDENCY_WITHHELD: possible over-withholding against the scope rule. Rest: SAI-G01 strike axis on
  ~8 names, one flip-distance class, DBA/INDA forward. Triage `pass-fx69`. Day 2 (10-29) compute
  started 14:48 (its download 69 min).
- **01:52 UTC Nov 19: download speed (owner's question). The download was never rate-bound.** The
  real day fetched option history at ~67 requests/min (4% of the 1,800 budget): Intrinio answers
  in ~2 s and the engine held a fixed 4 in flight. Probe (AAPL, 288 real requests): 4 → 106/min,
  16 → 517, 64 → 1,372, median 1.8/1.7/2.0 s, 0 refusals. **Engine D-395 (`worker` 69f987e9,
  pushed): `VYREON_CAPTURE_IN_FLIGHT`, default 64, max 128; speed only** (322 tests pass with
  `VYREON_FIXTURES` set). In the next worker image; the running day stays on e329b3c8.
  **Cluster budget:** core's leases (C-025) already share 1,800/min across workers; one node at 64
  can take most of it. **Owner rule for phase 1: start another node only when the download budget
  has headroom** (core's lease use below 1,800 minus one node's steady draw; measure the steady
  draw on the first block). Real day's ledger at 01:50 UTC: 17,746 requests, option EOD history
  13,125 (23.7 GB), daily bars 4,621.
  **Bulk option EOD files found (`bulk` access code on our account):** one whole-market CSV per
  day, 2021-09-27..2026-11-17 (1,448 files, ~130 MB zipped, 516 MB / 2.08M contracts for
  2026-11-03, released ~07:10 UTC the next day), 115 GB in all; plus US stock prices (5 y, 0.6 GB)
  and fundamentals. **Parity on 2026-11-03 vs the API rows the worker captured:** AAPL 3,376,
  ETHA 2,728 (ETHA + ETHA1), SPY 10,000 (page 1): every contract present; close, bid, ask,
  volume, open interest, high, low, open and mark identical; counts also equal for NVDA, EWJ,
  UNG, SPY (12,800). One request a day replaces 64, and the whole phase-1 window could be fetched
  once (~10 h at the 1/min bulk limit) into Spaces so runs and rebuilds use no option-EOD budget.
  Before adopting: wider parity (a split day, ETHA's reverse split, a null-volume day, the 3am
  release vs later corrections) and an engine source path from the CSV. **Startup checkout
  (owner):** "Individual and Startup include the same APIs, endpoints, and datasets"; 2K calls/min,
  1 RPM bulk/paging; $999/qtr ×2, $1,998 ×2, $2,997 after. WebSocket: our options stream is
  `opra_derived_realtime` (Intrinio's synthetic price), so no use as an input; dropped.
- **02:05 UTC Nov 19: owner direction: use the bulk files wherever they exist and match the API,
  transient per day like the other inputs (no Spaces copy needed), tested on a cluster node before
  swapping everything.** Network from the core node: 1.1–1.75 Gbps to public test files, Intrinio's
  S3 1.56 Gbps with 6 files at once (the 21 Mbps seen was request handling, not the link).
  **Parity (`checks/2026-11-19-bulk-parity.json`): bulk equals the API in every case**: option EOD
  SPY 2026-11-03 both pages (12,800), ETHA 10-07/10-08 reverse split with ETHA1, XLK 2025-12-05
  split, EWJ 2026-08-26 null volumes, SPY/AAPL/ETHA 2026-06-15 (no later corrections); every
  contract present, no extras, 0 mismatches within 1e-4 on 14 fields; the only difference is the
  API's float32 values (45.04999923706055) vs the file's decimals (45.05), max relative 5.7e-8,
  floating-point noise to report on the first switched day. EWJ 08-26: 793 API rows with null
  `exercise_style` where bulk says "A" (engine default American: confirm). Daily bars: the 5-year
  stock-price bulk equals the API on 35 days across SPY, NVDA, ETHA and XLK splits (key rows by
  security id: IBIT's ticker was a Defiance ETF in 2022–23). **Replaceable: option EOD and daily
  bars** (with snapshots and replay already files). Not replaceable: act01 intervals and exq01
  trades (per contract), company (bulk fundamentals are standardized only, no acceptance times),
  macro. Files arrive T+1 ~07:10 UTC (options), ~04:30 UTC (bars). **Engine work next:** a bulk
  source whose identity is the file's hash plus the extracted rows; the operation id is pinned in
  receipts (h2-engine request.rs; h3-asset node.rs, q01_execution.rs, h4_quality.rs, h4_g01.rs;
  h3-q04; h3-m0-contract), so a new source id or an accepted alias. Then an A/B: 2026-11-03 on
  the bulk source vs tonight's API-sourced result.
- **02:40 UTC: owner decision: fix round x's other twelve in their simplest form (core #1 2 7 8 9 12
  17 19 in `/home/diggz/wt/core-fix-y`; engine #3 10 14 15 16 in `/home/diggz/wt/fx30`), then NO
  further audit of the operations features.** Reason: three rounds (v, w, x) keep finding the same
  class of rare-failure chain, much of it in the previous round's fixes; the owner reads that as a
  sign the features are ready. Ops audits resume only when real operation shows a new failure;
  engine and science audits continue as before.
- **01:12 UTC: round x's six fixes merged (core main 7b27153, image pushed; engine `pass-s3i` e48641a5,
  1,377 passed, 8 known).** Daily claims are daily-only (`daily: true`); the first daily tape
  container waits up to 30 minutes for late scheduling (a no-work morning now holds a node about 30
  minutes, about $0.25); `/live` plus a liveness probe restarts a frozen core, and the lock's session
  uses TCP keepalives (freed in about a minute behind a partition); unreadable archive entries are
  skipped and named; a due-but-unqueued session is a DAILY_STOPPED problem; the flaky test counts
  only its own processes (40 parallel runs passed). Unverified until deployed: the daily claim
  through HTTP, `JOB_COMPLETION_INDEX` on the cluster.
- **00:48 UTC: round x (gpt-6.2-astra; Codex 90% → 91%): 18 findings + 1 suspicion on the simplified
  operations features; no defect in any published value.** Count not converging, meaningfulness is:
  most need a chain of unusual failures (a lost Postgres disconnect notice during an archive
  retry; a gateway 504 after Discord saved a post; one stalled filesystem write). Fixing the six
  that matter, simply (`/home/diggz/wt/core-fix-x`, `/home/diggz/wt/fx29`): 13 daily workers claim
  only daily work (phase 1 relevant); 6 a daily worker waits for late scheduling; 5 a liveness probe
  restarts a frozen core; 4 one bad file no longer drops an archive category; 11 no green digest
  after a failed scheduling step; 18 the flaky test counted every process on the host. **Proposed
  to the owner: record the other twelve as stated limits in the decisions (no new machinery) and
  stop auditing the operations features until real operation shows something new.**
- **00:17 UTC: round w answered by simplification and merged (core main 96f1042, C-053, migration 0023;
  engine `pass-s3i` 8e67c4ec; core 23 unit + 57 DB tests, engine 1,375 passed, 8 known).** Owner
  approved retiring the gate. One core at a time enforced by an advisory lock; the gate, its RBAC
  and template and the notice claim lease removed; the CronJob starts the worker directly (Forbid
  overlaps); digest outside locks; Discord `wait=true`; the archive phase in a child process under
  one deadline; packing failures counted; NOT_DERIVED resolves only on publication. A new engine
  archive test failed once in a parallel run and passed on reruns: to be found. Core image
  `vyreon-core:96f1042` pushed (deploy after the sample). **Round x started** (gpt-6.2-astra): the
  simplification, and the flaky test.
- **23:42 UTC: round w (first audit on gpt-6.2-astra, about 2% of Codex's week: 88% → 90%): 20 findings
  on round v's fixes.** Most sit in machinery the fixes added: notice locks and leases (one real
  stall: the digest holds a lock that blocks publishing, phase 1 included), the daily gate (Job-
  creation RBAC is namespace-wide in effect, no image pull secret, can skip a day or start two
  workers), and races needing two core instances, which never run. Response: simplify. One core
  at a time, enforced by an advisory lock at startup; machinery that only survived two instances
  removed; the rest fixed plainly (`/home/diggz/wt/core-fix-w`, `/home/diggz/wt/fx28`). **Waiting
  on the owner: retire the gate** (the CronJob starts the worker directly; on an exchange holiday a
  node starts and finds nothing, at most about $0.17, about 9 days a year).
- **20:07 UTC: round v's 9 findings all real and fixed; merged (core main, migrations 0021, 0022; engine
  `pass-s3i` 41ca88cb, D-360 amended; core 25 unit + 62 DB tests, engine 1,371 passed, 8 known).**
  Scheduler and Discord sender are separate tasks; each post is claimed under a lock (one residual
  duplicate if core stops between Discord accepting and core recording; Discord webhooks take no
  idempotency key); digests and problem openings share a lock; the weekly check is red while any
  problem is open; archive rows record their endpoint and bucket; packing runs under the archive's
  15-minute deadline; the 07:45 CronJob runs a gate on the core node (`vyreon-core daily
  start-worker`, minimal RBAC) that starts a worker Job only when daily work is waiting. Core image
  rebuilt and pushed; deploy after the sample (delete the old `daily` CronJob first, per the README).
  Untested on the real cluster: the gate's RBAC and Spaces.
- **19:33 UTC: round v returned: 9 findings, no credential leak.** For phase 1: the worker's archive
  packing has no deadline (a stalled compressor could hold a node and its block). Daily operation:
  Discord sending can delay scheduling; empty schedules still start a node; a weekly verdict can be
  green with a failed history pass; a green digest can follow an open alert; delivery is not
  deduplicated; a missing-archive alert can be postponed by retries; a cancelled repair job resolves a
  problem; a bucket change mixes rows. Verify-and-fix running across core (`/home/diggz/wt/core-fix-v`)
  and the engine (`/home/diggz/wt/fx27`); nothing applied until it lands.
- **19:14 UTC: perishable archive merged: core main 8928dab (C-052, migration 0020) and engine `pass-s3i`
  5e40b9dd (D-360; suite 1,370 passed, 8 known failures).** Core signs pre-signed PUT and HEAD URLs
  with rusty-s3 0.11.0 (the hand-written signer replaced: proven library first); the worker packs
  and uploads the cohort prints and ACT01 pulls after a day publishes; core verifies and records;
  a missing archive shows in the digest. Company captures stay in core's database (C-043). To apply
  after the sample: the owner creates a Spaces key limited to bucket `vyreon-data` (narrowest write);
  runbook order in deploy/k8s/README.md "Perishable archive"; the droplet keeps archiving until the
  cluster's archive is seen working. Real Spaces untested (Content-MD5 and ETag behaviour to confirm
  on the first day). **Round v started: one narrow Codex audit of C-050, C-051, C-052 and D-360**
  (credentials, wrong states, stopped work, cost). Codex at 88% of its week (resets 2026-11-20).
- **18:34 UTC: daily runs built and merged (core main 0bd9cd7, C-051, migration 0019; 18 unit + 53 DB
  tests; image `vyreon-core:0bd9cd7` pushed, includes the notices).** A session's inputs are ready by
  07:30 New York on T+1 (evidence: the droplet's captures, Intrinio's FAQ on open interest from
  04:00 ET, the engine's settled-bar rule; summer unchecked). Core queues each session's tape job,
  day block and history pass from `VYREON_CORE_DAILY_FROM` for `VYREON_CORE_DAILY_BUILD`, ahead of
  backfill work; a CronJob (`40-daily.yaml`) starts workers at 07:45 New York, Tuesday to Saturday;
  deadline 20:00 MT on T+1; `DAILY_STOPPED` raised if the build is not admitted. About $1.10 a
  session. Autoscaling the pool to 0: DigitalOcean's pages disagree; the API decides when applied
  (min 1 would cost about $120 a month: owner's call then). **Applied after the sample, in the
  runbook's order (deploy/k8s/README.md).** Open before the droplet retires: the cluster does not
  yet archive the perishable option prints to Spaces (C-004); the droplet still does. **Built
  2026-11-17 (C-052, engine D-360; branches `core-archive` and engine `pass-fx26`, not merged, not
  deployed): migration 0020, upload URLs signed by core, read back before kept; `vyreon-core archive
  list`; runbook `deploy/k8s/README.md` "Perishable archive". Owner: a Spaces key limited to
  `vyreon-data`, Secret `vyreon-core-spaces`, then the images and the settings.** **Design
  (owner, 2026-11-17): workers stay stateless and credential-free; core alone holds the Spaces key
  and hands a worker a pre-signed upload URL for one named object, valid for minutes (the standard
  S3 pattern); the worker uploads directly to Spaces and reports it; core records what was kept per
  session.** Data never passes through core, so the same mechanism serves the sample's raw
  downloads (about 45 GB a day) if they are kept.
- **18:19 UTC: STEP 3 (CLUSTER VERIFICATION) COMPLETE.** 2025-12-10 PUBLISHED on 1a532d8b: 2,688 of 2,688
  measure slots and 512 of 512 day-layer slots, zero faults of any kind. With 2025-04-11 (2,684 of
  2,688, 4 stated ETHA absences), the June 2022 block (META contained) and 2024-07-16's cause found
  and contained (outside phase 1's range), every verification day is answered. **Image diff (process
  addition b), 2025-12-10, build d55f6439 (2026-11-16) against 1a532d8b, value by value:** of 3,958
  readings, no state changed and the values of all but 12 families are identical. Each change ties
  to a decision: CDD-MACRO01 (64) CPI is now the unrevised CPIAUCNS (D-305: e.g. 324.245 → 324.8);
  INT-TEMP-EVC01 (64) now SUPPORTED from the full 252-session tape history (D-310 with the
  completed cache); FIELD synchronization readings follow from those inputs; company families
  (17 each) gained the provider-comparison and vintage coordinates (D-259..D-261, D-337..D-356);
  DLM-EVENT-WINDOW states its forecast basis (D-305). One unexplained: DLM-OI-CENTROID for BABA
  differs at the 16th significant digit (summation order), probably from BABA1's adjusted root now
  resolving; to confirm. Idle node deleted; only the sample is running.
- **18:10 UTC: notices redesign merged (core main b04f882, C-050, migration 0018; 18 unit + 48 DB tests;
  image `vyreon-core:b04f882` pushed). Deploys when the sample finishes** (a core restart mid-run
  risks the workers' leases). Problems are derived from the database every minute: one alert
  when one opens, one "Resolved" when it clears; old chatty notices are log only (`vyreon-core
  log`); `problems`, `digest [weekly]`, `notices mark-sent`. Deadline checks start when
  `VYREON_CORE_DEADLINES_FROM` is set: at the first daily run, not for the backfill.
- **Owner, 2026-11-17, the sequence:** when the two-week sample completes safely and without
  error, the system is called **ready** ("from this day forward it can reliably build data"); then
  phase 1's backfill starts and, in parallel, **daily runs capture each new trading session**
  (deadline alerts switched on from that day). When phase 1 completes, the current dataset with
  enough history is what the reports are built on.
- **Owner, 2026-11-17: phase 1 is the priority** ("The database build is 10-ish days alone for
  phase 1. Then we can sprint towards live launch readiness."); no launch target set.
- **17:54 UTC: the droplet's old Discord alerts retired (owner's word):** `vyreon-health.timer` and
  `vyreon-heartbeat.timer` stopped and disabled; `vyreon-capture` and `vyreon-archive` keep running
  (they save the option-trade cohort prints the provider will not serve again, to the volume and
  Spaces, until the cluster runs daily) with their notices muted to
  `/var/log/vyreon-notices-muted.log` (systemd drop-ins `mute-notices.conf`). The droplet itself
  (`vyreon-engine-shared-8vcpu-amd`) retires once the cluster's daily runs keep those prints:
  owner's call then.
- **17:48 UTC: notices design APPROVED; webhook stored as cluster Secret `vyreon-core-webhook`** (new
  channel; file gitignored, mode 600, never printed). Owner: "I want to open it and immediately
  know if i gotta go look at it or its running ok." Building in core (`/home/diggz/wt/core-notices`):
  problems table (one alert per problem, one "resolved"), deadline alerts, paused queue, last-time
  failures, provider auth/error bursts, DB size; 09:00 MT digest with a verdict first line (🟢 / 🟡 /
  🔴 Needs you), Saturday readiness check; spend and idle-node checks are v2. Switched on with the
  backlog marked sent.
- **Discord notices (owner, 2026-11-17, pasted ten days of the droplet's alerts): noise.** A normal
  "provider has not published yet" raised as 🔴 every evening; 64 tickers listed two or three times
  per alert; a stopped run re-alerted every 12 h for 3.5 days; "all well" while nothing had
  published for a week. Planned in core: alerts only for what needs the owner (refused day, paused
  queue, a session not published by its deadline, spend, core disk), one 09:00 MT digest, counts not
  ticker lists, one alert per problem then the digest then one "resolved". Waiting on the owner: the
  webhook as a cluster secret; leave to stop the droplet's alert timer (its capture timer checked
  separately for perishables first).
- **13:42 UTC: STEP 4a STARTED: the two-week sample, 2026-11-02..2026-11-13 (10 sessions) as two blocks
  of 5 on build 05837898+3d485555ec3d** (image pushed, registered 4,679 slots, admitted), Job
  `v-sample`, 2 pods; pool `day` at 4 nodes (2 more for 2025-04-11, computing, and 2025-12-10,
  downloading, on 1a532d8b). Judged against the approved pass criteria when done; history passes
  then scheduled once for the range (C-046). **Not yet built: process addition (d), keeping the
  sample's raw downloads in Spaces** (needs a worker upload step and a Spaces credential in the
  cluster); without it a sample re-run downloads again (about 3.5 hours). Built only if the sample
  needs re-runs.
- **13:27 UTC: D-359 merged (`pass-s3i` 05837898; suite 1,362 passed, 8 known failures).** A source the
  provider's partial closing snapshot fails (under half its chain, counts now stated) is a stated
  absence: that session's option readings and every option-history reading reaching it are
  withheld PROVIDER_INPUT_MISSING; the day proceeds. Open for the deeper-history phase, not
  phase 1 (its windows start after mid-2024): a partial session withholds those assets'
  option-history readings for the ~200 sessions after it (to about August 2024), and the 10%
  missing-input hold will probably hold such days. Choice deferred to build evidence, per the
  owner's rule that computing over gaps waits for the build: change the hold, or end the history
  at the partial session.
- **13:03 UTC: 2024-07-16's cause found (D-357's evidence worked): `requote` rejects 111 sources with
  `close quotes cover under half of the population`: 41 on 2023-10-30, 59 on 2023-11-17, and
  1-4 on seven other dates up to 2024-06-10 (61 assets).** The provider's closing snapshot was
  partial on those dates; the gate is right, but one rejected source refuses the whole day.
  2025-04-11 and 2025-12-10 (windows back to mid-2024) hit none, so phase 1 looks clear; the
  containment is fixed anyway (`/home/diggz/wt/fx25`, D-359: such a source is a stated provider
  absence for that asset and session, the day proceeds). Job 1377 cancelled; its node deleted.
  Evidence: `~/claude-artifacts/provider-evidence/requote-partial-snapshots-2023.txt`.
- **11:29 UTC: TAPE HISTORY COMPLETE for phase 1: 654 of 654 sessions (2024-04-09..2026-11-13) in core's
  tape cache, 128 entries stated absences (2025-01-10 and 01-13, provider-refused).** About 5.5
  hours from first start, most of it on 6-18 pods; idle nodes deleted (pool down to the 3 day
  re-runs). This is phase 1's whole tape need: no day job will download tape history again.
- **10:09 UTC: June 2022 block DONE on 2967d2cb: 2022-06-13, 06-14 and 06-15 all PUBLISHED from an
  empty disk.** Each day: 2,604 measure slots, 2,218 complete, 1 engine fault (META VS-P01, the
  rename-era price history; contained) and its 13 dependants, no failed measure; the field/day
  layer 496 of 496. The rest are stated absences (pre-tape era sources). Its node deleted.
- **09:56 UTC: day re-runs started on 1a532d8b** (jobs 1377 2024-07-16, 1378 2025-04-11, 1379
  2025-12-10; all claimable, their tape history cached), Job `v-day-1a53`, 3 pods; pool `day` at 10
  nodes (about $1.67/h) while tapes finish (519 of 654; the rest are after 2025-12-10). 2022-06-15
  still computing (job 68).
- **08:54 UTC: 2022-06-14 PUBLISHED on 2967d2cb** (the day the old build lost to META's history is
  now published, META contained); 06-15 computing. D-358 worked on real data: 2025-01-10 and
  01-13 recorded as 128 stated tape absences after their retries. Tape history 385 of 654; late-2025
  sessions take 6-9 min each (bigger replay files, download-bound per node), so the pool went to 7
  nodes and the tape Job to 18 pods (same node-hours, about 1.5 h instead of 3-4).
- **07:31 UTC: the queue had paused itself** when old tape job 259 failed for the last time
  (core's safety stop: `queue_control` paused with that reason), so the first 654 tape pods found
  nothing to claim and exited. Resumed (`vyreon-core queue resume`) now that D-358 handles the
  cause; Job `v-tape-1a53b` restarted, cached sessions skipping in seconds (47 done in 90 s).
  **Practice for the build: after any job fails for the last time, read the evidence, fix or
  accept the cause, then resume; check `core.queue_control` whenever pods find nothing to claim.**
- **07:21 UTC: build 1a532d8b+c17a5feae181 registered and admitted (image pushed): D-357 (a failing
  child's error lines lead its message; evidence reads every run log) and D-358 (a replay
  session the provider refuses on a job's last attempt is a stated TAPE_PROVIDER_REFUSED absence
  in core's tape cache; later sessions proceed).** 440 old tape jobs cancelled; 654 re-queued on
  1a532d8b (cached sessions are skipped by `tape_held`); pool `day` at 3 nodes; Job `v-tape-1a53`,
  6 pods. Open, from D-358: core's claim does not send `max_attempts` (the worker assumes 3);
  superseding a refused session later means deleting its rows (preview first) and re-running.
- **06:55 UTC: 2022-06-13 PUBLISHED on 2967d2cb** (company fix confirmed on a real day; META
  contained); job 68 on 06-14. **Tape history stalled at 214 of 654:** Intrinio answers HTTP 500
  for the replay files of 2025-01-10 and 2025-01-13 (three attempts each; Intrinio's own MCP fails
  the same way for cta_a and utp on both dates; 2025-01-08 is served, 1.6 GB). The failed jobs
  (259, 260) block every later tape claim, so pods found nothing and the Job ran out. Evidence:
  `~/claude-artifacts/provider-evidence/tape-500-2025-01-10.txt`. Fix running (`/home/diggz/wt/fx24`,
  D-358): a session the provider refuses on a job's last attempt becomes a stated provider-
  unavailable absence in core's tape cache, so later sessions proceed. Tape Job deleted, its 3
  nodes deleted; resumes on the next build.
- **06:46 UTC: 2024-07-16 refused a third time on the old build (job 65, now cancelled); the cause
  is still unseen:** `requote` prints its per-source FAILED lines to a log the evidence does not
  read, and the worker's error message keeps only the first 500 characters of the child's
  output (successful sources). My catcher script also failed (it used `bc`, absent from the
  image; fixed). Engine fix running (`/home/diggz/wt/fx23`, D-357): a failing child's error lines
  lead its message; the evidence reads every log in the run's logs directory. 2024-07-16 re-runs
  on the next build after the tape history. Tape jobs: about 230 of 654 done, 6 pods.
- **05:51 UTC: tape history first, as D-310 planned.** A day job without core's tape caches
  captures its 252 prior tape sessions inline, about a minute each, and does NOT upload them
  (core held 319 entries over 4 sessions; unchanged after 5.5 hours of inline captures). So
  2025-04-11 and 2025-12-10 (old build, their company data doomed anyway) were stopped (jobs 66,
  67 cancelled) and **654 tape jobs queued for 2024-04-09..2026-11-13 on 2967d2cb** (phase 1's whole
  tape range, the sample's included), Job `v-tape-2967`, 4 pods (2 CPU each) on the 2 freed nodes,
  about 4 hours. Then the day re-runs and the sample read core's caches. Still running: job 68
  (2022-06-13..15 on 2967d2cb), job 65 (2024-07-16, old build, for the requote catcher).
  **Lesson for the plan: a day job must never be started before its tape history is cached.**
- **04:40 UTC: D-356 merged; build 2967d2cb+32d87d91c30e registered, admitted, image pushed.**
  The real capture showed the defect was wider than an empty page: every fundamentals listing
  page with fewer rows than its page size carries no `next_page` key, so since D-281 EVERY
  day's company capture failed. Now: a short page without the key ends its request (a full one
  still fails), and a schema terminal withholds the reading (MISSING_REQUIRED_INPUT, the
  terminal stated) instead of FAILED, so it never stops a day. A test bound to the date
  2026-11-17 fixed. 2022-06-13..15 re-queued on 2967d2cb (Job `v-day-2967`, the freed node).
  2025-04-11 and 2025-12-10 (b727fc6b) keep running for their other evidence; their company
  readings will fail staging, so both re-run on 2967d2cb afterwards.
- **04:11 UTC: re-runs on b727fc6b, first results (failure evidence in core worked on first use).**
  (1) **2022-06-13: every company's CDD-FND01/02 FAILED (`SCHEMA:next_page_absent`), so the day
  refused to stage.** Intrinio's `/companies/{id}/fundamentals` returns an EMPTY list with no
  `next_page` key; D-281 (round n, 2026-11-16) made that an incomplete capture. Real response
  kept in `~/claude-artifacts/provider-evidence/2026-11-17-empty-fundamentals/`. Fix running
  (`/home/diggz/wt/fx22`, D-356: an empty page without `next_page` ends the request; a capture's
  schema terminal is a provider absence, never a stopped day). Job 64 cancelled until then.
  (2) META's VS-P01 on 2022-06-13 is now contained (ENGINE_FAULT for META and its dependants,
  the day not stopped): cold start after the FB→META rename, likely a price history split across
  Intrinio's two security records; outside phase 1's range (no rename among the 64 since); fix
  before any deeper history. (3) **2024-07-16 refused again at the requote of middle-layout
  sources (2023-09-27 and similar)**: the requote binary's per-source FAILED lines are not in the
  log the evidence reads; a catcher reads them from the live retry pod
  (`cluster-scratch/catch-requote.sh`). Also seen: HTTP 429 from Intrinio with four nodes
  running (retryable). 2025-04-11 and 2025-12-10 still computing.
- **00:14 UTC: sample criteria APPROVED by the owner; spend is evaluated after the sample, before
  phase 1.** June 2022 block (old build 518faf7e) ended: 06-06..06-13 published; **2022-06-14
  failed twice at intent B: `META:2022-06-14 VS-P01 fault: daily_history: H3 asset failed: P01
  retained history does not close eligible CAP126; older input required`** (META renamed from
  FB on 06-09), and that one asset stopped the whole day (a containment gap). Evidence kept in
  `cluster-scratch/summaries-2206e/`. Old jobs 55-63 and 25 cancelled; node deleted. **New core
  deployed (3e7b13e; migrations 0016, 0017 applied); build b727fc6b+9b96b570b46b registered (4,679
  slots) and admitted.** Re-runs queued as jobs 64-67: 2022-06-13..06-15 (the META case),
  2024-07-16, 2025-04-11, 2025-12-10 (image diff against `testbed-dumps/c-1210`); pool `day` at 4
  nodes ($0.67/h), Job `v-day-b727`, watcher `cluster-scratch/watch-job.sh`. Tape history is
  sparse (319 cached entries), so tape-history readers may be withheld on these days.
- **23:39 UTC: round u (last narrow check before the build image): no findings** (no wrong
  number, stopped day or credential path; D-354's ordinary-day frequency not establishable
  without the sample). Converged; the image stands.
- **Two-week sample: draft pass criteria and sizing, for the owner's approval** (process
  addition a). Sessions 2026-11-02 to 2026-11-13 (10), two blocks of 5 on 2 nodes, with their
  tape jobs and one history pass for the range. Pass = all of: (1) every session publishes, or a
  refusal has its evidence in core and a cause; (2) no unexplained ENGINE_FAULT; (3) each sensor
  on at least 80% of asset-days, or every miss diagnosed (engine → rework, provider → stated
  absence, structural → not applicable), D-354's withheld open interest counted; (4) `refcheck`
  (QuantLib, TA-Lib) agrees within its tolerances on every day; (5) EDGAR agrees for every
  company's latest-quarter figures; (6) no day-over-day jump the market did not make (plots for
  the owner's eye); (7) the image diff: one day re-run on the new image against its earlier
  dump, every changed value tied to a decision. A failure is fixed and its days re-run.
  Sizing: about 12 node-hours per block (first day ~5.5 h, later days ~1.6 h, measured on the
  June block), about 25 node-hours, about $4-5; raws kept in Spaces about 450 GB, about $9 a
  month until phase 1 starts. Spend: DigitalOcean month to date $102.53 (all of November, standing
  infrastructure included). **Phase 1 (about 1,100 node-hours planned, about $185) would take the
  total past $300: the owner decides at the sample's end.**
- **23:21 UTC: images built and pushed:** `vyreon-worker:b727fc6b` (build b727fc6b+9b96b570b46b,
  inventory `cluster-scratch/inventory-b727fc6b.json`) and `vyreon-core:3e7b13e` (8 minutes on
  the laptop with the build cache). Registry login refreshed (30 days, from the DO API, never
  printed). **Core deploys when the June 2022 block ends** (job 55, on 06-14, 06-15 downloading,
  about 2 hours): it uses core and must not lose it mid-day. Then register the build, re-run
  2025-04-11 and 2024-07-16, diff against the earlier dumps, history passes.
- **23:11 UTC: candidate final: `pass-s3i` b727fc6b (D-355: one selection rule for every FND01
  cell; a newer unreadable statement withholds, no older figure stands in); suite 1,347 passed,
  8 failed (`option_field`). `worker` fast-forwarded to it (pushed). Images building on the
  laptop (`cluster-scratch/build-images.sh`, log `build-b727fc6b.log`): worker from `worker`,
  core from main (migrations 0016, 0017). Round u started in parallel: narrow Codex check of
  batches 17-18 and D-351, only phase-1 wrong numbers, stopped days or credential leaks.**
- **22:56 UTC: batch 17 merged (`pass-s3i` 2bf314fd, pushed; D-352..354); suite 1,346 passed, 8
  failed (`option_field`).** All six round-t findings real and fixed. D-343 retired (declared
  dividends never deducted; no to-common or EPS line → withheld). Q4's operands all get D-346's
  check. The tape cache validates itself on every read (no side markers). Intrinio's dividend
  records cannot tell special from ordinary, so a one-step ladder reduction reads as unknown and
  **that session's open interest is withheld (PROVIDER_OI_INCOMPLETE)**: watch its frequency in
  the sample (coverage floor). Last small fix (D-355: a newer unreadable net-income or to-common
  figure withholds, one shared helper) running in `/home/diggz/wt/fx21`; then the image.
- **Reporter gap found (survey 2026-11-16):** Reporter reads only the engine's own API
  (VYREON-ENGINE-API@1: `/v1/review`, `/v1/rankings`, `/peers`, windows); core serves only
  `/v1/assets/{symbol}/history` and `/v1/sessions/{date}`. The paid issue's review and
  rankings need serving from core, without science in Reporter. No email/beehiiv output yet.
  **Owner, 2026-11-16:** Reporter was not built for the distributed system; the bulk of step 6 is
  adapting it to what the database became. vyreon-web is wired to Reporter only once Reporter
  reports reliably.
- **22:32 UTC: failure evidence merged** (engine D-351 into `pass-s3i` 347455f2; core C-049,
  migration 0017 `core.job_evidence`, main 0e27c5e; agent's totals: engine 1,341 passed, 8 failed;
  core 56 passed). A failing, refused or lease-lost worker puts job, stage, stop message, the
  plan's refusals and a redacted log excerpt (whole error lines; fill log last 4 MiB) to core
  within 60 s; core refuses any upload where a credential pattern survives. `vyreon-core evidence
  list|show`. **Round t (narrow) returned: 5 confirmed, 1 suspicion, all conditional or
  rederivation-only; nothing on the normal phase-1 path proven.** My D-343 was wrong by FAS 128
  (declared ≠ accumulated on cumulative preferred): being retired. Fix batch 17 started
  (`/home/diggz/wt/fx20`, D-352..358): retire D-343, guard Q4's operands, a self-validating tape
  cache instead of side markers, D-290 to require the provider's evidence of a special
  distribution (OCC §11A). Codex at 85% of its week (resets 2026-11-20).
- **22:19 UTC: batch 16 merged: `pass-s3i` 67599773 (pushed), D-346..349; suite 1,339 passed, 8
  failed (`option_field`).** Round s: 4 real and fixed, 1 already fixed by D-345 (verdicts in
  `.../batch14-round-s/triage/verdicts.md`). A newer null dividend line now withholds; the tape
  fault record now lives beside T's capture, so every derive path (including `vyreon-days
  daily/rebuild`'s deferred `derive_all`, a gap bigger than the audit said) sees it; a failed tape
  capture stays ENGINE_FAULT; D-336 needs every image of a series to be a new strike (OCC §11A).
  Residual named by D-349: D-290 can read an ordinary dividend day as an ex-date when the top old
  strike is delisted. **Round t started: narrow Codex check of batches 15 and 16 plus that
  residual.**
- **22:07 UTC: worker failure evidence (process addition c) started** in parallel: engine
  `/home/diggz/wt/fx19` (pass-fx19) and core `/home/diggz/wt/core-evidence`: a failing or refused
  worker puts its stop message, refusal list and a redacted log excerpt to a new core table
  before exiting; `vyreon-core evidence list|show`. Goes into the same image.
- **21:54 UTC: round s returned (Codex, batch 14): 4 confirmed, 1 suspicion; nothing blocks; no
  day-stopping or nondeterminism.** A stale preferred-dividend fact surviving a newer unreadable
  one (wrong number, pattern unseen); own-store and `derive-local` reading an unreconciled tape
  (partly closed by D-344); a failed tape capture relabelled as missing (mislabel); the reference
  treating an unreadable dividend as zero; suspicion: D-336 reading an ordinary ladder expansion
  as an adjustment. MSTR's figures and the upload deadline checked clean. Batch 15 merged
  (`pass-s3i` 17a70729, pushed). One agent verifies each finding (finding 5 on real chains) and
  fixes the real ones (`/home/diggz/wt/fx18`, D-346..350).
- **Batch 15 done: `pass-fx17` 17a70729 (pushed; D-343..345), held unmerged while round s reads
  the `pass-s3i` worktree.** Declared preferred dividends are the deduction where no to-common
  or EPS-adjustment line exists (MSTR, real figures: matches its own to-common lines); a fourth
  quarter mixing the declared and EPS elements is withheld. Own-store DLM-VWAP follows D-340
  (`derive_all`/`derive-local` still don't read tape faults: small, open). Reference has
  D-327/D-337/D-343. Suite 1,336 passed, 8 failed (`option_field`). Ordinary day: no change.
  Merge after round s, then a narrow check of it with round s's fixes.
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
