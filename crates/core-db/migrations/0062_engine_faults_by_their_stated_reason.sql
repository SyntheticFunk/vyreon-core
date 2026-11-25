-- (engine investigation, 2026-11-25) the tally counted a slot as an engine fault when the words
-- ENGINE_FAULT appeared anywhere in its body, and the digest named provider absences among them (every
-- withheld slot's result id ends `/engine-fault`, whatever its reason). Engine faults are now the slots
-- whose stated reason (the body's terminal reason or a role's) is ENGINE_FAULT or PARENT_ENGINE_FAULT
-- (`watch::FAULT`); the kept tallies are dropped and counted again by the watch (`tally_untallied`, in
-- batches, from its next minute).
SET LOCAL lock_timeout = '10s';
SET LOCAL statement_timeout = '60s';
DELETE FROM core.revision_tally;
