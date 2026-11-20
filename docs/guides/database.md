# Choosing the controller's database: SQLite or PostgreSQL

## What it is
The controller keeps all of its state in one database. Two backends are supported from one source tree, chosen when the
controller is built:

- **SQLite (default).** Embedded, zero setup, one file. The right choice for 1 or 2 machines and for evaluations.
- **PostgreSQL.** For large fleets (hundreds of machines), several controllers on different hosts sharing one state store,
  and sites that already run and back up a PostgreSQL service. SQLite has a single writer and one file; PostgreSQL has neither
  limit.

![SQLite or PostgreSQL: start embedded, grow into a shared database](../ux/readme-database.jpg)

| | SQLite (default) | PostgreSQL |
|---|---|---|
| Setup | None; the file is created and migrated on first start | `machinactl db setup pod\|package\|external` |
| Machines | 1 or 2 hosts, evaluations, labs | Hundreds |
| Controllers | One | Several on different hosts, one leader, hand-over inside the lease |
| Writers | One | Many |
| Backups | Online copy (`machinactl db backup`, needs `sqlite3`) | `pg_dump`, daily dumps from the managed pod |
| Way back | n/a | `machinactl db setup sqlite` (the SQLite file is never touched by a switch) |

**Status (what has actually been run).** The PostgreSQL build passes the controller's whole test suite against PostgreSQL 16, and an
isolated PostgreSQL-backed controller was run live on the lab host: login, security groups and rules, tags, key pairs, alarms,
the EC2 endpoint with boto3, 500 machines listed in under a second, **two controllers on one database** (one leader; killing it
hands over within the lease and raises the epoch), and recovery after the PostgreSQL server restarted ([claims ledger](../claims.md),
C24). The managed Postgres pod (`machina-db setup pod`) was set up, queried and backed up live. **Moving an existing site** is built: `machina-dbtool` copied a snapshot of the lab host's real controller database (144 tables, about
36,000 rows: machines, 19,000 tasks, audit and SOC history) into PostgreSQL with identical row counts, and a PostgreSQL controller
booted on that copy listed the same 4 machines. **Not done yet:** a PostgreSQL controller managing real hosts, `setup package` (written,
never run), the installer flag end to end, and `restore`. Until a site has run on it, SQLite remains the default and the safe choice.

## Configure
**SQLite (the default, nothing to do).** `DATABASE_URL` is `sqlite:///var/lib/machina/controller.db`; the file is created and migrated on
first start.

**PostgreSQL, with the helper.** `machina-db` (also `machinactl db ...`) sets the database up, writes `DATABASE_URL` into
`/etc/default/machina-platform`, points the controller's systemd unit at its PostgreSQL build (`machina-controller-pg`) and restarts it.
It never moves data: a PostgreSQL controller starts empty.

| Command | What it does |
|---|---|
| `machinactl db setup pod [--port 5432]` | A managed PostgreSQL 16 in a Podman container (systemd quadlet `machina-postgres`), data in `/var/lib/machina/postgres`, generated password in `/etc/machina/postgres.password`, bound to `127.0.0.1`, daily dumps to `/var/backups/machina/db`. Needs `podman`. |
| `machinactl db setup package` | PostgreSQL from `apt` or `dnf` (creates the `machina` role and database), same daily dumps. |
| `machinactl db setup external --url postgres://user:pw@host:5432/db` | Your own server (RDS, Patroni, ...). The login is checked with `psql` before anything is changed. |
| `machinactl db setup sqlite` | Back to the embedded default (a switch to PostgreSQL never touches the SQLite file, so this is a real way back). |
| `... --migrate` (on `pod`, `package` or `external`) | Also copy the current SQLite data into the new PostgreSQL before the controller restarts on it. |
| `machinactl db status` | Backend, reachability, login, size, migrations applied, what the controller reports. |
| `machinactl db backup [--out DIR] [--keep DAYS]` | `pg_dump` (PostgreSQL) or an online copy (SQLite, needs `sqlite3`). |
| `machinactl db restore FILE --yes` | `pg_restore` a dump (stops the controller while it runs). |

At install time the same choice is one flag: `scripts/install-platform.sh --database pod|package|external|sqlite [--database-url URL] [--database-migrate]`.

**Moving an existing SQLite site to PostgreSQL.** The easy way is `sudo machinactl db setup pod --migrate` (or `package` / `external`): it sets
the database up, stops the controller, copies, compares row counts and restarts the controller on PostgreSQL. By hand, with the
controller stopped and a new, empty PostgreSQL database:

```bash
machina-dbtool copy --from sqlite:///var/lib/machina/controller.db --to postgres://machina:PW@host:5432/machina          # dry run: prints the plan
machina-dbtool copy --from sqlite:///var/lib/machina/controller.db --to postgres://machina:PW@host:5432/machina --yes    # copies, then verifies
machina-dbtool verify --from ... --to ...                                                                                  # row counts again
```
It opens the SQLite file read-only, applies the PostgreSQL schema, refuses a target that already holds machines, hosts, tasks, users
or audit records (unless `--force`), loads tables in foreign-key order in one transaction (all or nothing), converts ids, flags and
timestamps by the target column's type, keeps insertion order where the code depends on it, and fails with the table, column and row
when a value cannot be converted. Run it in a quiet moment: tasks that are mid-flight when the controller stops are copied as they are.

**PostgreSQL, by hand (development).** Build the controller with the `postgres` feature on a Linux host and give it a `postgres://` URL.
The database and role must exist; the controller creates the tables itself.

```bash
make release-pg                                   # target/release/machina-controller-pg
DATABASE_URL=postgres://machina:PASSWORD@db.internal:5432/machina ./target/release/machina-controller-pg
```

| Setting | Meaning |
|---|---|
| `DATABASE_URL` | `sqlite://...` for the default build, `postgres://user:password@host:port/db` for the PostgreSQL build. A build refuses a URL for the other backend and says which binary to run. Passwords are never written to logs. |
| `MACHINA_DB_MAX_CONNECTIONS` | PostgreSQL connection pool size, default 20. (SQLite stays at 4 on purpose: it has one writer.) |

**Trying `machina-db` safely.** Put `--sandbox DIR` first (`sudo machina-db --sandbox /var/tmp/zz setup pod --port 15440`): every file lives
under `DIR`, the unit and container are named `zz-machina-postgres`, and the controller is never touched. Environment-variable
overrides do not survive `sudo`, so do not rely on them.

## Use
Nothing changes for operators or API clients: the REST API, the web UI and `machinactl` behave the same on both backends. The
controller reports which one it runs (planned: in `GET /api/v1/health` as `database_backend`).

For contributors, the rules that keep both backends working are in [CLAUDE.md](../../CLAUDE.md#database-backends): every query goes
through `crate::db::query*`, every schema change is written twice (`controller/migrations` and `controller/migrations_pg`), and
serialized check-then-write sections use `crate::db::begin_write`.

## Check it works
```bash
# SQLite (default build): the suite needs nothing else
cargo test -p machina-controller

# PostgreSQL build: point the tests at a server where the user may create databases
TEST_DATABASE_URL=postgres://machina:machina@127.0.0.1:5432/machina_test \
  cargo test -p machina-controller --no-default-features --features postgres
scripts/db/pg-test-clean.sh     # drop the per-test databases afterwards
```
Each test clones a migrated template database, so the PostgreSQL suite runs in a couple of minutes. Run the Rust build and tests on
a Linux host, not on macOS.

## Limits
- No site has run on PostgreSQL yet. Do not choose it for production until the live proof in the claims ledger covers real hosts.
- A PostgreSQL controller starts empty unless you use `--migrate` or `machina-dbtool`. The copy is a one-way snapshot: changes made on SQLite afterwards are not carried over, and going back to SQLite does not bring PostgreSQL-side changes with it.
- `machina-db setup package` and the installer's `--database` flag have not been run end to end.
- The PostgreSQL build uses a patched copy of the `sqlx-postgres` driver (`vendor/sqlx-postgres/MACHINA_PATCHES.md`) that reads
  integers of any width, JSON and timestamps stored as TEXT, as SQLite does. It is small, reviewed, and must be re-applied when
  sqlx is upgraded.
- A few SQLite-only tests (hand-built tables, write-lock races) run on the SQLite build only. Code paths no test exercises can still
  hold a SQLite-ism: live runs found two that all unit tests missed (a security-group rule naming another group; `/projects` with a bare
  column next to `GROUP BY`). `scripts/db/check_pg_sql.py` now parse-checks every literal SQL statement against the PostgreSQL schema in CI
  (it found 8 more), but SQL built with `format!()` and runtime type mismatches are only covered by tests and live runs.
- Backup and high availability differ: the Litestream pod in [controller-ha.md](../controller-ha.md) is for SQLite; PostgreSQL
  uses `pg_dump` (the daily timer), WAL archiving and the database's own replication.
