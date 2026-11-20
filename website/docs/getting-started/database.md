---
sidebar_position: 3
title: SQLite or PostgreSQL
description: Choose the controller's database, and move from SQLite to PostgreSQL with one command.
---

# SQLite or PostgreSQL

![SQLite or PostgreSQL](/readme-database.jpg)

The controller keeps its state in one database. Both backends come from one source tree and are chosen when the controller is built.

| | SQLite (default) | PostgreSQL |
|---|---|---|
| Setup | None; one file, created and migrated on first start | `machinactl db setup pod\|package\|external` |
| Fits | 1 or 2 hosts, labs, evaluations | Hundreds of machines |
| Controllers | One | Several on different hosts; one leader, hand-over inside the lease |
| Backups | Online copy | `pg_dump`, daily dumps from the managed pod |
| Way back | n/a | `machinactl db setup sqlite` (the SQLite file is never touched) |

## Move to PostgreSQL

```bash
sudo machinactl db setup pod --migrate    # managed PostgreSQL 16 in Podman, copies your SQLite data
sudo machinactl db status                 # backend, login, size, migrations
sudo machinactl db backup
```

`package` installs PostgreSQL from apt or dnf; `external --url postgres://user:pw@host/db` uses your own server. At install time the same
choice is `scripts/install-platform.sh --database pod|package|external|sqlite`. `machina-dbtool` copies a snapshot by hand.

## What has been run

The PostgreSQL build passes the controller's whole test suite on PostgreSQL 16. An isolated PostgreSQL-backed controller was run live: two
controllers on one database elect one leader and hand over within the lease, and 500 machines list in under a second. A copy of the lab
host's real database loaded with identical row counts. **Not done yet:** a PostgreSQL controller managing real hosts. Until a site has
run on it, SQLite remains the safe default. [Full guide](https://github.com/zyvorai/zyvor-machina/blob/main/docs/guides/database.md) · [Claims ledger](https://github.com/zyvorai/zyvor-machina/blob/main/docs/claims.md) (C24).
