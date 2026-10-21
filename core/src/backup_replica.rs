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
}
