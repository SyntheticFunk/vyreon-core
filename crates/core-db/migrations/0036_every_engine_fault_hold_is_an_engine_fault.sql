-- C-061 amended (audit 2026-11-22 prebuild, changes-core.md 9): the engine holds a day for its own
-- faults in three places, and only one read as ENGINE_FAULT; the other two read FAILED, and `omit`
-- (which refuses only ENGINE_FAULT) let a day the engine faulted be omitted as a provider day. The
-- three holds, as the worker reports them (`stop: <date> not published: ...`, engine worker branch
-- 2026-11-21):
--   * the final slots' hold, `held: N of M final slots are withheld for an engine fault (more than X): ...`
--     (vyreon-cli `exports.rs` `hold_final`), as before;
--   * the executor's, `held: intent <a|b>: the executor held the day (too many faulted slots; <log>)`
--     (vyreon-cli `day.rs`, the executor's exit 1, R7.1): detail without the log's path;
--   * the day-local derive's, `<date> held: N of M objects' day-local derived records faulted (more than X); ...`
--     (vyreon-cli `derive/local.rs`, D-103; it reaches the stop message as an error, not after `held: `):
--     detail its counts, without the inherited counts and the list of objects.
CREATE OR REPLACE FUNCTION core.failure_cause(message text) RETURNS jsonb LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE
        WHEN h LIKE '%lack a provider input%' THEN jsonb_build_object('code', 'PROVIDER_INPUT_MISSING', 'detail', regexp_replace(h, '\): .*$', ')'))
        WHEN h LIKE '%withheld for an engine fault%' THEN jsonb_build_object('code', 'ENGINE_FAULT', 'detail', regexp_replace(h, '\): .*$', ')'))
        WHEN h LIKE '%the executor held the day (too many faulted slots%' THEN jsonb_build_object('code', 'ENGINE_FAULT', 'detail', regexp_replace(h, ' \(too many faulted slots.*$', ' (too many faulted slots)'))
        WHEN l IS NOT NULL THEN jsonb_build_object('code', 'ENGINE_FAULT', 'detail', l)
        ELSE jsonb_build_object('code', 'FAILED', 'detail', COALESCE(NULLIF(left(split_part(message, E'\n', 1), 300), ''), 'no reason recorded'))
    END
    FROM (SELECT (regexp_match(message, '(?:^|: )held: ([^\r\n]*)'))[1] AS h,
                 (regexp_match(message, '([0-9]+ of [0-9]+ objects'' day-local derived records faulted \(more than [^)\r\n]*\))'))[1] AS l) x
$$;
