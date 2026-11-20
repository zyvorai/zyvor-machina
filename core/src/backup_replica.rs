// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Replicating a backup to a second site over S3 (any S3-compatible store). Pure planning only: it
//! validates the destination, builds the `aws` arguments and checks the result by comparing the
//! sizes the store reports with the sizes on disk. The agent, on the host that holds the backup,
//! runs the commands (see `agent/src/replicate.rs`).
//!
//! Every value that reaches an `aws` argument is validated first. `aws` is an argparse tool, so a
//! value starting with `-` would be read as a flag; bucket, key and endpoint are refused if they
//! do, and keys cannot climb out of their prefix.

use serde_json::Value;

/// A destination read from a `backup_targets` row of kind `s3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Dest {
    pub bucket: String,
    pub prefix: String,
    pub endpoint_url: String,
}

fn bucket_ok(b: &str) -> bool {
    (3..=63).contains(&b.len())
        && b.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        && !b.starts_with(['-', '.'])
        && !b.ends_with(['-', '.'])
}

/// Object keys: printable ASCII without control characters, no `..` segment, no leading `/` or `-`.
pub fn validate_key(k: &str) -> Result<(), String> {
    let ok = !k.is_empty()
        && k.len() <= 1024
        && !k.starts_with(['-', '/'])
        && k.chars().all(|c| c.is_ascii_graphic() || c == ' ')
        && !k.split('/').any(|seg| seg == "..");
    if ok {
        Ok(())
    } else {
        Err(format!("invalid object key '{k}'"))
    }
}

pub fn validate_endpoint(e: &str) -> Result<(), String> {
    if e.is_empty() {
        return Ok(());
    }
    let rest = e
        .strip_prefix("https://")
        .or_else(|| e.strip_prefix("http://"))
        .ok_or("endpoint_url must be http(s)")?;
    let ok = !rest.is_empty()
        && rest.len() <= 255
        && rest.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '/' | '_' | '[' | ']')
        });
    if ok {
        Ok(())
    } else {
        Err(format!("invalid endpoint_url '{e}'"))
    }
}

impl S3Dest {
    /// From the target's `config_json`: `bucket` (required), `prefix` (default `machina`),
    /// `endpoint_url` (optional, for S3-compatible stores).
    pub fn from_config(cfg: &Value) -> Result<Self, String> {
        let bucket = cfg
            .get("bucket")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if !bucket_ok(bucket) {
            return Err("the target needs a valid S3 bucket name".into());
        }
        let prefix = cfg
            .get("prefix")
            .and_then(Value::as_str)
            .unwrap_or("machina")
            .trim()
            .trim_matches('/');
        let prefix = if prefix.is_empty() { "machina" } else { prefix };
        validate_key(prefix)?;
        let endpoint = cfg
            .get("endpoint_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        validate_endpoint(&endpoint)?;
        Ok(Self {
            bucket: bucket.to_string(),
            prefix: prefix.to_string(),
            endpoint_url: endpoint,
        })
    }

    /// `<prefix>/<name>` for a backup file or directory called `name`.
    pub fn key_for(&self, name: &str) -> Result<String, String> {
        if name.is_empty() || name.contains('/') || name == ".." || name == "." {
            return Err(format!("invalid backup name '{name}'"));
        }
        let key = format!("{}/{}", self.prefix, name);
        validate_key(&key)?;
        Ok(key)
    }
}

pub fn uri(bucket: &str, key: &str) -> String {
    format!("s3://{bucket}/{key}")
}

fn with_endpoint(mut args: Vec<String>, endpoint: &str) -> Vec<String> {
    if !endpoint.is_empty() {
        args.push("--endpoint-url".into());
        args.push(endpoint.to_string());
    }
    args
}

/// Upload a file (`cp`) or a directory (`sync`).
pub fn push_args(
    local: &str,
    is_dir: bool,
    bucket: &str,
    key: &str,
    endpoint: &str,
) -> Vec<String> {
    let dest = if is_dir {
        format!("{}/", uri(bucket, key))
    } else {
        uri(bucket, key)
    };
    let verb = if is_dir { "sync" } else { "cp" };
    with_endpoint(
        vec![
            "s3".into(),
            verb.into(),
            local.into(),
            dest,
            "--only-show-errors".into(),
        ],
        endpoint,
    )
}

/// Download a file (`cp`) or a directory (`sync`).
pub fn fetch_args(
    dest: &str,
    is_dir: bool,
    bucket: &str,
    key: &str,
    endpoint: &str,
) -> Vec<String> {
    let src = if is_dir {
        format!("{}/", uri(bucket, key))
    } else {
        uri(bucket, key)
    };
    let verb = if is_dir { "sync" } else { "cp" };
    with_endpoint(
        vec![
            "s3".into(),
            verb.into(),
            src,
            dest.into(),
            "--only-show-errors".into(),
        ],
        endpoint,
    )
}

/// List every object under `key` with its size.
pub fn ls_args(bucket: &str, key: &str, endpoint: &str) -> Vec<String> {
    with_endpoint(
        vec![
            "s3".into(),
            "ls".into(),
            uri(bucket, key),
            "--recursive".into(),
        ],
        endpoint,
    )
}

/// Parse `aws s3 ls --recursive` output: `2026-01-02 03:04:05      12345 some/key name`.
pub fn parse_ls(out: &str) -> Vec<(u64, String)> {
    out.lines()
        .filter_map(|line| {
            // Fields are separated by runs of spaces; the key is everything after the size.
            let t = line.trim_start();
            let t = t.split_once(char::is_whitespace)?.1.trim_start(); // after date
            let t = t.split_once(char::is_whitespace)?.1.trim_start(); // after time
            let (size, key) = t.split_once(char::is_whitespace)?;
            Some((size.parse().ok()?, key.trim_start().to_string()))
        })
        .collect()
}

/// Check the store holds exactly what is on disk: every local file `(relative path, size)` has an
/// object of the same size at `key` (a single file) or `key/<relative path>` (a directory), and no
/// object is missing. Returns the total bytes.
pub fn verify(
    local: &[(String, u64)],
    remote: &[(u64, String)],
    key: &str,
    is_dir: bool,
) -> Result<u64, String> {
    if local.is_empty() {
        return Err("nothing to verify: the backup has no files".into());
    }
    let mut total = 0u64;
    for (rel, size) in local {
        let want = if is_dir {
            format!("{key}/{rel}")
        } else {
            key.to_string()
        };
        match remote.iter().find(|(_, k)| *k == want) {
            None => return Err(format!("{want} is missing from the store")),
            Some((rs, _)) if rs != size => {
                return Err(format!(
                    "{want} is {rs} bytes in the store but {size} on disk"
                ))
            }
            Some(_) => total += size,
        }
    }
    Ok(total)
}

/// For a fetch: the remote objects form a directory when more than the exact key is present.
pub fn remote_is_dir(remote: &[(u64, String)], key: &str) -> bool {
    let dir_prefix = format!("{key}/");
    remote.iter().any(|(_, k)| k.starts_with(&dir_prefix))
}

/// The `(relative path, size)` list a fetched copy should have, from the store listing.
pub fn expected_files(remote: &[(u64, String)], key: &str, is_dir: bool) -> Vec<(String, u64)> {
    if is_dir {
        let dir_prefix = format!("{key}/");
        remote
            .iter()
            .filter_map(|(s, k)| k.strip_prefix(&dir_prefix).map(|r| (r.to_string(), *s)))
            .filter(|(r, _)| !r.is_empty() && !r.ends_with('/'))
            .collect()
    } else {
        remote
            .iter()
            .filter(|(_, k)| k == key)
            .map(|(s, _)| (key.rsplit('/').next().unwrap_or(key).to_string(), *s))
            .collect()
    }
}

/// One file in a checksum manifest.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestFile {
    /// Path relative to the backup (a file's own name, or `sub/disk.qcow2` inside a directory).
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// The sidecar object that holds a copy's checksums. It sits next to the data, not inside a
/// directory copy, so `aws s3 sync` of the directory never picks it up.
pub fn manifest_key(key: &str) -> String {
    format!("{key}.manifest.json")
}

pub fn manifest_json(files: &[ManifestFile]) -> String {
    serde_json::json!({ "version": 1, "files": files }).to_string()
}

pub fn parse_manifest(text: &str) -> Result<Vec<ManifestFile>, String> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| format!("the checksum manifest is damaged: {e}"))?;
    let files = v
        .get("files")
        .cloned()
        .ok_or("the checksum manifest has no file list")?;
    serde_json::from_value(files).map_err(|e| format!("the checksum manifest is damaged: {e}"))
}

/// Lowercase hex SHA-256 of a file, read in 1 MiB chunks.
pub fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// SHA-256 of a string, as lowercase hex.
pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compare what is on disk with what the manifest recorded: same files, sizes and checksums.
pub fn verify_manifest(local: &[ManifestFile], manifest: &[ManifestFile]) -> Result<(), String> {
    if manifest.is_empty() {
        return Err("the checksum manifest lists no files".into());
    }
    for want in manifest {
        match local.iter().find(|l| l.path == want.path) {
            None => return Err(format!("{} is missing", want.path)),
            Some(l) if l.size != want.size => {
                return Err(format!(
                    "{} is {} bytes, the manifest says {}",
                    want.path, l.size, want.size
                ))
            }
            Some(l) if l.sha256 != want.sha256 => {
                return Err(format!(
                    "{} does not match its recorded checksum",
                    want.path
                ))
            }
            Some(_) => {}
        }
    }
    if let Some(extra) = local
        .iter()
        .find(|l| !manifest.iter().any(|m| m.path == l.path))
    {
        return Err(format!("{} is not in the checksum manifest", extra.path));
    }
    Ok(())
}

/// Delete a copy: the object (`rm key`) or the directory (`rm key/ --recursive`), never the bare
/// prefix, which would also match sibling names like `key.manifest.json` or `key2/`.
pub fn delete_args(bucket: &str, key: &str, is_dir: bool, endpoint: &str) -> Vec<String> {
    let args = if is_dir {
        vec![
            "s3".into(),
            "rm".into(),
            format!("{}/", uri(bucket, key)),
            "--recursive".into(),
            "--only-show-errors".into(),
        ]
    } else {
        vec![
            "s3".into(),
            "rm".into(),
            uri(bucket, key),
            "--only-show-errors".into(),
        ]
    };
    with_endpoint(args, endpoint)
}

/// Which copies fall outside "keep the newest `retain` per VM". `rows` are `(replica id, vm id)` for
/// verified copies, already ordered newest backup first; the first `retain` of each VM are kept.
pub fn expired<T: Copy, V: Eq + std::hash::Hash + Copy>(rows: &[(T, V)], retain: usize) -> Vec<T> {
    let mut seen: std::collections::HashMap<V, usize> = std::collections::HashMap::new();
    let mut out = Vec::new();
    for (id, vm) in rows {
        let n = seen.entry(*vm).or_insert(0);
        *n += 1;
        if *n > retain {
            out.push(*id);
        }
    }
    out
}

/// True when `remote_uri` is an object under this destination (`s3://bucket/prefix/...`), so a
/// retention pass never deletes outside the prefix its target owns.
pub fn within_dest(dest: &S3Dest, remote_uri: &str) -> bool {
    remote_uri.starts_with(&format!("s3://{}/{}/", dest.bucket, dest.prefix))
        && !remote_uri.contains("/../")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dest() -> S3Dest {
        S3Dest::from_config(&json!({"bucket": "dr-backups", "prefix": "site-a/", "endpoint_url": "https://s3.dr.example"})).unwrap()
    }

    #[test]
    fn dest_from_config() {
        let d = dest();
        assert_eq!(d.bucket, "dr-backups");
        assert_eq!(d.prefix, "site-a");
        assert_eq!(
            d.key_for("vm-20260101.qcow2").unwrap(),
            "site-a/vm-20260101.qcow2"
        );
        let default = S3Dest::from_config(&json!({"bucket": "dr-backups"})).unwrap();
        assert_eq!(default.prefix, "machina");
        assert_eq!(default.endpoint_url, "");
    }

    #[test]
    fn refuses_flag_injection_and_bad_values() {
        assert!(S3Dest::from_config(&json!({})).is_err());
        assert!(S3Dest::from_config(&json!({"bucket": "--no-sign-request"})).is_err());
        assert!(S3Dest::from_config(&json!({"bucket": "ok-bucket", "prefix": "--x"})).is_err());
        assert!(S3Dest::from_config(&json!({"bucket": "ok-bucket", "prefix": "a/../b"})).is_err());
        assert!(S3Dest::from_config(
            &json!({"bucket": "ok-bucket", "endpoint_url": "--no-verify-ssl"})
        )
        .is_err());
        assert!(
            S3Dest::from_config(&json!({"bucket": "ok-bucket", "endpoint_url": "ftp://x"}))
                .is_err()
        );
        assert!(S3Dest::from_config(&json!({"bucket": "UPPER"})).is_err());
        let d = dest();
        assert!(d.key_for("a/b").is_err());
        assert!(d.key_for("..").is_err());
        assert!(d.key_for("").is_err());
    }

    #[test]
    fn push_and_fetch_arguments() {
        let f = push_args(
            "/var/lib/machina/backups/a.qcow2",
            false,
            "b",
            "p/a.qcow2",
            "",
        );
        assert_eq!(
            f,
            [
                "s3",
                "cp",
                "/var/lib/machina/backups/a.qcow2",
                "s3://b/p/a.qcow2",
                "--only-show-errors"
            ]
        );
        let d = push_args(
            "/var/lib/machina/backups/vm.d",
            true,
            "b",
            "p/vm.d",
            "https://e",
        );
        assert_eq!(
            d,
            [
                "s3",
                "sync",
                "/var/lib/machina/backups/vm.d",
                "s3://b/p/vm.d/",
                "--only-show-errors",
                "--endpoint-url",
                "https://e"
            ]
        );
        let g = fetch_args("/restore/vm.d", true, "b", "p/vm.d", "");
        assert_eq!(
            g,
            [
                "s3",
                "sync",
                "s3://b/p/vm.d/",
                "/restore/vm.d",
                "--only-show-errors"
            ]
        );
        assert_eq!(
            ls_args("b", "p/a", ""),
            ["s3", "ls", "s3://b/p/a", "--recursive"]
        );
    }

    #[test]
    fn parses_listings_including_spaces_and_padding() {
        let out = "2026-10-21 10:00:00 1073741824 site-a/vm.d/disk-vda.qcow2\n2026-10-21 10:00:01        512 site-a/vm.d/manifest.json\n2026-10-21 10:00:02       7 site-a/with space.txt\nbad line\n";
        let p = parse_ls(out);
        assert_eq!(
            p,
            vec![
                (1073741824, "site-a/vm.d/disk-vda.qcow2".to_string()),
                (512, "site-a/vm.d/manifest.json".to_string()),
                (7, "site-a/with space.txt".to_string()),
            ]
        );
    }

    #[test]
    fn verify_a_file_and_a_directory() {
        let remote = vec![(100, "p/a.qcow2".to_string())];
        assert_eq!(
            verify(&[("a.qcow2".into(), 100)], &remote, "p/a.qcow2", false).unwrap(),
            100
        );
        assert!(
            verify(&[("a.qcow2".into(), 99)], &remote, "p/a.qcow2", false)
                .unwrap_err()
                .contains("100 bytes")
        );
        assert!(verify(&[("a.qcow2".into(), 100)], &[], "p/a.qcow2", false)
            .unwrap_err()
            .contains("missing"));
        assert!(verify(&[], &remote, "p/a.qcow2", false).is_err());

        let remote = vec![
            (10, "p/vm.d/manifest.json".to_string()),
            (900, "p/vm.d/vda.qcow2".to_string()),
        ];
        let local = vec![
            ("manifest.json".to_string(), 10),
            ("vda.qcow2".to_string(), 900),
        ];
        assert_eq!(verify(&local, &remote, "p/vm.d", true).unwrap(), 910);
        let short = vec![(10, "p/vm.d/manifest.json".to_string())];
        assert!(verify(&local, &short, "p/vm.d", true)
            .unwrap_err()
            .contains("vda.qcow2"));
    }

    #[test]
    fn fetch_planning_from_a_listing() {
        let remote = vec![
            (10, "p/vm.d/manifest.json".to_string()),
            (900, "p/vm.d/vda.qcow2".to_string()),
        ];
        assert!(remote_is_dir(&remote, "p/vm.d"));
        assert!(!remote_is_dir(&[(5, "p/a.qcow2".to_string())], "p/a.qcow2"));
        assert_eq!(
            expected_files(&remote, "p/vm.d", true),
            vec![
                ("manifest.json".to_string(), 10),
                ("vda.qcow2".to_string(), 900)
            ]
        );
        assert_eq!(
            expected_files(&[(5, "p/a.qcow2".to_string())], "p/a.qcow2", false),
            vec![("a.qcow2".to_string(), 5)]
        );
    }

    #[test]
    fn manifest_round_trips_and_names_a_sidecar() {
        let files = vec![
            ManifestFile {
                path: "disk.qcow2".into(),
                size: 10,
                sha256: "aa".into(),
            },
            ManifestFile {
                path: "manifest.json".into(),
                size: 2,
                sha256: "bb".into(),
            },
        ];
        let text = manifest_json(&files);
        assert_eq!(parse_manifest(&text).unwrap(), files);
        assert!(parse_manifest("nope").is_err());
        assert!(parse_manifest("{}").is_err());
        assert_eq!(manifest_key("p/vm.d"), "p/vm.d.manifest.json");
        // The sidecar is not under the directory prefix, so a listing for the directory excludes it.
        assert!(!remote_is_dir(&[(5, manifest_key("p/vm.d"))], "p/vm.d"));
    }

    #[test]
    fn checksums_detect_changed_missing_and_extra_files() {
        let f = |p: &str, s: u64, h: &str| ManifestFile {
            path: p.into(),
            size: s,
            sha256: h.into(),
        };
        let manifest = vec![f("a", 1, "h1"), f("b", 2, "h2")];
        assert!(verify_manifest(&[f("a", 1, "h1"), f("b", 2, "h2")], &manifest).is_ok());
        assert!(verify_manifest(&[f("a", 1, "h1")], &manifest)
            .unwrap_err()
            .contains("b is missing"));
        assert!(
            verify_manifest(&[f("a", 1, "h1"), f("b", 3, "h2")], &manifest)
                .unwrap_err()
                .contains("3 bytes")
        );
        assert!(
            verify_manifest(&[f("a", 1, "h1"), f("b", 2, "XX")], &manifest)
                .unwrap_err()
                .contains("checksum")
        );
        assert!(verify_manifest(
            &[f("a", 1, "h1"), f("b", 2, "h2"), f("c", 1, "h3")],
            &manifest
        )
        .unwrap_err()
        .contains("c is not"));
        assert!(verify_manifest(&[], &[]).is_err());
    }

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let p = std::env::temp_dir().join(format!("machina-sha-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256_hex("abc"));
        // A file bigger than the read buffer hashes the same as the whole string.
        let big = "x".repeat((1 << 20) + 17);
        std::fs::write(&p, big.as_bytes()).unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256_hex(&big));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn delete_never_uses_a_bare_prefix() {
        assert_eq!(
            delete_args("b", "p/a.qcow2", false, ""),
            ["s3", "rm", "s3://b/p/a.qcow2", "--only-show-errors"]
        );
        assert_eq!(
            delete_args("b", "p/vm.d", true, "https://e"),
            [
                "s3",
                "rm",
                "s3://b/p/vm.d/",
                "--recursive",
                "--only-show-errors",
                "--endpoint-url",
                "https://e"
            ]
        );
    }

    #[test]
    fn retention_keeps_the_newest_per_vm() {
        // (replica, vm), newest first within each VM.
        let rows = [(1, 'a'), (2, 'a'), (3, 'a'), (4, 'b'), (5, 'b'), (6, 'a')];
        assert_eq!(expired(&rows, 2), vec![3, 6]);
        assert!(expired(&rows, 10).is_empty());
        assert_eq!(expired(&rows, 1), vec![2, 3, 5, 6]);
        assert!(expired::<i32, char>(&[], 3).is_empty());
    }

    #[test]
    fn retention_only_touches_objects_under_the_targets_prefix() {
        let d = dest();
        assert!(within_dest(&d, "s3://dr-backups/site-a/vm.qcow2"));
        assert!(!within_dest(&d, "s3://dr-backups/other/vm.qcow2"));
        assert!(!within_dest(&d, "s3://elsewhere/site-a/vm.qcow2"));
        assert!(!within_dest(&d, "s3://dr-backups/site-a/../other/x"));
    }
}
