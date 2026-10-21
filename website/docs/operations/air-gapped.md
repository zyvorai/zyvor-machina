---
sidebar_position: 7
title: Air-gapped and sovereign sites
description: Run Machina with no route to the internet - what to mirror, which settings point at your mirror, and how to prove nothing leaves the site.
---

# Air-gapped and sovereign sites

Machina has no required cloud dependency and never phones home on its own. The controller contacts nothing for updates unless you
set `MACHINA_UPDATE_CHECK_URL`, and there is no telemetry upload. Everything below is something **an operator asks for** that
reaches a public address by default, with the setting that moves it to your mirror.

## Install and join with no network

1. Copy `machina-<version>-linux-amd64.tar.gz` (and `SHA256SUMS`) to the host, verify it, then
   `sudo ./install.sh --all`. See [Install](https://github.com/zyvorai/zyvor-machina/blob/main/docs/INSTALL.md#air-gapped--offline-bundle).
2. Join more hosts from the same bundle:
   `sudo ./install.sh --agent --join https://<controller>:5094 --token-file /root/join.token --ca-sha256 <fp>`.
3. The installer never installs OS packages. Mirror your distribution's libvirt and QEMU packages internally.

## What reaches the internet by default, and where to point it

| Feature | Default source | Setting |
|---|---|---|
| Kubernetes bootstrap (k3s installer) | `get.k3s.io` | `MACHINA_K3S_INSTALL_URL` |
| metrics-server manifest | GitHub release | `MACHINA_METRICS_SERVER_MANIFEST_URL` |
| KubeVirt version and releases | `storage.googleapis.com`, GitHub | `MACHINA_KUBEVIRT_STABLE_URL`, `MACHINA_KUBEVIRT_RELEASES_URL` |
| CDI version and releases | GitHub API and releases | `MACHINA_CDI_LATEST_API_URL`, `MACHINA_CDI_RELEASES_URL` |
| Kata Containers | GitHub | `MACHINA_KATA_LATEST_API_URL`, `MACHINA_KATA_EXAMPLES_URL`, `MACHINA_KATA_HELM_CHART` |
| Guest-agent ISO | GitHub release | `iso_url` in the install request |
| Bare-metal OS images | The catalog lists Ubuntu, Debian, Rocky and AlmaLinux mirrors | Import by URL with an explicit checksum from an internal mirror ([guide](/docs/getting-started/provision-bare-metal)) |
| VM image templates | Public cloud-image URLs | Create the template from your own URL or upload the image |
| AI assistant | Hosted providers if you configure one | Use a local provider: `ollama`, `vllm` or `openai_compatible` with an internal base URL |
| "New version" banner | Off | `MACHINA_UPDATE_CHECK_URL` (your own feed) |

These Kubernetes variables are read by the daemon, so set them in its environment file. Container images that k3s, KubeVirt and
CDI pull at runtime come from the registries their manifests name; run an internal registry and mirror them (k3s reads
`/etc/rancher/k3s/registries.yaml`).

## Prove nothing leaves

On each host:

```bash
sudo ./scripts/airgap-check.sh            # lists established connections from Machina processes to public addresses
sudo ./scripts/airgap-check.sh --allow 10.20.0.5   # ignore a mirror you chose to use
```

It exits 1 and prints the connection if a Machina process talks to a public address, and 0 if every peer is private, loopback
or link-local. It is a point-in-time check, not a firewall: enforce the boundary on your network, and use the check as evidence.
Run it while exercising the features you use.

## What this does not cover

- Sovereign compliance is more than network isolation (key custody, audit retention, operator access). See
  [Security](./security) and the compliance guide.
- Fleet and OTA edge tooling are separate Zyvor products with their own offline modes.
- An offline *updater* for Machina itself is not built: upgrade by installing the next bundle over the old one.
- Not yet verified on a host with no default route: see claims C58.
