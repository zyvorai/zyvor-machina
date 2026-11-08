---
sidebar_position: 9
title: Machina for Mac
description: A native macOS client for your Machina host and fleet.
---

# Machina for Mac

A native SwiftUI app for macOS 15 or later that connects to a Machina daemon (`https://HOST:5092`) and, through it, the fleet controller. It needs no server changes.

## Install

Download `Machina-<version>.dmg` from the GitHub releases tagged `macos-v<version>`, open it and drag **Machina** to Applications. The DMG is signed with a Developer ID and notarized by Apple. The current release is 0.3.0.

**Updates:** *Check for Updates* finds the newest `macos-v*` release. From 0.3.0 on, the app can download and install it for you: it compares the SHA-256 in the release notes, requires the new app to be signed by the same Developer ID team and to pass Gatekeeper, asks you to confirm, then swaps the app and relaunches. If any check fails, nothing is installed.

## Sign in

Enter the server URL and a PAM user. The first connection asks you to trust the host's self-signed certificate; the app stores its SHA-256 pin in the Keychain and refuses a changed certificate. You can save several servers and switch between them. Single sign-on (OIDC/SAML) is not available in the app yet.

## What you can do

- Manage VMs: power, clone, rename, disks, NICs, snapshots, backups, metrics and logs, with live state updates and a VNC console.
- Run the fleet: hosts, tasks, alerts, approvals, DRS, Kubernetes and container pages, security and eBPF controls, bare metal, and Fleet Cloud (including a launch wizard).
- Operate the data plane and hardware: forms for the VMM guard, XDP shield, sched_ext scheduler and VM edge; bare metal BMC logins, image deploy, virtual media and serial-over-LAN; a chaos experiment editor; stack drift, converge and auto-heal; VPC routes and subnet NAT.
- See the fleet at a glance: topology map, VM grid with sparklines, capacity heatmap and an activity timeline.
- Work faster: a Cmd-K palette, pinned VMs, saved filters, bulk actions, CSV/JSON export, copy-as-curl and drag-and-drop ISO upload.
- Stay informed: a menu bar status glyph, notifications you can approve from (Touch ID protected) and quiet hours.

Every change is confirmed first and follows your role: operators can't use admin-only controls. eBPF enforcement needs an explicit lease and a typed phrase, node isolation starts as a dry run, BMC passwords are masked and never copied or exported, and destructive bare metal actions need the hostname typed.

## Known limits

The VM serial console is disabled until a daemon fix lands, and bare metal serial-over-LAN may not connect until the daemon proxies it. Load balancers have a single listener and stacks have no converge preview in the native controller API. Shortcuts and Siri integration needs a build made with Xcode. See the [app README](https://github.com/zyvorai/zyvor-machina/blob/main/macos/README.md) for the full list.
