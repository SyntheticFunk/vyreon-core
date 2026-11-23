-- C-070 (audit 2026-11-23, Gemini 1, BLOCKS): at most one PUBLISHED revision per day, profile and
-- layer (the base, the day-local revision, the history revision). 0002's index held one per day
-- and profile revision only, and a publish superseded only the same profile revision, so a build
-- that bumped a profile revision left the old one PUBLISHED beside the new and readers served both.
-- A publish now supersedes the day's revision of its layer (`publish::promote`); this index holds it.
--
-- The store is checked first: a day with two PUBLISHED revisions of one layer stops the migration,
-- naming them: core does not choose which to keep (the operator retires the day, `vyreon-core days
-- retire`, and computes it again, or publishes it again). The same query is C-070's pre-check.
DO $$
DECLARE
    found text;
BEGIN
    SELECT string_agg(format('%s %s %s: revisions %s', d.represented_date, d.profile_id, d.layer, d.revisions), '; ')
    INTO found
    FROM (SELECT represented_date, profile_id,
                 CASE WHEN base_revision IS NULL THEN 'base' WHEN day_local THEN 'day-local' ELSE 'history' END AS layer,
                 string_agg(revision_id || ' (' || profile_revision || ')', ', ' ORDER BY revision_id) AS revisions
          FROM vyreon.day_revision
          WHERE state = 'PUBLISHED'
          GROUP BY 1, 2, 3
          HAVING count(*) > 1
          ORDER BY 1, 2, 3
          LIMIT 20) d;
    IF found IS NOT NULL THEN
        RAISE EXCEPTION 'C-070: days with two PUBLISHED revisions of one layer (at most 20 shown): %', found;
    END IF;
END $$;

CREATE UNIQUE INDEX day_revision_one_published_layer
    ON vyreon.day_revision (represented_date, profile_id, (base_revision IS NULL), day_local) WHERE state = 'PUBLISHED';
