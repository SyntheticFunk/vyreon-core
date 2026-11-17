-- C-052 amended (audit 2026-11-17 round v, coreops 3): a row names the whole place its upload URL
-- was signed for, the endpoint beside the bucket, and core reads the object back only there. A
-- request sent again after the settings changed moves a row not kept to the new place with its
-- new URL; a report for a row whose place is no longer the one core signs for is recorded FAILED,
-- never verified elsewhere. Rows from before this have no endpoint (the archive was off on the
-- cluster: none are expected); they count as another place.
ALTER TABLE core.perishable_archive ADD COLUMN endpoint text;
