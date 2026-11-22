-- Audit 2026-11-22 prebuild round 2 (core-watch.md) 6 and 8: a claim keeps a miss only when a
-- claimable job no other claim held was there and it still could not lease one; the miss names
-- that job, and the watch counts it only while that same job is still claimable (a different job
-- queued later is not the work the claim missed). NULL only on rows written before this.
ALTER TABLE core.claim_miss ADD COLUMN job_id bigint;
