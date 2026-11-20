// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! Key pair generation, image registration and copying, image attributes and instance type offerings.
//!
//! An image is a template (`ami-`). `RegisterImage` from a snapshot clones the snapshot into a volume that the image owns and
//! registers a template over it; `CopyImage` registers another template over the same source disk. Both stay inside the one
//! region; `SourceRegion` must name it.

use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::{Extension, Json};
use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{Ed25519KeyPair, KeyPair};
use uuid::Uuid;

use crate::auth::{require_operator, AuthUser};
use crate::resource_ids::{ec2_id, Kind};
use crate::state::AppState;

use super::more::{api_err, resolve};
use super::{tagspec, xml_escape, Ec2Error};

type Params = BTreeMap<String, String>;

const REGION: &str = "machina";

fn bad(code: &'static str, msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad(code, msg)
}

fn unsupported(msg: impl Into<String>) -> Ec2Error {
    Ec2Error::bad("UnsupportedOperation", msg)
}

fn need(p: &Params, k: &str) -> Result<String, Ec2Error> {
    p.get(k).cloned().ok_or_else(|| bad("MissingParameter", format!("The request must contain the parameter {k}")))
}

// ---- key pairs -------------------------------------------------------------------------------------------------------

fn put_u32(v: &mut Vec<u8>, n: u32) {
    v.extend_from_slice(&n.to_be_bytes());
}

fn put_str(v: &mut Vec<u8>, s: &[u8]) {
    put_u32(v, s.len() as u32);
    v.extend_from_slice(s);
}

/// An ed25519 key in OpenSSH's own formats: the private key file (`-----BEGIN OPENSSH PRIVATE KEY-----`, unencrypted) and
/// the `authorized_keys` line.
pub struct SshKey {
    pub private_pem: String,
    pub public_line: String,
}

pub fn encode_ed25519(seed: &[u8], public: &[u8], comment: &str, check: u32) -> SshKey {
    let mut pub_blob = Vec::new();
    put_str(&mut pub_blob, b"ssh-ed25519");
    put_str(&mut pub_blob, public);
    let mut private = Vec::new();
    put_u32(&mut private, check);
    put_u32(&mut private, check);
    put_str(&mut private, b"ssh-ed25519");
    put_str(&mut private, public);
    let mut secret = Vec::with_capacity(64);
    secret.extend_from_slice(seed);
    secret.extend_from_slice(public);
    put_str(&mut private, &secret);
    put_str(&mut private, comment.as_bytes());
    let mut pad = 1u8;
    while private.len() % 8 != 0 {
        private.push(pad);
        pad += 1;
    }
    let mut file = b"openssh-key-v1\0".to_vec();
    put_str(&mut file, b"none");
    put_str(&mut file, b"none");
    put_str(&mut file, b"");
    put_u32(&mut file, 1);
    put_str(&mut file, &pub_blob);
    put_str(&mut file, &private);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&file);
    let wrapped: Vec<&str> = b64.as_bytes().chunks(70).map(|c| std::str::from_utf8(c).unwrap_or_default()).collect();
    SshKey {
        private_pem: format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n-----END OPENSSH PRIVATE KEY-----\n", wrapped.join("\n")),
        public_line: format!("ssh-ed25519 {} {comment}", base64::engine::general_purpose::STANDARD.encode(&pub_blob)),
    }
}

pub fn generate_ed25519(comment: &str) -> Result<SshKey, Ec2Error> {
    let internal = |m: &str| Ec2Error::new(axum::http::StatusCode::INTERNAL_SERVER_ERROR, "InternalError", m.to_string());
    let rng = SystemRandom::new();
    let doc = Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| internal("could not generate a key"))?;
    let bytes = doc.as_ref();
    // ring's PKCS#8 for ed25519 is a fixed 16-byte header, the 32-byte seed, then the public key
    let seed = bytes.get(16..48).ok_or_else(|| internal("unexpected key encoding"))?;
    let pair = Ed25519KeyPair::from_pkcs8(bytes).map_err(|_| internal("could not read the generated key"))?;
    let public = pair.public_key().as_ref();
    Ed25519KeyPair::from_seed_and_public_key(seed, public).map_err(|_| internal("the generated key is inconsistent"))?;
    let mut check = [0u8; 4];
    rng.fill(&mut check).map_err(|_| internal("no randomness"))?;
    Ok(encode_ed25519(seed, public, comment, u32::from_be_bytes(check)))
}

/// `CreateKeyPair`: the key is generated here, only the public half is stored, and the private key is returned once.
/// ed25519 only (`KeyType` defaults to it here); `rsa` and `.ppk` are refused.
pub async fn create_key_pair(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "KeyName")?;
    match p.get("KeyType").map(String::as_str) {
        None | Some("ed25519") => {}
        Some("rsa") => return Err(unsupported("KeyType=rsa is not supported: keys are ed25519 (omit KeyType or pass ed25519)")),
        Some(_) => return Err(bad("InvalidParameterValue", "KeyType must be rsa or ed25519")),
    }
    match p.get("KeyFormat").map(String::as_str) {
        None | Some("pem") => {}
        Some("ppk") => return Err(unsupported("KeyFormat=ppk is not supported")),
        Some(_) => return Err(bad("InvalidParameterValue", "KeyFormat must be pem or ppk")),
    }
    tagspec::only_types(p, &["key-pair"])?;
    let key = generate_ed25519(&name)?;
    let body: crate::api::keypairs::CreateKeypairBody =
        serde_json::from_value(serde_json::json!({ "name": name, "public_key": key.public_line })).map_err(|e| bad("InvalidParameterValue", e.to_string()))?;
    let Json(k) = crate::api::keypairs::create_keypair(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    tagspec::apply(state, Kind::KeyPair, k.id, &tagspec::tags_for(p, "key-pair")).await?;
    Ok(format!(
        "<keyName>{}</keyName><keyFingerprint>{}</keyFingerprint><keyPairId>{}</keyPairId><keyMaterial>{}</keyMaterial>",
        xml_escape(&k.name),
        xml_escape(&k.fingerprint),
        ec2_id(Kind::KeyPair, k.id),
        xml_escape(&key.private_pem)
    ))
}

// ---- images --------------------------------------------------------------------------------------------------------

async fn template_row(state: &AppState, image: &str) -> Result<(Uuid, String, String, String, bool, Option<String>, String, String), Ec2Error> {
    let id = resolve(state, Kind::Image, image, "InvalidAMIID.NotFound").await?;
    let row = crate::db::query_as(
        "SELECT id, name, version, source_disk, cloud_init, os_family, category, description FROM templates WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.ok_or_else(|| bad("InvalidAMIID.NotFound", format!("The image id '{image}' does not exist")))
}

fn check_register_options(p: &Params) -> Result<(), Ec2Error> {
    if p.contains_key("ImageLocation") {
        return Err(unsupported("ImageLocation (an S3 manifest) is not supported: register an image from a snapshot with BlockDeviceMapping.1.Ebs.SnapshotId"));
    }
    for k in ["BootMode", "TpmSupport", "UefiData", "ImdsSupport", "BillingProducts.1"] {
        if p.contains_key(k) {
            return Err(unsupported(format!("{k} is not supported")));
        }
    }
    if p.get("VirtualizationType").is_some_and(|v| v != "hvm") {
        return Err(unsupported("only hvm images are supported"));
    }
    if p.get("EnaSupport").is_some_and(|v| v == "false") || p.get("SriovNetSupport").is_some() {
        return Err(unsupported("EnaSupport and SriovNetSupport are not supported"));
    }
    Ok(())
}

/// `RegisterImage` from a snapshot: the snapshot is cloned into a volume the image owns and a private template is registered
/// over it.
pub async fn register_image(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "Name")?;
    check_register_options(p)?;
    tagspec::only_types(p, &["image"])?;
    let mut snapshot: Option<String> = None;
    for n in 1..=24 {
        let pre = format!("BlockDeviceMapping.{n}");
        let Some(dev) = p.get(&format!("{pre}.DeviceName")) else { break };
        let root = super::run_options::normalize_device(dev).is_some_and(|(_, root)| root) || p.get("RootDeviceName") == Some(dev);
        if let Some(s) = p.get(&format!("{pre}.Ebs.SnapshotId")) {
            if root {
                snapshot = Some(s.clone());
            } else {
                return Err(unsupported(format!("{pre}: only the root device can come from a snapshot in an image")));
            }
        }
    }
    let snapshot = snapshot.ok_or_else(|| bad("MissingParameter", "a root BlockDeviceMapping with Ebs.SnapshotId is required"))?;
    let snap = resolve(state, Kind::Snapshot, &snapshot, "InvalidSnapshot.NotFound").await?;
    let source_name = format!("ami-src-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let Json(vol) = crate::api::volumes::create_volume_from_snapshot(
        State(state.clone()),
        Extension(actor.clone()),
        Path(snap),
        Json(crate::api::volumes::VolumeFromSnapshotBody { name: source_name.clone() }),
    )
    .await
    .map_err(api_err)?;
    let body = crate::api::templates::CreateTemplateBody {
        name: name.clone(),
        version: "1".into(),
        source_disk: source_name,
        cloud_init: true,
        os_family: None,
        category: "instance".into(),
        description: p.get("Description").cloned().unwrap_or_else(|| format!("registered from {snapshot} (volume {})", ec2_id(Kind::Volume, vol.id))),
        featured: false,
        marketplace: false,
        icon: None,
    };
    let Json(row) = crate::api::templates::create_template(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    private(state, actor, &name, "1").await;
    tagspec::apply(state, Kind::Image, row.id, &tagspec::tags_for(p, "image")).await?;
    Ok(format!("<imageId>{}</imageId>", ec2_id(Kind::Image, row.id)))
}

async fn private(state: &AppState, actor: &AuthUser, name: &str, version: &str) {
    let _ = crate::api::templates::set_template_visibility(
        State(state.clone()),
        Extension(actor.clone()),
        Path((name.to_string(), version.to_string())),
        Json(crate::api::templates::VisibilityBody { visibility: "private".into() }),
    )
    .await;
}

/// `CopyImage` inside the one region: another private template over the same source disk.
pub async fn copy_image(state: &AppState, actor: &AuthUser, p: &Params) -> Result<String, Ec2Error> {
    require_operator(actor)?;
    let name = need(p, "Name")?;
    let source = need(p, "SourceImageId")?;
    let region = need(p, "SourceRegion")?;
    if region != REGION {
        return Err(bad("InvalidParameterValue", format!("SourceRegion must be '{REGION}', the only region")));
    }
    if p.get("Encrypted").is_some_and(|v| v == "true") || p.contains_key("KmsKeyId") {
        return Err(unsupported("encrypted images are not supported"));
    }
    tagspec::only_types(p, &["image"])?;
    let (_, _, _, source_disk, cloud_init, os_family, category, description) = template_row(state, &source).await?;
    let body = crate::api::templates::CreateTemplateBody {
        name: name.clone(),
        version: "1".into(),
        source_disk,
        cloud_init,
        os_family,
        category,
        description: p.get("Description").cloned().unwrap_or_else(|| if description.is_empty() { format!("copy of {source}") } else { description }),
        featured: false,
        marketplace: false,
        icon: None,
    };
    let Json(row) = crate::api::templates::create_template(State(state.clone()), Extension(actor.clone()), Json(body)).await.map_err(api_err)?;
    private(state, actor, &name, "1").await;
    tagspec::apply(state, Kind::Image, row.id, &tagspec::tags_for(p, "image")).await?;
    Ok(format!("<imageId>{}</imageId>", ec2_id(Kind::Image, row.id)))
}

/// `DescribeImageAttribute`.
pub async fn describe_image_attribute(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    let image = need(p, "ImageId")?;
    let attribute = need(p, "Attribute")?;
    let (id, _, _, _, _, _, _, description) = template_row(state, &image).await?;
    let body = match attribute.as_str() {
        "description" => format!("<description><value>{}</value></description>", xml_escape(&description)),
        "launchPermission" => {
            let visibility: Option<String> = crate::db::query_scalar("SELECT COALESCE(visibility, 'public') FROM templates WHERE id = ?").bind(id).fetch_optional(&state.pool).await?;
            let projects: Vec<String> = crate::db::query_scalar("SELECT project FROM image_shares WHERE template_id = ? ORDER BY project").bind(id).fetch_all(&state.pool).await?;
            let mut items = String::new();
            if visibility.as_deref() == Some("public") {
                items.push_str("<item><group>all</group></item>");
            }
            for pr in projects {
                items.push_str(&format!("<item><userId>{}</userId></item>", xml_escape(&pr)));
            }
            format!("<launchPermission>{items}</launchPermission>")
        }
        "blockDeviceMapping" => "<blockDeviceMapping><item><deviceName>/dev/vda</deviceName><ebs><deleteOnTermination>true</deleteOnTermination></ebs></item></blockDeviceMapping>".to_string(),
        "kernel" | "ramdisk" | "sriovNetSupport" | "bootMode" | "tpmSupport" | "uefiData" | "lastLaunchedTime" | "imdsSupport" => {
            format!("<{attribute}><value/></{attribute}>")
        }
        "productCodes" => "<productCodes/>".to_string(),
        other => return Err(bad("InvalidParameterValue", format!("Attribute '{other}' is not a valid image attribute"))),
    };
    Ok(format!("<imageId>{image}</imageId>{body}"))
}

// ---- instance type offerings -------------------------------------------------------------------------------------------

/// `DescribeInstanceTypeOfferings`: every instance type is offered in the region and on every host (a zone is a host).
pub async fn describe_instance_type_offerings(state: &AppState, p: &Params) -> Result<String, Ec2Error> {
    check_offering_filters(p)?;
    let location_type = p.get("LocationType").map(String::as_str).unwrap_or("region");
    let flavors: Vec<String> = crate::db::query_scalar("SELECT name FROM flavors ORDER BY name").fetch_all(&state.pool).await?;
    let locations: Vec<String> = match location_type {
        "region" => vec![REGION.to_string()],
        "availability-zone" => crate::db::query_scalar("SELECT hostname FROM hosts ORDER BY hostname").fetch_all(&state.pool).await?,
        "availability-zone-id" => {
            let hosts: Vec<Uuid> = crate::db::query_scalar("SELECT id FROM hosts ORDER BY hostname").fetch_all(&state.pool).await?;
            hosts.into_iter().map(super::fleet::zone_id).collect()
        }
        _ => return Err(bad("InvalidParameterValue", "LocationType must be region, availability-zone or availability-zone-id")),
    };
    let filters = super::parse_filters(p);
    let mut items = String::new();
    for loc in &locations {
        for f in &flavors {
            let keep = filters.iter().all(|(n, v)| match n.as_str() {
                "instance-type" => super::foundation::any_match(v, f),
                "location" => super::foundation::any_match(v, loc),
                _ => true,
            });
            if keep {
                items.push_str(&format!(
                    "<item><instanceType>{}</instanceType><locationType>{location_type}</locationType><location>{}</location></item>",
                    xml_escape(f),
                    xml_escape(loc)
                ));
            }
        }
    }
    Ok(format!("<instanceTypeOfferingSet>{items}</instanceTypeOfferingSet>"))
}

/// Rows for the filter names `DescribeInstanceTypeOfferings` answers; other names are refused.
pub fn check_offering_filters(p: &Params) -> Result<(), Ec2Error> {
    for (name, values) in super::parse_filters(p) {
        if values.is_empty() || !matches!(name.as_str(), "instance-type" | "location") {
            return Err(bad("InvalidParameterValue", format!("The filter '{name}' is not valid for DescribeInstanceTypeOfferings")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&str, &str)]) -> Params {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    struct Reader<'a> {
        buf: &'a [u8],
        at: usize,
    }

    impl<'a> Reader<'a> {
        fn u32(&mut self) -> u32 {
            let v = u32::from_be_bytes(self.buf[self.at..self.at + 4].try_into().unwrap());
            self.at += 4;
            v
        }
        fn string(&mut self) -> Vec<u8> {
            let n = self.u32() as usize;
            let out = self.buf[self.at..self.at + n].to_vec();
            self.at += n;
            out
        }
    }

    /// Reads the file back the way OpenSSH does: magic, cipher none, one key, matching check ints, ed25519, 64-byte secret.
    fn parse_private(pem: &str) -> (Vec<u8>, Vec<u8>, String) {
        let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        let raw = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        assert!(raw.starts_with(b"openssh-key-v1\0"));
        let mut r = Reader { buf: &raw, at: 15 };
        assert_eq!(r.string(), b"none");
        assert_eq!(r.string(), b"none");
        assert_eq!(r.string(), b"");
        assert_eq!(r.u32(), 1);
        let _pub_blob = r.string();
        let private = r.string();
        assert_eq!(private.len() % 8, 0, "the private section is padded to the cipher block size");
        assert_eq!(private[0..4], private[4..8], "check ints match");
        let mut q = Reader { buf: &private, at: 8 };
        assert_eq!(q.string(), b"ssh-ed25519");
        let public = q.string();
        let secret = q.string();
        let comment = String::from_utf8(q.string()).unwrap();
        (secret, public, comment)
    }

    #[test]
    fn a_generated_key_is_a_valid_openssh_ed25519_pair() {
        let k = generate_ed25519("mykey").unwrap();
        assert!(k.private_pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----\n") && k.private_pem.ends_with("-----END OPENSSH PRIVATE KEY-----\n"));
        let (secret, public, comment) = parse_private(&k.private_pem);
        assert_eq!(comment, "mykey");
        assert_eq!(secret.len(), 64);
        assert_eq!(&secret[32..], &public[..], "the secret ends with the public key");
        Ed25519KeyPair::from_seed_and_public_key(&secret[..32], &public).expect("seed and public key belong together");
        // the public line carries the same key
        let line_b64 = k.public_line.split_whitespace().nth(1).unwrap();
        let blob = base64::engine::general_purpose::STANDARD.decode(line_b64).unwrap();
        assert!(blob.ends_with(&public));
        assert!(k.public_line.starts_with("ssh-ed25519 ") && k.public_line.ends_with(" mykey"));
        // two keys differ
        assert_ne!(generate_ed25519("a").unwrap().public_line, k.public_line);
    }

    /// If `ssh-keygen` is installed it must accept the file and derive the same public key from it.
    #[test]
    fn ssh_keygen_derives_the_same_public_key() {
        let k = generate_ed25519("probe").unwrap();
        let dir = std::env::temp_dir().join(format!("machina-key-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("id");
        std::fs::write(&path, &k.private_pem).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        match std::process::Command::new("ssh-keygen").args(["-y", "-f"]).arg(&path).output() {
            Ok(out) => {
                assert!(out.status.success(), "ssh-keygen rejected the key: {}", String::from_utf8_lossy(&out.stderr));
                let derived = String::from_utf8_lossy(&out.stdout);
                let want: Vec<&str> = k.public_line.split_whitespace().take(2).collect();
                let got: Vec<&str> = derived.split_whitespace().take(2).collect();
                assert_eq!(got, want);
            }
            Err(_) => eprintln!("ssh-keygen not installed: skipped"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_options_that_are_not_ed25519_are_refused() {
        let reg = |pairs: &[(&str, &str)]| check_register_options(&p(pairs));
        assert_eq!(reg(&[("ImageLocation", "s3://x")]).unwrap_err().code, "UnsupportedOperation");
        assert_eq!(reg(&[("BootMode", "uefi")]).unwrap_err().code, "UnsupportedOperation");
        assert_eq!(reg(&[("VirtualizationType", "paravirtual")]).unwrap_err().code, "UnsupportedOperation");
        assert!(reg(&[("VirtualizationType", "hvm"), ("Architecture", "x86_64")]).is_ok());
    }

    #[test]
    fn offering_filters_are_checked() {
        assert!(check_offering_filters(&p(&[("Filter.1.Name", "location"), ("Filter.1.Value.1", "machina")])).is_ok());
        assert!(check_offering_filters(&p(&[("Filter.1.Name", "memory"), ("Filter.1.Value.1", "x")])).is_err());
    }

    #[tokio::test]
    async fn offerings_cover_region_and_zones_and_filter_by_type() {
        let (state, _rx) = crate::engine::test_support::test_state().await;
        crate::db::query("INSERT INTO flavors (id, name, vcpus, memory_mib, disk_gib) VALUES (?, 'm1.small', 1, 2048, 20)").bind(Uuid::new_v4()).execute(&state.pool).await.unwrap();
        crate::db::query("INSERT INTO flavors (id, name, vcpus, memory_mib, disk_gib) VALUES (?, 'm1.large', 4, 8192, 20)").bind(Uuid::new_v4()).execute(&state.pool).await.unwrap();
        let region = describe_instance_type_offerings(&state, &p(&[])).await.unwrap();
        assert_eq!(region.matches("<item>").count(), 2);
        assert!(region.contains("<locationType>region</locationType><location>machina</location>"));
        let one = describe_instance_type_offerings(&state, &p(&[("Filter.1.Name", "instance-type"), ("Filter.1.Value.1", "m1.l*")])).await.unwrap();
        assert_eq!(one.matches("<item>").count(), 1);
        assert!(one.contains("m1.large"));
        assert_eq!(describe_instance_type_offerings(&state, &p(&[("LocationType", "rack")])).await.unwrap_err().code, "InvalidParameterValue");
    }
}
