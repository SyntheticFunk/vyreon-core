-- C-098 (audit of 2026-11-26's evening batch, Codex and Gemini): the fixes that need the schema. Additive and
-- short; it locks only core's own small tables (`core.withheld_tally`, `core.withheld_counted`,
-- `core.perishable_unpacked`), never `vyreon.measurement`, `core.lease`, `core.job` or `core.budget_lease`.
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';

-- Core 4: the daily session's reserve is on from its worker's ASK, not only from a grant (after an idle gap the
-- nightly's first draw met the backfill at the full ceiling). One row a worker, its last ask.
CREATE TABLE core.budget_daily_ask (
    worker   text        PRIMARY KEY CHECK (worker <> ''),
    asked_at timestamptz NOT NULL
);

-- Core 10: a worker that could not pack a day's logs can say why.
ALTER TABLE core.perishable_unpacked DROP CONSTRAINT perishable_unpacked_category_check;
ALTER TABLE core.perishable_unpacked ADD CONSTRAINT perishable_unpacked_category_check
    CHECK (category IN ('exq01-events', 'act01-auto', 'run-logs'));

-- Core 9: the counts read every body schema's withholdings now (`census::stated`): a cell of a FIELD array and a
-- company array are grains of their own, and what was counted under the first reading is counted again (the
-- watch recounts in its bounded way, the newest revisions first; until then the census reads the bodies).
TRUNCATE core.withheld_counted, core.withheld_tally;
ALTER TABLE core.withheld_tally DROP CONSTRAINT withheld_tally_grain_check;
ALTER TABLE core.withheld_tally ADD CONSTRAINT withheld_tally_grain_check
    CHECK (grain IN ('slot', 'role', 'coordinate', 'cell', 'array'));
