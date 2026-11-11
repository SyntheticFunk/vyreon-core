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

`vyreon-core serve` runs the HTTP service (the worker API, `crates/core-api/src/worker.rs`);
`vyreon-core calendar load`, `schedule` and `queue` manage work. Every command and setting is listed
at the top of `crates/vyreon-core/src/main.rs`.

`vyreon-core migrate` brings a database to the current migrations (`VYREON_CORE_PG` or
`VYREON_CORE_PG_FILE`). Migrations are `crates/core-db/migrations/NNNN_name.sql`; an applied one is
never edited.
