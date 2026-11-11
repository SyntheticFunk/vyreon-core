# vyreon-core

The database, API, job queue and cluster around the Vyreon engine. See `AGENTS.md` and `docs/MILESTONES.md`.

## Development

```bash
cargo test                      # unit tests
docker run -d --name vyreon-core-test -e POSTGRES_PASSWORD=test -p 127.0.0.1:55432:5432 postgres:16
VYREON_CORE_TEST_PG="host=127.0.0.1 port=55432 user=postgres password=test dbname=postgres" \
  cargo test -- --ignored       # database tests; each creates and drops its own database
```

`vyreon-core universe load <PROFILE.json> <dir>` stores a profile's members and ticker history from
saved Intrinio responses; `vyreon-core universe on <profile> <date>` lists them for a date.

`vyreon-core migrate` brings a database to the current migrations (`VYREON_CORE_PG` or
`VYREON_CORE_PG_FILE`). Migrations are `crates/core-db/migrations/NNNN_name.sql`; an applied one is
never edited.
