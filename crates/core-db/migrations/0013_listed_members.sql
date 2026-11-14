-- C-036: a member has slots only on the sessions it is listed. Each revision records the members
-- listed on its day when it was published (asset keys, profile order), so what a stored day was
-- expected to hold never changes when the universe is loaded again. Its expected slots are the
-- build's inventory rows for the field object plus those of the members recorded here.
ALTER TABLE vyreon.day_revision ADD COLUMN listed_members text[];

-- Revisions published before this: every member of their build's inventory was expected on every
-- day, so each records exactly those objects (the field aside), which keeps what was expected of
-- them unchanged. Ordered by the profile's member order where the universe is loaded, else by the
-- inventory's.
UPDATE vyreon.day_revision d SET listed_members = (
    SELECT COALESCE(array_agg(o.object_id ORDER BY m.ordinal, o.first), '{}')
    FROM (SELECT e.object_id, min(e.slot_ordinal) AS first
          FROM core.expected_slot e
          WHERE e.engine_build = d.engine_build AND e.profile_revision = d.profile_revision AND e.object_id <> 'FIELD.CORE64'
          GROUP BY e.object_id) o
    LEFT JOIN core.member m ON m.profile_id = d.profile_id AND m.asset_key = o.object_id);

ALTER TABLE vyreon.day_revision ALTER COLUMN listed_members SET NOT NULL;
