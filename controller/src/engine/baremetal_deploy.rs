// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

//! OS deploy for allocated bare-metal servers: render an unattended-install document
//! (Ubuntu autoinstall or Red Hat-family kickstart) whose first boot runs the normal
//! Machina join command, so a deployed machine becomes a host through the existing
//! enrollment flow and no new join protocol.

use chrono::{Duration, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::db::DbPool;
use crate::engine::baremetal_lifecycle::{token_hash, transition, Lifecycle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Autoinstall,
    Kickstart,
}

/// `ubuntu-22.04`, `ubuntu-24.04`, `rhel-9`, `rocky-9`, `almalinux-9`.
pub fn install_kind(os: &str) -> anyhow::Result<InstallKind> {
    match os {
        "ubuntu-22.04" | "ubuntu-24.04" => Ok(InstallKind::Autoinstall),
        "rhel-9" | "rocky-9" | "almalinux-9" => Ok(InstallKind::Kickstart),
        other => anyhow::bail!(
            "unsupported os '{other}' (ubuntu-22.04, ubuntu-24.04, rhel-9, rocky-9, almalinux-9)"
        ),
    }
}

#[derive(Debug, Deserialize)]
pub struct DeployRequest {
    pub os: String,
    #[serde(default)]
    pub ssh_authorized_keys: Vec<String>,
    #[serde(default = "default_tz")]
    pub timezone: String,
    #[serde(default)]
    pub ttl_hours: i64,
}

fn default_tz() -> String {
    "UTC".into()
}

pub fn valid_hostname(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

fn valid_timezone(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 64
        && !t.contains("..")
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
}

fn valid_ssh_key(k: &str) -> bool {
    let k = k.trim();
    !k.is_empty()
        && !k.chars().any(|c| c.is_control())
        && !k.contains('"')
        && (k.starts_with("ssh-") || k.starts_with("ecdsa-") || k.starts_with("sk-"))
}

/// The join command goes into a script on the installed system; it must stay on one line
/// and must not be able to end the document it is embedded in.
fn valid_join_command(c: &str) -> bool {
    !c.is_empty()
        && !c.chars().any(|ch| ch == '\n' || ch == '\r' || ch == '\0')
        && !c.contains("MACHINA_EOF")
}

pub struct RenderInput<'a> {
    pub hostname: &'a str,
    pub join_command: &'a str,
    pub ssh_keys: &'a [String],
    pub timezone: &'a str,
}

fn validate(i: &RenderInput<'_>) -> anyhow::Result<()> {
    if !valid_hostname(i.hostname) {
        anyhow::bail!("server hostname '{}' is not a valid host name", i.hostname);
    }
    if !valid_timezone(i.timezone) {
        anyhow::bail!("invalid timezone");
    }
    if i.ssh_keys.is_empty() {
        anyhow::bail!("at least one ssh_authorized_keys entry is required (no password login)");
    }
    if !i.ssh_keys.iter().all(|k| valid_ssh_key(k)) {
        anyhow::bail!("ssh_authorized_keys entries must be single-line public keys");
    }
    if !valid_join_command(i.join_command) {
        anyhow::bail!("join command is not a single safe line");
    }
    Ok(())
}

const JOIN_SCRIPT_PATH: &str = "/usr/local/sbin/machina-join.sh";

/// Ubuntu autoinstall (subiquity) user-data. First boot runs the join script once.
pub fn render_autoinstall(i: &RenderInput<'_>) -> anyhow::Result<String> {
    validate(i)?;
    let keys: String = i
        .ssh_keys
        .iter()
        .map(|k| format!("      - \"{}\"\n", k.trim()))
        .collect();
    Ok(format!(
        "#cloud-config\n\
         autoinstall:\n\
         \x20 version: 1\n\
         \x20 locale: en_US.UTF-8\n\
         \x20 keyboard:\n\
         \x20   layout: us\n\
         \x20 identity:\n\
         \x20   hostname: {host}\n\
         \x20   username: machina\n\
         \x20   password: \"!\"\n\
         \x20 ssh:\n\
         \x20   install-server: true\n\
         \x20   allow-pw: false\n\
         \x20   authorized-keys:\n\
         {keys}\
         \x20 storage:\n\
         \x20   layout:\n\
         \x20     name: lvm\n\
         \x20 timezone: {tz}\n\
         \x20 user-data:\n\
         \x20   disable_root: true\n\
         \x20   write_files:\n\
         \x20     - path: {path}\n\
         \x20       permissions: \"0700\"\n\
         \x20       content: |\n\
         \x20         #!/bin/sh\n\
         \x20         set -e\n\
         \x20         {join}\n\
         \x20   runcmd:\n\
         \x20     - [ {path} ]\n\
         \x20 shutdown: reboot\n",
        host = i.hostname,
        keys = keys,
        tz = i.timezone,
        path = JOIN_SCRIPT_PATH,
        join = i.join_command,
    ))
}

/// Red Hat-family kickstart. A systemd oneshot runs the join script at first boot.
pub fn render_kickstart(i: &RenderInput<'_>, repo_url: &str) -> anyhow::Result<String> {
    validate(i)?;
    if repo_url.is_empty()
        || repo_url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        anyhow::bail!("a package repository URL is required for kickstart installs");
    }
    let keys: String = i
        .ssh_keys
        .iter()
        .map(|k| format!("sshkey --username=machina \"{}\"\n", k.trim()))
        .collect();
    Ok(format!(
        "text\n\
         lang en_US.UTF-8\n\
         keyboard us\n\
         timezone {tz} --utc\n\
         network --bootproto=dhcp --hostname={host} --activate\n\
         url --url=\"{repo}\"\n\
         rootpw --lock\n\
         user --name=machina --groups=wheel --lock\n\
         {keys}\
         zerombr\n\
         clearpart --all --initlabel\n\
         autopart --type=lvm\n\
         firstboot --disable\n\
         reboot\n\
         \n\
         %packages\n\
         @^minimal-environment\n\
         curl\n\
         %end\n\
         \n\
         %post --log=/root/machina-ks-post.log\n\
         cat > {path} <<'MACHINA_EOF'\n\
         #!/bin/sh\n\
         set -e\n\
         {join}\n\
         MACHINA_EOF\n\
         chmod 0700 {path}\n\
         cat > /etc/systemd/system/machina-join.service <<'MACHINA_EOF'\n\
         [Unit]\n\
         Description=Join this machine to Machina\n\
         After=network-online.target\n\
         Wants=network-online.target\n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={path}\n\
         ExecStartPost=/usr/bin/systemctl disable machina-join.service\n\
         [Install]\n\
         WantedBy=multi-user.target\n\
         MACHINA_EOF\n\
         systemctl enable machina-join.service\n\
         %end\n",
        tz = i.timezone,
        host = i.hostname,
        repo = repo_url,
        keys = keys,
        path = JOIN_SCRIPT_PATH,
        join = i.join_command,
    ))
}

/// Allocated -> deploying. Issues an enrollment token, renders the document and stores it.
/// Returns the join command (so the caller can show it) and the install kind.
pub async fn begin_deploy(
    pool: &DbPool,
    id: Uuid,
    req: &DeployRequest,
    controller_base: &str,
) -> anyhow::Result<InstallKind> {
    let kind = install_kind(&req.os)?;
    let (hostname, mac, lifecycle): (String, String, String) = crate::db::query_as(
        "SELECT hostname, mac_address, lifecycle FROM baremetal_servers WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("server not found"))?;
    if Lifecycle::parse(&lifecycle)? != Lifecycle::Allocated {
        anyhow::bail!("server must be allocated before it can be deployed (is {lifecycle})");
    }
    if mac.is_empty() {
        anyhow::bail!("server has no mac_address; it cannot be matched when it PXE-boots");
    }
    let repo = std::env::var("MACHINA_METAL_RHEL_REPO").unwrap_or_default();

    // Render with a placeholder first so a bad request fails before a token is issued.
    let probe = RenderInput {
        hostname: &hostname,
        join_command: "true",
        ssh_keys: &req.ssh_authorized_keys,
        timezone: &req.timezone,
    };
    match kind {
        InstallKind::Autoinstall => {
            render_autoinstall(&probe)?;
        }
        InstallKind::Kickstart => {
            render_kickstart(&probe, &repo)?;
        }
    }

    let cluster_id: Uuid = crate::db::query_scalar("SELECT id FROM clusters LIMIT 1")
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("no cluster configured"))?;
    let ttl = if req.ttl_hours <= 0 {
        24
    } else {
        req.ttl_hours.min(72)
    };
    let enroll = format!("join-{}", Uuid::new_v4());
    let (install_cmd, pinned) = crate::api::enrollment::join_commands(controller_base, &enroll);
    let join = pinned.unwrap_or(install_cmd);
    let input = RenderInput {
        hostname: &hostname,
        join_command: &join,
        ssh_keys: &req.ssh_authorized_keys,
        timezone: &req.timezone,
    };
    let doc = match kind {
        InstallKind::Autoinstall => render_autoinstall(&input)?,
        InstallKind::Kickstart => render_kickstart(&input, &repo)?,
    };

    transition(pool, id, Lifecycle::Deploying).await?;
    crate::db::query(
        "INSERT INTO enrollment_tokens (token, cluster_id, expires_at) VALUES (?, ?, ?)",
    )
    .bind(&enroll)
    .bind(cluster_id)
    .bind(Utc::now() + Duration::hours(ttl))
    .execute(pool)
    .await?;
    crate::db::query(
        "INSERT INTO baremetal_deployments (server_id, os, user_data) VALUES (?, ?, ?)
         ON CONFLICT (server_id) DO UPDATE SET os = excluded.os, user_data = excluded.user_data,
           token_hash = ''",
    )
    .bind(id)
    .bind(&req.os)
    .bind(doc)
    .execute(pool)
    .await?;
    Ok(kind)
}

/// A PXE-booting machine in `deploying`: mint the URL token for its installer config.
/// Returns `(server_id, os, token)`.
pub async fn deploy_boot_target(
    pool: &DbPool,
    mac: &str,
) -> anyhow::Result<Option<(Uuid, String, String)>> {
    let Some(mac) = super::baremetal_boot::normalize_mac(mac) else {
        return Ok(None);
    };
    let row: Option<(Uuid, String)> = crate::db::query_as(
        "SELECT s.id, d.os FROM baremetal_servers s
         JOIN baremetal_deployments d ON d.server_id = s.id
         WHERE s.mac_address = ? AND s.lifecycle = 'deploying'",
    )
    .bind(&mac)
    .fetch_optional(pool)
    .await?;
    let Some((id, os)) = row else { return Ok(None) };
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    crate::db::query("UPDATE baremetal_deployments SET token_hash = ? WHERE server_id = ?")
        .bind(token_hash(&token))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(Some((id, os, token)))
}

/// The stored document, only for the token minted at PXE time.
pub async fn document(pool: &DbPool, id: Uuid, token: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String, String)> = crate::db::query_as(
        "SELECT token_hash, user_data FROM baremetal_deployments WHERE server_id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(h, d)| (!h.is_empty() && h == token_hash(token)).then_some(d)))
}

/// The joining host has the registered hostname: deploying -> deployed, drop the document.
pub async fn mark_deployed_by_hostname(pool: &DbPool, hostname: &str) {
    let id: Option<Uuid> = crate::db::query_scalar(
        "SELECT id FROM baremetal_servers WHERE hostname = ? AND lifecycle = 'deploying'",
    )
    .bind(hostname)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if let Some(id) = id {
        if transition(pool, id, Lifecycle::Deployed).await.is_ok() {
            let _ = crate::db::query("DELETE FROM baremetal_deployments WHERE server_id = ?")
                .bind(id)
                .execute(pool)
                .await;
        }
    }
}

/// Kernel/initrd/stage-2 URLs and command line for the installer, as an iPXE script.
pub fn ipxe_deploy_script(
    controller_base: &str,
    server_id: Uuid,
    token: &str,
    os: &str,
    kind: InstallKind,
    rhel_repo: &str,
) -> String {
    let c = controller_base.trim_end_matches('/');
    let a = format!("{c}/dist");
    let cfg = format!("{c}/api/v1/baremetal/boot/install/{server_id}/{token}");
    let args = match kind {
        InstallKind::Autoinstall => {
            format!("ip=dhcp autoinstall ds=nocloud-net;s={cfg}/ url={a}/install-{os}.iso")
        }
        InstallKind::Kickstart => format!("ip=dhcp inst.ks={cfg}/ks.cfg inst.repo={rhel_repo}"),
    };
    format!(
        "#!ipxe\n\
         kernel {a}/install-{os}-vmlinuz initrd=install-initrd {args}\n\
         initrd --name install-initrd {a}/install-{os}-initrd\n\
         boot\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<String> {
        vec!["ssh-ed25519 AAAAC3Nza ops@example".to_string()]
    }

    fn input<'a>(k: &'a [String], join: &'a str) -> RenderInput<'a> {
        RenderInput {
            hostname: "rack2-u14",
            join_command: join,
            ssh_keys: k,
            timezone: "Europe/Berlin",
        }
    }

    #[test]
    fn os_names_map_to_install_kinds() {
        assert_eq!(
            install_kind("ubuntu-24.04").unwrap(),
            InstallKind::Autoinstall
        );
        assert_eq!(install_kind("rocky-9").unwrap(), InstallKind::Kickstart);
        assert!(install_kind("windows-11").is_err());
    }

    #[test]
    fn autoinstall_has_no_password_login_and_runs_the_join_once() {
        let k = keys();
        let d = render_autoinstall(&input(
            &k,
            "curl -fsSL https://c/install.sh | bash -s -- --token join-1",
        ))
        .unwrap();
        assert!(d.starts_with("#cloud-config\nautoinstall:\n"));
        assert!(d.contains("hostname: rack2-u14"));
        assert!(d.contains("allow-pw: false"));
        assert!(d.contains("ssh-ed25519 AAAAC3Nza ops@example"));
        assert!(d.contains("timezone: Europe/Berlin"));
        assert!(d.contains("--token join-1"));
        assert!(d.contains("runcmd:"));
    }

    #[test]
    fn kickstart_needs_a_repo_and_embeds_the_join_in_a_first_boot_unit() {
        let k = keys();
        let i = input(&k, "bash /tmp/j.sh --token join-2");
        assert!(render_kickstart(&i, "").is_err());
        let d = render_kickstart(&i, "http://mirror.local/rocky9").unwrap();
        assert!(d.contains("url --url=\"http://mirror.local/rocky9\""));
        assert!(d.contains("rootpw --lock"));
        assert!(d.contains("sshkey --username=machina \"ssh-ed25519 AAAAC3Nza ops@example\""));
        assert!(d.contains("ExecStartPost=/usr/bin/systemctl disable machina-join.service"));
        assert!(d.contains("--token join-2"));
    }

    #[test]
    fn hostile_input_is_rejected_before_rendering() {
        let k = keys();
        assert!(render_autoinstall(&input(&k, "true\nrm -rf /")).is_err());
        assert!(render_autoinstall(&input(&k, "echo MACHINA_EOF")).is_err());
        assert!(render_autoinstall(&RenderInput {
            hostname: "a b",
            ..input(&k, "true")
        })
        .is_err());
        assert!(render_autoinstall(&RenderInput {
            timezone: "../etc",
            ..input(&k, "true")
        })
        .is_err());
        let none: Vec<String> = vec![];
        assert!(render_autoinstall(&input(&none, "true")).is_err());
        let bad = vec!["ssh-ed25519 AAAA\"\n  - evil".to_string()];
        assert!(render_autoinstall(&input(&bad, "true")).is_err());
    }

    #[test]
    fn ipxe_deploy_script_points_at_the_install_assets() {
        let id = Uuid::from_u128(1);
        let s = ipxe_deploy_script(
            "https://c:5093/",
            id,
            "tk",
            "ubuntu-24.04",
            InstallKind::Autoinstall,
            "",
        );
        assert!(s.contains("https://c:5093/dist/install-ubuntu-24.04-vmlinuz"));
        assert!(s.contains(&format!(
            "s=https://c:5093/api/v1/baremetal/boot/install/{id}/tk/"
        )));
        let r = ipxe_deploy_script(
            "https://c",
            id,
            "tk",
            "rocky-9",
            InstallKind::Kickstart,
            "http://m/r9",
        );
        assert!(r.contains("inst.ks=") && r.contains("inst.repo=http://m/r9"));
    }

    #[tokio::test]
    async fn deploy_flow_stores_doc_for_the_pxe_token_and_join_marks_deployed() {
        let pool = crate::db::testing::pool().await;
        crate::db::query("INSERT INTO clusters (id, name) VALUES (?, 'c')")
            .bind(Uuid::from_u128(1))
            .execute(&pool)
            .await
            .unwrap();
        let id = Uuid::new_v4();
        crate::db::query(
            "INSERT INTO baremetal_servers (id, hostname, bmc_type, lifecycle, mac_address)
             VALUES (?, 'metal-d1', 'fake', 'allocated', 'aa:bb:cc:00:00:01')",
        )
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
        let req = DeployRequest {
            os: "ubuntu-24.04".into(),
            ssh_authorized_keys: keys(),
            timezone: "UTC".into(),
            ttl_hours: 0,
        };
        assert_eq!(
            begin_deploy(&pool, id, &req, "https://c:5093")
                .await
                .unwrap(),
            InstallKind::Autoinstall
        );
        // Not allocated any more: a second deploy is refused.
        assert!(begin_deploy(&pool, id, &req, "https://c:5093")
            .await
            .is_err());
        let (got, os, token) = deploy_boot_target(&pool, "AA:BB:CC:00:00:01")
            .await
            .unwrap()
            .unwrap();
        assert_eq!((got, os.as_str()), (id, "ubuntu-24.04"));
        assert!(document(&pool, id, "wrong").await.unwrap().is_none());
        let doc = document(&pool, id, &token).await.unwrap().unwrap();
        assert!(doc.contains("--token join-"));
        mark_deployed_by_hostname(&pool, "metal-d1").await;
        let lc: String =
            crate::db::query_scalar("SELECT lifecycle FROM baremetal_servers WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(lc, "deployed");
        assert!(document(&pool, id, &token).await.unwrap().is_none());
    }
}
