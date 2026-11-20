// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! machina-dbtool: copy a controller's SQLite database into PostgreSQL, and check the copy.
//!
//!   machina-dbtool copy   --from sqlite:///var/lib/machina/controller.db --to postgres://machina:pw@host/machina [--yes] [--force]
//!   machina-dbtool verify --from ... --to ...
//!
//! Stop the controller first. `copy` migrates the target (creating the schema if it is empty), refuses a target that already holds
//! machines, hosts, tasks, users or audit records unless `--force`, replaces the target's seeded defaults with the source's rows, loads
//! tables in foreign-key order inside one transaction, resets the identity counters, and compares row counts. Without `--yes` it
//! prints what it would do and changes nothing. The SQLite file is opened read-only.

mod convert;

use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use convert::{convert, dependency_order, PgKind, Source, Target};
use futures_util::TryStreamExt;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{PgPool, Row, SqlitePool};

#[derive(Parser)]
#[command(name = "machina-dbtool", about = "Copy a Machina controller's SQLite database into PostgreSQL")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Copy every table from SQLite into PostgreSQL
    Copy {
        /// SQLite database (sqlite:///path or a plain path)
        #[arg(long)]
        from: String,
        /// PostgreSQL database (postgres://user:password@host:5432/db)
        #[arg(long)]
        to: String,
        /// Actually do it (without this, only the plan is printed)
        #[arg(long)]
        yes: bool,
        /// Replace a target that already holds data
        #[arg(long)]
        force: bool,
    },
    /// Compare the row count of every table in the two databases
    Verify {
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
    },
}

/// Tables that hold operational data; a target where any of them has rows is not a fresh database.
const GUARD_TABLES: [&str; 5] = ["vms", "hosts", "tasks", "users", "audit_logs"];

fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

async fn open_sqlite(from: &str) -> Result<SqlitePool> {
    let url = if from.starts_with("sqlite:") { from.to_string() } else { format!("sqlite://{from}") };
    let options = SqliteConnectOptions::from_str(&url)?.read_only(true).create_if_missing(false);
    SqlitePoolOptions::new().max_connections(2).connect_with(options).await.with_context(|| format!("cannot open the SQLite database {from}"))
}

async fn open_pg(to: &str) -> Result<PgPool> {
    if !(to.starts_with("postgres://") || to.starts_with("postgresql://")) {
        bail!("--to must be a postgres:// URL");
    }
    PgPoolOptions::new().max_connections(4).connect_with(PgConnectOptions::from_str(to)?).await.context("cannot connect to PostgreSQL")
}

async fn source_tables(sqlite: &SqlitePool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name")
        .fetch_all(sqlite)
        .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
}

async fn target_tables(pg: &PgPool) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT table_name::text FROM information_schema.tables WHERE table_schema = current_schema() AND table_type = 'BASE TABLE' \
         AND table_name <> '_sqlx_migrations' ORDER BY table_name",
    )
    .fetch_all(pg)
    .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>(0)).collect())
}

async fn count_sqlite(sqlite: &SqlitePool, table: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {}", q(table))).fetch_one(sqlite).await?)
}

async fn count_pg(pg: &PgPool, table: &str) -> Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {}", q(table))).fetch_one(pg).await?)
}

/// Everything the copy needs to know about the target's columns, read before the transaction opens (information_schema queries on
/// another connection would wait for the table locks the transaction takes).
type Columns = std::collections::HashMap<String, Vec<PgColumn>>;

struct PgColumn {
    name: String,
    kind: PgKind,
    identity: bool,
    required: bool,
}

async fn pg_columns(pg: &PgPool, table: &str) -> Result<Vec<PgColumn>> {
    let rows = sqlx::query(
        "SELECT column_name::text, data_type::text, is_identity::text, is_nullable::text, column_default::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = $1 ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pg)
    .await?;
    rows.iter()
        .map(|r| {
            let name: String = r.get(0);
            let data_type: String = r.get(1);
            let identity = r.get::<String, _>(2) == "YES";
            let nullable = r.get::<String, _>(3) == "YES";
            let has_default = r.get::<Option<String>, _>(4).is_some();
            Ok(PgColumn { kind: PgKind::from_data_type(&data_type).with_context(|| format!("{table}.{name}"))?, name, identity, required: !nullable && !has_default })
        })
        .collect()
}

/// One value, read by its real storage class (`typeof()`, selected next to every column): SQLite lets a TEXT-declared column hold a
/// BLOB (metric subjects hold a machine's 16-byte id), and a declared type cannot tell which one a given row holds.
fn read_value(row: &sqlx::sqlite::SqliteRow, column: usize) -> Result<Source> {
    let class: String = row.try_get(column * 2 + 1)?;
    let raw = || row.try_get_raw(column * 2);
    Ok(match class.as_str() {
        "null" => Source::Null,
        "integer" => Source::Int(<i64 as sqlx::Decode<sqlx::Sqlite>>::decode(raw()?).map_err(|e| anyhow!("{e}"))?),
        "real" => Source::Real(<f64 as sqlx::Decode<sqlx::Sqlite>>::decode(raw()?).map_err(|e| anyhow!("{e}"))?),
        "text" => {
            // The live data has a 16-byte machine id stored with the TEXT class (metric subjects); keep it as the id it is.
            let bytes = <Vec<u8> as sqlx::Decode<sqlx::Sqlite>>::decode(raw()?).map_err(|e| anyhow!("{e}"))?;
            match String::from_utf8(bytes) {
                Ok(t) if !(t.len() == 16 && !t.bytes().all(|b| b.is_ascii_graphic())) => Source::Text(t),
                Ok(t) => Source::Blob(t.into_bytes()),
                Err(e) => Source::Blob(e.into_bytes()),
            }
        }
        "blob" => Source::Blob(<Vec<u8> as sqlx::Decode<sqlx::Sqlite>>::decode(raw()?).map_err(|e| anyhow!("{e}"))?),
        other => bail!("unexpected SQLite storage class {other}"),
    })
}

/// Copy one table. Returns the number of rows written.
async fn copy_table(sqlite: &SqlitePool, tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, pg_cols: &[PgColumn], table: &str) -> Result<i64> {
    let source_cols: Vec<String> = sqlx::query(&format!("PRAGMA table_info({})", q(table))).fetch_all(sqlite).await?.iter().map(|r| r.get::<String, _>(1)).collect();
    let cols: Vec<&PgColumn> = pg_cols.iter().filter(|c| !c.identity && source_cols.contains(&c.name)).collect();
    for c in pg_cols.iter().filter(|c| !c.identity && c.required && !source_cols.contains(&c.name)) {
        bail!("{table}.{} is required in PostgreSQL but the SQLite table has no such column", c.name);
    }
    if cols.is_empty() {
        return Ok(0);
    }
    let order = if pg_cols.iter().any(|c| c.identity) { " ORDER BY rowid" } else { "" }; // keeps `seq` in insertion order
    let select = format!(
        "SELECT {} FROM {}{order}",
        cols.iter().map(|c| format!("{n}, typeof({n})", n = q(&c.name))).collect::<Vec<_>>().join(", "),
        q(table)
    );
    let max_rows = (30_000 / cols.len()).clamp(1, 500);
    let insert_head = format!("INSERT INTO {} ({}) VALUES ", q(table), cols.iter().map(|c| q(&c.name)).collect::<Vec<_>>().join(", "));

    let mut rows = sqlx::query(&select).fetch(sqlite);
    let mut batch: Vec<Vec<Target>> = Vec::with_capacity(max_rows);
    let mut written = 0i64;
    let mut n = 0i64;
    while let Some(row) = rows.try_next().await? {
        n += 1;
        let mut out = Vec::with_capacity(cols.len());
        for (i, c) in cols.iter().enumerate() {
            let source = read_value(&row, i).with_context(|| format!("{table}.{} row {n}", c.name))?;
            out.push(convert(source, c.kind).with_context(|| format!("{table}.{} row {n}", c.name))?);
        }
        batch.push(out);
        if batch.len() == max_rows {
            written += flush(tx, &insert_head, cols.len(), &mut batch).await.with_context(|| format!("writing {table}"))?;
        }
    }
    written += flush(tx, &insert_head, cols.len(), &mut batch).await.with_context(|| format!("writing {table}"))?;
    Ok(written)
}

async fn flush(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, head: &str, width: usize, batch: &mut Vec<Vec<Target>>) -> Result<i64> {
    if batch.is_empty() {
        return Ok(0);
    }
    let mut sql = String::from(head);
    for r in 0..batch.len() {
        if r > 0 {
            sql.push(',');
        }
        sql.push('(');
        for c in 0..width {
            if c > 0 {
                sql.push(',');
            }
            sql.push_str(&format!("${}", r * width + c + 1));
        }
        sql.push(')');
    }
    let mut query = sqlx::query(&sql);
    for row in batch.iter() {
        for v in row {
            query = match v {
                Target::Uuid(x) => query.bind(*x),
                Target::Bool(x) => query.bind(*x),
                Target::Int(x) => query.bind(*x),
                Target::Double(x) => query.bind(*x),
                Target::Text(x) => query.bind(x.clone()),
            };
        }
    }
    query.execute(&mut **tx).await?;
    let n = batch.len() as i64;
    batch.clear();
    Ok(n)
}

async fn plan(sqlite: &SqlitePool, pg: &PgPool) -> Result<(Vec<String>, Vec<String>)> {
    let source = source_tables(sqlite).await?;
    let target = target_tables(pg).await?;
    let missing: Vec<&String> = source.iter().filter(|t| !target.contains(t)).collect();
    if !missing.is_empty() {
        bail!(
            "the SQLite database has tables the PostgreSQL schema does not ({}); the PostgreSQL controller is older than the SQLite one, or the schemas have diverged. Nothing was copied.",
            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    let edges: Vec<(String, String)> = sqlx::query(
        "SELECT (SELECT relname::text FROM pg_class WHERE oid = c.conrelid), (SELECT relname::text FROM pg_class WHERE oid = c.confrelid) \
         FROM pg_constraint c WHERE c.contype = 'f' AND c.connamespace = (SELECT oid FROM pg_namespace WHERE nspname = current_schema())",
    )
    .fetch_all(pg)
    .await?
    .iter()
    .map(|r| (r.get::<String, _>(0), r.get::<String, _>(1)))
    .collect();
    Ok((dependency_order(&target, &edges)?, source))
}

async fn copy(from: &str, to: &str, yes: bool, force: bool) -> Result<()> {
    let sqlite = open_sqlite(from).await?;
    let pg = open_pg(to).await?;
    println!("migrating the PostgreSQL schema ...");
    sqlx::migrate!("../controller/migrations_pg").run(&pg).await.context("could not apply the PostgreSQL schema")?;
    let (order, source) = plan(&sqlite, &pg).await?;

    if !force {
        for t in GUARD_TABLES {
            if order.iter().any(|x| x == t) && count_pg(&pg, t).await? > 0 {
                bail!("the PostgreSQL database already holds data ({t} is not empty). Use a new database, or --force to replace its contents.");
            }
        }
    }
    println!("{:<40} {:>10}", "table", "SQLite rows");
    let mut total = 0;
    for t in &order {
        let n = if source.contains(t) { count_sqlite(&sqlite, t).await? } else { 0 };
        total += n;
        if n > 0 {
            println!("{t:<40} {n:>10}");
        }
    }
    println!("{} tables, {total} rows to copy. The target's seeded defaults are replaced by the source's rows.", order.len());
    if !yes {
        println!("dry run: nothing changed. Stop the controller, then run again with --yes.");
        return Ok(());
    }

    let mut columns: Columns = Columns::new();
    for t in &order {
        columns.insert(t.clone(), pg_columns(&pg, t).await?);
    }
    let mut tx = pg.begin().await?;
    let all = order.iter().map(|t| q(t)).collect::<Vec<_>>().join(", ");
    sqlx::query(&format!("TRUNCATE {all} RESTART IDENTITY CASCADE")).execute(&mut *tx).await?;
    for t in &order {
        if !source.contains(t) {
            continue;
        }
        let n = copy_table(&sqlite, &mut tx, &columns[t], t).await?;
        if n > 0 {
            println!("copied {t:<38} {n:>10}");
        }
    }
    // identity counters continue after the highest copied value
    for t in &order {
        if columns[t].iter().any(|c| c.identity && c.name == "seq") {
            sqlx::query(&format!(
                "SELECT setval(pg_get_serial_sequence('{}', 'seq'), COALESCE((SELECT MAX(seq) FROM {}), 1), (SELECT COUNT(*) > 0 FROM {}))",
                t.replace('\'', "''"),
                q(t),
                q(t)
            ))
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    println!("committed. Checking row counts ...");
    verify(from, to).await
}

async fn verify(from: &str, to: &str) -> Result<()> {
    let sqlite = open_sqlite(from).await?;
    let pg = open_pg(to).await?;
    let source = source_tables(&sqlite).await?;
    let target = target_tables(&pg).await?;
    let mut bad = 0;
    let mut checked = 0;
    for t in &source {
        if !target.contains(t) {
            println!("MISSING in PostgreSQL: {t}");
            bad += 1;
            continue;
        }
        let (a, b) = (count_sqlite(&sqlite, t).await?, count_pg(&pg, t).await?);
        checked += 1;
        if a != b {
            println!("DIFFERENT {t}: SQLite {a}, PostgreSQL {b}");
            bad += 1;
        }
    }
    if bad > 0 {
        return Err(anyhow!("{bad} table(s) differ"));
    }
    println!("verified: {checked} tables have the same row count in both databases");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Copy { from, to, yes, force } => copy(&from, &to, yes, force).await,
        Cmd::Verify { from, to } => verify(&from, &to).await,
    }
}
