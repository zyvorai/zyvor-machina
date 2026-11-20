// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! A fresh, fully migrated database for one test, on whichever backend this build uses. Not part of the public API: it is `pub`
//! only so the integration tests in `controller/tests` can use it too.
//!
//! SQLite: a private in-memory database. PostgreSQL: a database cloned from a migrated template on the server named by
//! `TEST_DATABASE_URL` (the user needs CREATEDB), so tests run in parallel without seeing each other. Databases are left
//! behind; drop the `machina_t_*` and `machina_tpl_*` ones with `scripts/db/pg-test-clean.sh`.

use super::DbPool;

#[cfg(feature = "sqlite")]
pub async fn pool() -> DbPool {
    // max_connections(1) is load-bearing: sqlx opens `sqlite::memory:` with a PRIVATE cache, so every pooled connection is a
    // separate empty database and only the first one would ever see the migrated schema.
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    super::migrate(&pool).await.expect("migrate failed");
    pool
}

#[cfg(feature = "postgres")]
pub async fn pool() -> DbPool {
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tokio::sync::OnceCell;

    // Migrating ~150 tables and 300 indexes for every test is slow, so each test process migrates one template database and each
    // test clones it (`CREATE DATABASE ... TEMPLATE`, a file copy) into a database of its own.
    static TEMPLATE: OnceCell<String> = OnceCell::const_new();
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("the PostgreSQL build's tests need TEST_DATABASE_URL, for example postgres://machina:machina@127.0.0.1:5432/machina_test");
    let base = PgConnectOptions::from_str(&url).expect("TEST_DATABASE_URL is not a valid URL");
    let admin = || PgPoolOptions::new().max_connections(1).connect_with(base.clone());

    let template = TEMPLATE
        .get_or_init(|| async {
            let name = format!("machina_tpl_{}_{}", std::process::id(), uuid::Uuid::new_v4().simple());
            let admin = admin().await.expect("cannot reach TEST_DATABASE_URL");
            sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
            admin.close().await;
            let pool = PgPoolOptions::new().max_connections(2).connect_with(base.clone().database(&name)).await.unwrap();
            super::migrate(&pool).await.expect("migrate failed");
            pool.close().await; // a template database must have no open connections
            name
        })
        .await
        .clone();

    let name = format!("machina_t_{}", uuid::Uuid::new_v4().simple());
    let admin = admin().await.expect("cannot reach TEST_DATABASE_URL");
    sqlx::query(&format!("CREATE DATABASE {name} TEMPLATE {template}")).execute(&admin).await.unwrap();
    admin.close().await;
    PgPoolOptions::new().max_connections(4).connect_with(base.database(&name)).await.unwrap()
}
