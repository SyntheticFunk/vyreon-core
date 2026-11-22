-- C-068 (C-035 amended): the operational failure-surface audit of 2026-11-22 (opsurface/ops.md), 13.
-- The session a lease's job stood on when the lease expired (a lost node, a
-- killed pod, a Job's deadline), so an expiry is charged to the day it stopped on only after that day
-- has already lost `work::LOST_LEASES_FREE` leases (`work::lost`). NULL for every other ending.
ALTER TABLE core.lease ADD COLUMN stopped_on date;
