-- C-052 amended (audit 2026-11-18 round x, 17). A worker sends a "not packed" report again when
-- the answer to its first sending was lost; one occurrence (the job, the worker that ran it, the
-- session and the category) is one record. A repeat already stored is the same report: the
-- earliest is kept.
DELETE FROM core.perishable_unpacked u
WHERE EXISTS (SELECT FROM core.perishable_unpacked e
              WHERE e.job_id = u.job_id AND e.worker = u.worker AND e.session = u.session AND e.category = u.category
                AND e.unpacked_id < u.unpacked_id);
ALTER TABLE core.perishable_unpacked ADD CONSTRAINT perishable_unpacked_once UNIQUE (job_id, worker, session, category);
