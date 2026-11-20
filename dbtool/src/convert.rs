// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Turning what SQLite stored into what a PostgreSQL column of a given type takes.
//!
//! SQLite columns hold whatever was written: a 16-byte id is a BLOB, a flag an INTEGER, a JSON document TEXT. The PostgreSQL schema
//! (`controller/migrations_pg`) gives each column a real type, so every value is converted by the *target* column's type, and a value
//! that cannot be converted is an error naming the table, column and value kind, never a silent change.

use anyhow::{anyhow, bail, Result};
use uuid::Uuid;

/// A value read from SQLite.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// A value ready to bind for a PostgreSQL column.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Uuid(Option<Uuid>),
    Bool(Option<bool>),
    Int(Option<i64>),
    Double(Option<f64>),
    Text(Option<String>),
}

/// The PostgreSQL column types the controller schema uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgKind {
    Uuid,
    Bool,
    Int,
    Double,
    Text,
}

impl PgKind {
    /// From `information_schema.columns.data_type`.
    pub fn from_data_type(data_type: &str) -> Result<PgKind> {
        Ok(match data_type {
            "uuid" => PgKind::Uuid,
            "boolean" => PgKind::Bool,
            "bigint" | "integer" | "smallint" => PgKind::Int,
            "double precision" | "real" => PgKind::Double,
            "text" | "character varying" | "character" => PgKind::Text,
            other => bail!("unsupported PostgreSQL column type {other:?}"),
        })
    }
}

fn uuid_from_blob(b: &[u8]) -> Result<Uuid> {
    Uuid::from_slice(b).map_err(|_| anyhow!("a {}-byte BLOB is not a 16-byte id", b.len()))
}

/// Convert one value for a column of `kind`.
pub fn convert(value: Source, kind: PgKind) -> Result<Target> {
    use Source::*;
    Ok(match (kind, value) {
        (PgKind::Uuid, Null) => Target::Uuid(None),
        (PgKind::Uuid, Blob(b)) => Target::Uuid(Some(uuid_from_blob(&b)?)),
        // ids kept as text, in either the plain hex or the hyphenated form
        (PgKind::Uuid, Text(t)) => Target::Uuid(Some(Uuid::parse_str(t.trim()).map_err(|_| anyhow!("{t:?} is not an id"))?)),
        (PgKind::Bool, Null) => Target::Bool(None),
        (PgKind::Bool, Int(i)) => Target::Bool(Some(i != 0)),
        (PgKind::Bool, Text(t)) => match t.to_ascii_lowercase().as_str() {
            "1" | "true" | "t" => Target::Bool(Some(true)),
            "0" | "false" | "f" | "" => Target::Bool(Some(false)),
            other => bail!("{other:?} is not a boolean"),
        },
        (PgKind::Int, Null) => Target::Int(None),
        (PgKind::Int, Int(i)) => Target::Int(Some(i)),
        (PgKind::Int, Real(r)) if r.fract() == 0.0 => Target::Int(Some(r as i64)),
        (PgKind::Int, Text(t)) => Target::Int(Some(t.trim().parse().map_err(|_| anyhow!("{t:?} is not an integer"))?)),
        (PgKind::Double, Null) => Target::Double(None),
        (PgKind::Double, Real(r)) => Target::Double(Some(r)),
        (PgKind::Double, Int(i)) => Target::Double(Some(i as f64)),
        (PgKind::Double, Text(t)) => Target::Double(Some(t.trim().parse().map_err(|_| anyhow!("{t:?} is not a number"))?)),
        (PgKind::Text, Null) => Target::Text(None),
        (PgKind::Text, Text(t)) => Target::Text(Some(t)),
        // a machine's id stored in a text column (metric subjects) becomes its canonical text, as the PostgreSQL build writes it
        (PgKind::Text, Blob(b)) if b.len() == 16 => Target::Text(Some(uuid_from_blob(&b)?.to_string())),
        // `pool:` + the pool's 16-byte id (a storage pool's metric subject): the prefix stays, the id becomes canonical text
        (PgKind::Text, Blob(b)) if b.len() > 16 && b[..b.len() - 16].iter().all(|c| c.is_ascii_graphic()) && b[b.len() - 17] == b':' => {
            let (prefix, id) = b.split_at(b.len() - 16);
            Target::Text(Some(format!("{}{}", String::from_utf8_lossy(prefix), uuid_from_blob(id)?)))
        }
        (PgKind::Text, Int(i)) => Target::Text(Some(i.to_string())),
        (PgKind::Text, Real(r)) => Target::Text(Some(r.to_string())),
        (k, v) => bail!("cannot store a {} in a {k:?} column", kind_name(&v)),
    })
}

fn kind_name(v: &Source) -> String {
    match v {
        Source::Null => "NULL".into(),
        Source::Int(_) => "INTEGER".into(),
        Source::Real(_) => "REAL".into(),
        Source::Text(_) => "TEXT".into(),
        Source::Blob(b) => format!("{}-byte BLOB", b.len()),
    }
}

/// Order tables so every table comes after the tables it references (foreign keys). `edges` are (table, references).
/// Returns an error naming the tables left over when there is a cycle.
pub fn dependency_order(tables: &[String], edges: &[(String, String)]) -> Result<Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut needs: BTreeMap<&str, BTreeSet<&str>> = tables.iter().map(|t| (t.as_str(), BTreeSet::new())).collect();
    for (t, r) in edges {
        if t != r && needs.contains_key(t.as_str()) && needs.contains_key(r.as_str()) {
            needs.get_mut(t.as_str()).unwrap().insert(r.as_str());
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    while out.len() < tables.len() {
        let ready: Vec<&str> = needs.iter().filter(|(t, n)| !done.contains(**t) && n.iter().all(|r| done.contains(r))).map(|(t, _)| *t).collect();
        if ready.is_empty() {
            let left: Vec<&str> = needs.keys().filter(|t| !done.contains(**t)).copied().collect();
            bail!("foreign keys form a cycle among: {}", left.join(", "));
        }
        for t in ready {
            done.insert(t);
            out.push(t.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixteen_byte_blobs_become_uuids_and_text_ids_parse_in_both_forms() {
        let id = Uuid::new_v4();
        assert_eq!(convert(Source::Blob(id.as_bytes().to_vec()), PgKind::Uuid).unwrap(), Target::Uuid(Some(id)));
        assert_eq!(convert(Source::Text(id.simple().to_string()), PgKind::Uuid).unwrap(), Target::Uuid(Some(id)));
        assert_eq!(convert(Source::Text(id.to_string()), PgKind::Uuid).unwrap(), Target::Uuid(Some(id)));
        assert_eq!(convert(Source::Null, PgKind::Uuid).unwrap(), Target::Uuid(None));
        assert!(convert(Source::Blob(vec![1, 2, 3]), PgKind::Uuid).is_err());
        assert!(convert(Source::Text("web-1".into()), PgKind::Uuid).is_err());
    }

    #[test]
    fn flags_numbers_and_text() {
        assert_eq!(convert(Source::Int(1), PgKind::Bool).unwrap(), Target::Bool(Some(true)));
        assert_eq!(convert(Source::Int(0), PgKind::Bool).unwrap(), Target::Bool(Some(false)));
        assert_eq!(convert(Source::Int(7), PgKind::Int).unwrap(), Target::Int(Some(7)));
        assert_eq!(convert(Source::Real(2.0), PgKind::Int).unwrap(), Target::Int(Some(2)));
        assert!(convert(Source::Real(2.5), PgKind::Int).is_err());
        assert_eq!(convert(Source::Int(3), PgKind::Double).unwrap(), Target::Double(Some(3.0)));
        assert_eq!(convert(Source::Text("x".into()), PgKind::Text).unwrap(), Target::Text(Some("x".into())));
        assert!(convert(Source::Text("x".into()), PgKind::Bool).is_err());
    }

    #[test]
    fn a_machine_id_stored_in_a_text_column_keeps_its_canonical_text() {
        let id = Uuid::new_v4();
        assert_eq!(convert(Source::Blob(id.as_bytes().to_vec()), PgKind::Text).unwrap(), Target::Text(Some(id.to_string())));
        // `pool:` followed by the raw id, as the sampler stored it
        let mut pool = b"pool:".to_vec();
        pool.extend_from_slice(id.as_bytes());
        assert_eq!(convert(Source::Blob(pool), PgKind::Text).unwrap(), Target::Text(Some(format!("pool:{id}"))));
        // another BLOB in a text column is refused rather than mangled
        assert!(convert(Source::Blob(vec![0xff; 5]), PgKind::Text).is_err());
    }

    #[test]
    fn column_types_map() {
        assert_eq!(PgKind::from_data_type("uuid").unwrap(), PgKind::Uuid);
        assert_eq!(PgKind::from_data_type("bigint").unwrap(), PgKind::Int);
        assert_eq!(PgKind::from_data_type("boolean").unwrap(), PgKind::Bool);
        assert!(PgKind::from_data_type("jsonb").is_err());
    }

    #[test]
    fn tables_load_after_the_tables_they_reference() {
        let t = |s: &str| s.to_string();
        let tables = vec![t("vms"), t("hosts"), t("clusters"), t("tasks")];
        let edges = vec![(t("vms"), t("hosts")), (t("hosts"), t("clusters")), (t("tasks"), t("hosts")), (t("hosts"), t("hosts"))];
        let order = dependency_order(&tables, &edges).unwrap();
        let pos = |n: &str| order.iter().position(|x| x == n).unwrap();
        assert!(pos("clusters") < pos("hosts") && pos("hosts") < pos("vms") && pos("hosts") < pos("tasks"));
        let cycle = vec![(t("a"), t("b")), (t("b"), t("a"))];
        assert!(dependency_order(&[t("a"), t("b")], &cycle).is_err());
    }
}
