# Local patches to sqlx-postgres 0.8.6

Vendored from crates.io (MIT OR Apache-2.0), used through `[patch.crates-io]` in the workspace `Cargo.toml`. Only the
PostgreSQL build of `machina-controller` compiles it.

**Why.** The controller was written against SQLite, which has no column widths and no JSON, timestamp or boolean types: an
`INTEGER` is read as `i32`, `i64` or `bool`, a TEXT column as `serde_json::Value` or `DateTime<Utc>`. PostgreSQL's driver refuses
those reads ("mismatched types"), and the controller has several hundred such read sites, only some of which a test exercises.
Rather than change each one (and miss the rest), the type-compatibility checks for *reads* are relaxed here, so PostgreSQL reads
what SQLite would have returned. Writes are unchanged and strict: the schema (`controller/migrations_pg`) uses the type each
column is bound as (BOOLEAN, BIGINT, UUID, TEXT).

**What changed** (each hunk is marked `// machina`):

| File | Change |
|---|---|
| `types/int.rs` | `i16`/`i32`/`i64` read any integer column and NUMERIC (SUM() is NUMERIC) |
| `types/float.rs` | `f32`/`f64` read FLOAT4, FLOAT8, integers and NUMERIC (AVG() is NUMERIC) |
| `types/bool.rs` | `bool` also reads integer columns |
| `types/json.rs` | `Json<T>` / `serde_json::Value` read TEXT too, and are *written* as compact TEXT (not JSONB), matching what SQLite stores |
| `types/uuid.rs` | `Uuid` also reads TEXT |
| `types/chrono/datetime.rs` | `DateTime<Tz>` / `NaiveDateTime` read TEXT timestamps (RFC 3339, `YYYY-MM-DD HH:MM:SS`, ISO with `Z`) and are *written* as TEXT (RFC 3339 / `%F %T%.f`) like SQLite's driver, since every timestamp column is TEXT and PostgreSQL will not compare TEXT with a timestamp parameter |
| `types/lenient.rs` | new: shared helpers (NUMERIC to f64/i64, text timestamp parsing) |

**On a sqlx upgrade:** re-apply these hunks to the new `sqlx-postgres` (they are small), bump the version here, and run the
PostgreSQL test suite.
