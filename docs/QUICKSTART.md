# Quickstart: a controller, a node, a VM

One path, copy-paste. Everything here uses commands that ship in the packages. Deeper detail: [INSTALL.md](INSTALL.md).

## 1. The controller (about 5 minutes)

On the machine that will manage the fleet (Ubuntu 22.04+, Debian 12+, RHEL/Rocky/Alma 9+ or Fedora):

```bash
sudo machina-preflight --role controller        # says exactly what to fix, if anything
sudo apt install ./machina_*_amd64.deb ./machina-controller_*_amd64.deb ./machina-agent_*_amd64.deb
#   RPM systems: sudo dnf install ./machina-*.rpm
sudo machinactl show-login                      # URL, user name and the generated admin password
```

The controller keeps its state in embedded SQLite, which is right for a first cloud. For a large fleet or several controllers, choose PostgreSQL at install time (`scripts/install-platform.sh --database pod`) or later with `sudo machinactl db setup pod --migrate`: [database guide](guides/database.md).

Open the URL (`https://<host>:5092`; the certificate is self-signed until you install your own) and sign in as `admin`. Change the password.

## 2. Let nodes join (once)

Nodes join over HTTPS and pin the controller by its CA fingerprint, so the controller needs a join listener and something for new nodes to install:

```bash
sudo tee -a /etc/default/machina-platform >/dev/null <<'EOF2'
MACHINA_PUBLIC_URL=http://<controller-address>:5093
MACHINA_CONTROLLER_TLS_ADDR=0.0.0.0:5094
MACHINA_CONTROLLER_TLS_SANS=<controller-address>
EOF2
sudo machinactl dist publish                    # puts the agent where /dist serves it (checksummed)
sudo systemctl restart machina-controller
```

Allow port 5094 from your nodes only. The listener serves just join, the CA, health and the install script; the rest of the API stays on `127.0.0.1:5093`.

## 3. Add a node: one command or one click

- **Web:** Platform → Hosts → **Add machine** (or `/platform/enroll`). A token is created for you; the page shows the one command (with a countdown and a lifetime choice), the Ansible / cloud-init / Terraform equivalents on the Automation tab, and a live map and terminal of the node joining. When validation fails it names the failing check and its fix and offers **Re-check**. If the controller's HTTPS join listener is off, a banner says so and names `MACHINA_CONTROLLER_TLS_ADDR`.
- **Command line, from the controller:** `sudo machinactl host add user@node` (needs SSH and sudo on the node).
- **Paste:** run the command the wizard shows (it carries a single-use token that expires in 1 hour) on the node as a normal user with sudo.

On a bare Ubuntu or Fedora node the command installs libvirt and QEMU, downloads the agent from your controller (checksums verified), checks the controller's CA fingerprint, receives the node's own certificate and token, starts the agent and waits to be validated. Nothing is copied by hand and no fleet-wide secret is sent.

```bash
sudo machinactl host list                       # NAME ADDRESS STATE VMS CPU% VALIDATION
```

Automating many nodes: [`deploy/`](../deploy/README.md) has an Ansible playbook, a cloud-init template, Terraform/OpenTofu and a Compose demo that all use the same command.

## 4. First VM

Web: **Create VM** (`/create`, or Platform → VM Builder). Pick a host (or let the scheduler choose), an image and a size. The guide for each screen: [customer/getting-started.md](customer/getting-started.md).

## 5. Day 2

```bash
sudo machinactl backup all                      # one verified archive: database, config, secrets, CAs (keep it private)
sudo machinactl restore /var/backups/machina/full/machina-full-<time>.tar.gz
sudo machinactl host drain NODE                 # move VMs off before maintenance (undrain to schedule again)
sudo machinactl host remove NODE                # its token and certificate stop working
```

**Upgrade:** take a backup, install the new packages on the controller, then on each node (controller first): `sudo apt install ./machina*_new.deb`. Configuration and data are kept. On a controller host, `sudo machinactl upgrade --from DIR` (a release directory) does the same in order: it takes `backup all` first, upgrades the controller, then the daemon, then the local agent, checks health after each step and rolls a failed step back by itself. `--dry-run` shows the plan first (see [INSTALL.md](INSTALL.md)).

## When something is wrong

| Symptom | Do |
|---|---|
| A machine will not install or join | `sudo machina-preflight --role agent --controller https://<controller>:5094` (add `--fix` to install libvirt/QEMU) |
| A host shows failed validation | Platform → Hosts → the host: each failed check says how to fix it |
| "failed tasks" on Mission Control | Platform → Tasks: grouped by cause, with a suggested fix |
| Forgot the admin password | `sudo machinactl show-login` |

## What was tested

[claims.md](claims.md) lists what has actually been run (the one-command join, the automation files) and what has not. The automation files are syntax-checked but most are not run end to end yet.
