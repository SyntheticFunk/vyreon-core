# vyreon-core

The database, API, job queue and cluster around the Vyreon engine. See `AGENTS.md` and `docs/MILESTONES.md`.

## Development

```bash
cargo test                      # unit tests
docker run -d --name vyreon-core-test --memory 1536m -e POSTGRES_PASSWORD=test -p 127.0.0.1:55432:5432 postgres:16
VYREON_CORE_TEST_PG="host=127.0.0.1 port=55432 user=postgres password=test dbname=postgres" \
  cargo test -- --ignored       # database tests; each creates and drops its own database
```

`vyreon-core universe load <PROFILE.json> <dir>` stores a profile's members and ticker history from
saved Intrinio responses; `vyreon-core universe on <profile> <date>` lists them for a date.

`vyreon-core build register <inventory.json>` registers an engine build with its slot inventory;
`vyreon-core months <first> <last>` creates the store's monthly partitions; `vyreon-core publish
<bundle>...` checks, writes, reads back and publishes day bundles (`crates/core-contract`).
`vyreon-core engine-store export <date> <dir>` writes a day of the droplet's store as bundles.
By hand, a revision computed from a base must name the base revision it read and read its day
only; `publish --vouch <bundle>...` is the operator's word that the bundles were computed from
what is published now (C-042), which an exported day's derived bundle needs.

`vyreon-core serve` runs the HTTP service: the worker API (`crates/core-api/src/worker.rs`) and the reader API
(`crates/core-api/src/reader.rs`; keys from `vyreon-core api-key create`);
`vyreon-core calendar load`, `schedule` and `queue` manage work. Every command and setting is listed
at the top of `crates/vyreon-core/src/main.rs`.

The perishable archive (C-052): workers upload a published day's cohort prints and ACT01 pulls to
Spaces through upload URLs core signs (core alone holds the key); `vyreon-core archive list [date]`.

The reference check (C-054): each worker recomputes the day it published with the reference suite
(QuantLib, TA-Lib) before deleting the day's raw inputs and sends core the report; a disagreement,
or a check that did not run, is a problem, never an unpublished day; `vyreon-core refcheck list
[first [last]]` and `refcheck show <date> [--report]`.

The owner's Discord channel (C-050) carries problems (one alert when one opens, one "Resolved"),
a 09:00 MT digest led by its verdict and a Saturday check; `vyreon-core problems`, `digest
[weekly]` and `log` show the same from the command line.

`vyreon-core migrate` brings a database to the current migrations (`VYREON_CORE_PG` or
`VYREON_CORE_PG_FILE`). Migrations are `crates/core-db/migrations/NNNN_name.sql`; an applied one is
never edited.
