// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use sqlx::{Database, FromRow};
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

pub mod dialect;
#[doc(hidden)]
pub mod testing;

#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("enable exactly one of the `sqlite` and `postgres` features of machina-controller");
#[cfg(not(any(feature = "sqlite", feature = "postgres")))]
compile_error!("enable one of the `sqlite` or `postgres` features of machina-controller");

/// The database backend the controller was built for. Everything outside this module names these aliases instead of the
/// `sqlx::Sqlite*` / `sqlx::Pg*` types, so the backend is chosen here and not in ~260 files.
#[cfg(feature = "sqlite")]
pub type Db = sqlx::Sqlite;
#[cfg(feature = "sqlite")]
pub type DbPool = sqlx::SqlitePool;
#[cfg(feature = "sqlite")]
pub type DbConn = sqlx::SqliteConnection;
#[cfg(feature = "postgres")]
pub type Db = sqlx::Postgres;
#[cfg(feature = "postgres")]
pub type DbPool = sqlx::PgPool;
#[cfg(feature = "postgres")]
pub type DbConn = sqlx::PgConnection;

/// `sqlite` or `postgres`, for health and diagnostics.
pub const BACKEND: &str = if cfg!(feature = "postgres") { "postgres" } else { "sqlite" };

/// Where the controller looks for its database when `DATABASE_URL` is not set.
#[cfg(feature = "sqlite")]
pub const DEFAULT_URL: &str = "sqlite:///var/lib/machina/controller.db";
#[cfg(feature = "postgres")]
pub const DEFAULT_URL: &str = "postgres://machina@127.0.0.1:5432/machina";

/// Prepare a statement for this backend. SQLite uses the text as written. PostgreSQL gets it rewritten once (placeholders,
/// `CURRENT_TIMESTAMP`, `INSERT OR IGNORE`, ...; see `dialect`) and cached, so the SQL in the rest of the controller stays in one form.
#[cfg(feature = "sqlite")]
fn sql(text: &str) -> &str {
    text
}

#[cfg(feature = "postgres")]
fn sql(text: &str) -> &str {
    dialect::cached_postgres(text)
}

pub fn query<DB: Database>(text: &str) -> sqlx::query::Query<'_, DB, <DB as Database>::Arguments<'_>> {
    sqlx::query(sql(text))
}

pub fn query_as<'q, DB, O>(text: &'q str) -> sqlx::query::QueryAs<'q, DB, O, <DB as Database>::Arguments<'q>>
where
    DB: Database,
    O: for<'r> FromRow<'r, <DB as Database>::Row>,
{
    sqlx::query_as(sql(text))
}

pub fn query_scalar<'q, DB, O>(text: &'q str) -> sqlx::query::QueryScalar<'q, DB, O, <DB as Database>::Arguments<'q>>
where
    DB: Database,
    (O,): for<'r> FromRow<'r, <DB as Database>::Row>,
{
    sqlx::query_scalar(sql(text))
}

/// Start a transaction that serializes against every other `begin_write` section: the check-then-write sequences (address and
/// quota allocation, tags, Elastic IPs) that must not interleave. SQLite takes its one write lock up front (`BEGIN IMMEDIATE`);
/// PostgreSQL has no such lock, so the same guarantee is a transaction-scoped advisory lock that every such section takes first.
/// One key for all of them is deliberate: correct by construction, and these sections are short control-plane operations.
#[cfg(feature = "sqlite")]
pub async fn begin_write(pool: &DbPool) -> Result<sqlx::Transaction<'static, Db>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

#[cfg(feature = "postgres")]
pub async fn begin_write(pool: &DbPool) -> Result<sqlx::Transaction<'static, Db>, sqlx::Error> {
    /// An arbitrary constant naming "machina serialized write section".
    const WRITE_SECTION_LOCK: i64 = 0x6d61_6368_696e_6101;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(WRITE_SECTION_LOCK).execute(&mut *tx).await?;
    Ok(tx)
}


// JSON documents are stored as TEXT on both backends (SQLite has no JSON type, and the controller's SQL treats them as text).
// `Json<T>` is a drop-in for `sqlx::types::Json<T>` that is written as TEXT on both backends (PostgreSQL's own `Json<T>` binds as
// JSONB, which a TEXT column rejects). Reading JSON from TEXT works through the patched driver in vendor/sqlx-postgres.
use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Json<T>(pub T);

impl<T> std::ops::Deref for Json<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T> std::ops::DerefMut for Json<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
impl<T> sqlx::Type<Db> for Json<T> {
    fn type_info() -> <Db as Database>::TypeInfo {
        <str as sqlx::Type<Db>>::type_info()
    }
    fn compatible(ty: &<Db as Database>::TypeInfo) -> bool {
        <str as sqlx::Type<Db>>::compatible(ty)
    }
}
impl<'r, T: serde::de::DeserializeOwned> sqlx::Decode<'r, Db> for Json<T> {
    fn decode(value: <Db as Database>::ValueRef<'r>) -> Result<Self, BoxDynError> {
        let text = <&str as sqlx::Decode<Db>>::decode(value)?;
        Ok(Json(serde_json::from_str(text)?))
    }
}
impl<'q, T: serde::Serialize> sqlx::Encode<'q, Db> for Json<T> {
    fn encode_by_ref(&self, buf: &mut <Db as Database>::ArgumentBuffer<'q>) -> Result<IsNull, BoxDynError> {
        <String as sqlx::Encode<'q, Db>>::encode(serde_json::to_string(&self.0)?, buf)
    }
}

/// The bind value for a machine in `metric_samples.subject` / `metric_hourly.subject`: its 16-byte id on SQLite, its canonical text
/// on PostgreSQL (the column is TEXT there because it also holds group and pool subjects, and the sampler's `INSERT ... SELECT v.id`
/// stores a uuid as that text).
#[cfg(feature = "sqlite")]
pub fn subject_id(id: Uuid) -> Uuid {
    id
}
#[cfg(feature = "postgres")]
pub fn subject_id(id: Uuid) -> String {
    id.to_string()
}

/// A database URL that is safe to log: `scheme://user:password@host/db` loses the password, anything else is returned as is.
pub fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return url.to_string() };
    let Some((auth, host)) = rest.rsplit_once('@') else { return url.to_string() };
    match auth.split_once(':') {
        Some((user, _)) => format!("{scheme}://{user}:<redacted>@{host}"),
        None => url.to_string(),
    }
}

#[cfg(feature = "sqlite")]
pub async fn connect(database_url: &str) -> anyhow::Result<DbPool> {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    if !database_url.starts_with("sqlite:") {
        anyhow::bail!(
            "this machina-controller build uses the embedded SQLite database but DATABASE_URL is {}; \
             use a sqlite:// URL, or run the PostgreSQL build (machina-controller-pg) for a postgres:// URL",
            redact_url(database_url)
        );
    }
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("foreign_keys", "ON")
        // WAL already gives durability at checkpoint boundaries, so the extra
        // fsync FULL does on every commit is unneeded belt-and-suspenders here
        // and was a real contributor to write-lock hold time under the
        // concurrent writers below (reconcile, channel_worker, webhook_worker,
        // leader election, per-request trace spans, ...).
        .pragma("synchronous", "NORMAL")
        .busy_timeout(Duration::from_secs(5));

    // Deliberately small: SQLite allows exactly one writer no matter how many
    // connections the pool hands out, so a bigger pool doesn't add write
    // throughput — it only admits more simultaneous contenders for that one
    // writer lock. Tried raising this to 16 to help concurrent readers and it
    // made things much worse (queries observed up to 69s): with 16 slots, up
    // to 16 writers (reconcile, channel_worker, webhook_worker, leader
    // election, per-request trace spans, ...) can pile onto the lock at once,
    // and once enough of those are individually stuck in their own 5s
    // busy_timeout, the pool itself saturates — new requests, including plain
    // SELECTs, then queue for a *pool connection* behind a stack of blocked
    // writers. The small pool was accidentally acting as admission control;
    // keep it small until write contention is reduced some other way (see
    // observability::record_trace's prune throttling).
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;

    Ok(pool)
}


/// Connections the PostgreSQL pool may open (`MACHINA_DB_MAX_CONNECTIONS`, default 20).
#[cfg(feature = "postgres")]
fn pg_max_connections() -> u32 {
    std::env::var("MACHINA_DB_MAX_CONNECTIONS").ok().and_then(|v| v.parse().ok()).filter(|n| *n >= 1).unwrap_or(20)
}

#[cfg(feature = "postgres")]
pub async fn connect(database_url: &str) -> anyhow::Result<DbPool> {
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    if !(database_url.starts_with("postgres://") || database_url.starts_with("postgresql://")) {
        anyhow::bail!(
            "this machina-controller build uses PostgreSQL but DATABASE_URL is {}; \
             use a postgres:// URL, or run the embedded-SQLite build (machina-controller) for a sqlite:// URL",
            redact_url(database_url)
        );
    }
    let options = PgConnectOptions::from_str(database_url)?.application_name("machina-controller");
    let pool = PgPoolOptions::new()
        .max_connections(pg_max_connections())
        .acquire_timeout(Duration::from_secs(10))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                // Timestamps are TEXT in UTC; keep any session-dependent date arithmetic in UTC too.
                sqlx::query("SET TIME ZONE 'UTC'").execute(conn).await?;
                Ok::<(), sqlx::Error>(())
            })
        })
        .connect_with(options)
        .await?;
    Ok(pool)
}
pub async fn migrate(pool: &DbPool) -> anyhow::Result<()> {
    #[cfg(feature = "sqlite")]
    sqlx::migrate!("./migrations").run(pool).await?;
    #[cfg(feature = "postgres")]
    sqlx::migrate!("./migrations_pg").run(pool).await?;
    Ok(())
}

pub async fn ensure_bootstrap(
    pool: &DbPool,
    admin_user: &str,
    admin_password: &str,
) -> anyhow::Result<()> {
    // Reap tasks left 'running' by a worker that died or was restarted mid-task.
    // Nothing else transitions running->failed, so without this they stay
    // 'running' forever (never retried, since a re-publish only matches 'pending')
    // and block reconcile from healing the affected VM.
    //
    // When a STABLE controller id is configured (MACHINA_CONTROLLER_ID), reap only
    // OUR own orphans (+ legacy NULL-owner rows) so we never fail a peer
    // controller's in-flight task in a multi-controller deployment.
    //
    // Without a configured id, this process's own id (`config.controller_id`) is a
    // fresh random value each boot (see config.rs), so claimed_by-scoping would
    // never match our OWN prior-run tasks either — we can't tell "my own orphan"
    // from "a peer's live task" by identity alone in that case. Nothing enforces
    // that MACHINA_CONTROLLER_ID is set in every multi-controller deployment, so
    // do NOT assume "unset" means "single controller, safe to fail every running
    // row": if a peer happens to also be unset, that would fail its in-flight
    // work every time any one of them restarts. Fall back to a staleness check
    // instead: only reap rows that have had no progress in a long time (or were
    // never claimed at all). A live task's `updated_at` is refreshed at claim
    // time and again by any progress update, so a peer's genuinely in-flight task
    // stays well inside the window; a truly orphaned task (worker died, no peer
    // owns it) eventually crosses it and gets recovered on a later restart. The
    // threshold is intentionally generous (some operations — backup/clone of a
    // large disk — may run a long time between progress updates) to bias toward
    // never killing live work over reaping instantly.
    // Select (rather than blind-UPDATE) so each reaped row can be run through
    // finalize_terminal_task_failure below: a task reaped here was 'running',
    // meaning it may already have taken side effects (e.g. ha.recover writing
    // the VM's new host_id) before the controller crashed. A bare status
    // write, as this used to do, skipped set_vm_error/ha.recover's host_id
    // revert/webhook dispatch entirely, leaving that state stranded forever
    // since nothing else ever transitions a 'running' row.
    let reap_rows: Vec<(Uuid, String, serde_json::Value)> = if let Some(id) =
        std::env::var("MACHINA_CONTROLLER_ID")
            .ok()
            .filter(|s| !s.is_empty())
    {
        query_as(
            "SELECT id, operation, payload FROM tasks \
                 WHERE status = 'running' AND (claimed_by = ? OR claimed_by IS NULL)",
        )
        .bind(id)
        .fetch_all(pool)
        .await?
    } else {
        query_as(
            "SELECT id, operation, payload FROM tasks \
                 WHERE status = 'running' \
                   AND (claimed_by IS NULL OR updated_at < datetime('now', '-60 minutes'))",
        )
        .fetch_all(pool)
        .await?
    };
    if !reap_rows.is_empty() {
        tracing::warn!(
            "reaping {} task(s) left in 'running' state after restart",
            reap_rows.len()
        );
        for (task_id, operation, payload) in reap_rows {
            let msg = crate::tasks::TaskMessage {
                task_id,
                operation,
                payload,
            };
            crate::tasks::worker::finalize_terminal_task_failure(
                pool,
                &msg,
                "controller restarted while task was running",
            )
            .await;
        }
    }

    // These three "check count == 0, then insert" blocks are check-then-act:
    // in a multi-controller deployment sharing one DB (see the reaper's
    // MACHINA_CONTROLLER_ID handling above), two controllers can both boot
    // against an empty DB and both observe count == 0 before either commits
    // its insert. clusters.name, users.username, and hosts(cluster_id,
    // hostname) are all UNIQUE, so a plain INSERT would make the loser crash
    // the whole ensure_bootstrap (and thus startup) on a constraint
    // violation instead of just no-op'ing. Use INSERT OR IGNORE so the loser
    // of the race silently defers to whichever controller won it, instead of
    // failing to start.
    let cluster_count: i64 = query_scalar("SELECT COUNT(*) FROM clusters")
        .fetch_one(pool)
        .await?;
    if cluster_count == 0 {
        let cluster_id = Uuid::new_v4();
        query("INSERT OR IGNORE INTO clusters (id, name) VALUES (?, ?)")
            .bind(cluster_id)
            .bind("default")
            .execute(pool)
            .await?;
    }

    let user_count: i64 = query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if user_count == 0 {
        let hash = bcrypt::hash(admin_password, bcrypt::DEFAULT_COST)?;
        query(
            "INSERT OR IGNORE INTO users (id, username, password_hash, role) VALUES (?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(admin_user)
        .bind(hash)
        .bind("admin")
        .execute(pool)
        .await?;
    }

    let host_count: i64 = query_scalar("SELECT COUNT(*) FROM hosts")
        .fetch_one(pool)
        .await?;
    if host_count == 0 {
        let cluster_id: Uuid = query_scalar::<_, Uuid>("SELECT id FROM clusters LIMIT 1")
            .fetch_one(pool)
            .await?;
        query(
            "INSERT OR IGNORE INTO hosts (id, cluster_id, hostname, address, state, agent_grpc_addr)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4())
        .bind(cluster_id)
        .bind("localhost")
        .bind("127.0.0.1")
        .bind("online")
        .bind(
            std::env::var("MACHINA_AGENT_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:50051".into()),
        )
        .execute(pool)
        .await?;
    }

    crate::engine::template_catalog::ensure_default_templates(pool).await?;

    ensure_native_projects(pool).await?;

    Ok(())
}

/// The machine's own hostname (kernel value), if it has a usable one.
fn machine_hostname() -> Option<String> {
    let h = std::fs::read_to_string("/proc/sys/kernel/hostname").ok()?;
    let h = h.trim().to_string();
    (!h.is_empty() && h != "localhost").then_some(h)
}

/// This machine's address on its default route (asking the kernel which source it would use;
/// nothing is sent). None when there is only loopback.
fn primary_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then(|| ip.to_string())
}

/// The first-boot host row is called `localhost` at `127.0.0.1`, which tells an operator nothing.
/// Give it the machine's real hostname and address (only when its agent is local and the name is
/// free), so the UI, host shell, VM network policy and load balancers see a real node.
pub async fn name_local_host(pool: &DbPool) -> anyhow::Result<()> {
    rename_local_host(pool, machine_hostname().as_deref(), primary_ip().as_deref()).await
}

pub(crate) async fn rename_local_host(
    pool: &DbPool,
    hostname: Option<&str>,
    ip: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(name) = hostname {
        query(
            "UPDATE hosts SET hostname = ?
             WHERE hostname = 'localhost' AND agent_grpc_addr LIKE '127.0.0.1:%'
               AND NOT EXISTS (SELECT 1 FROM hosts h2 WHERE h2.hostname = ?)",
        )
        .bind(name)
        .bind(name)
        .execute(pool)
        .await?;
    }
    if let (Some(name), Some(ip)) = (hostname, ip) {
        query(
            "UPDATE hosts SET address = ?
             WHERE hostname = ? AND agent_grpc_addr LIKE '127.0.0.1:%' AND address IN ('127.0.0.1', '', 'localhost')",
        )
        .bind(ip)
        .bind(name)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Backfills a real `projects` row for every distinct project name already in use
/// across `vms.project` / `project_quotas.project` (see migration 020). Existing
/// free-text project filtering elsewhere is untouched — this only gives those same
/// names a stable id for the new project-registry endpoints (`api::projects`) to
/// build membership/roles on top of. Runs every boot (INSERT OR IGNORE), matching
/// `ensure_default_templates`, so a project name introduced later via the legacy
/// free-text path gets backfilled on the next restart.
async fn ensure_native_projects(pool: &DbPool) -> anyhow::Result<()> {
    let names: Vec<String> = query_scalar(
        "SELECT DISTINCT name FROM (
            SELECT COALESCE(NULLIF(project, ''), 'default') AS name FROM vms
            UNION
            SELECT COALESCE(NULLIF(project, ''), 'default') AS name FROM project_quotas
            UNION
            SELECT 'default' AS name
        )",
    )
    .fetch_all(pool)
    .await?;

    for name in names {
        query("INSERT OR IGNORE INTO projects (id, name) VALUES (?, ?)")
            .bind(Uuid::new_v4())
            .bind(&name)
            .execute(pool)
            .await?;
    }

    Ok(())
}

pub async fn ensure_machina_db_ownership() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::redact_url;

    #[test]
    fn passwords_are_removed_from_urls_that_are_logged() {
        assert_eq!(redact_url("postgres://machina:s3cret@db.internal:5432/machina"), "postgres://machina:<redacted>@db.internal:5432/machina");
        assert_eq!(redact_url("postgres://machina@db.internal/machina"), "postgres://machina@db.internal/machina");
        assert_eq!(redact_url("sqlite:///var/lib/machina/controller.db"), "sqlite:///var/lib/machina/controller.db");
        assert_eq!(redact_url("postgres://u:p%40ss@h/d"), "postgres://u:<redacted>@h/d");
    }
}

#[cfg(test)]
mod local_host_name_tests {
    use super::*;

    #[tokio::test]
    async fn the_seeded_local_host_gets_its_real_name_and_address_once() {
        let pool = crate::db::testing::pool().await;
        let n = |name: &str| format!("SELECT COUNT(*) FROM hosts WHERE hostname = '{name}'");
        query("INSERT INTO hosts (id, hostname, address, state, agent_grpc_addr) VALUES (?, 'localhost', '127.0.0.1', 'online', '127.0.0.1:50051')")
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        rename_local_host(&pool, Some("node-7"), Some("10.1.2.3")).await.unwrap();
        let (name, addr): (String, String) = query_as("SELECT hostname, address FROM hosts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!((name.as_str(), addr.as_str()), ("node-7", "10.1.2.3"));
        // Idempotent, and a remote host called localhost is left alone.
        rename_local_host(&pool, Some("node-7"), Some("10.9.9.9")).await.unwrap();
        let c: i64 = query_scalar(&n("node-7")).fetch_one(&pool).await.unwrap();
        assert_eq!(c, 1);
        let addr: String = query_scalar("SELECT address FROM hosts").fetch_one(&pool).await.unwrap();
        assert_eq!(addr, "10.1.2.3");
    }
}
