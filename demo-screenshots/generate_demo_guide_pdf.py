#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Generate Demo & Scripts Guide PDF for machina."""

from PIL import Image, ImageDraw, ImageFont
import os

DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(DIR, "machina-demo-scripts-guide.pdf")
W, H = 1920, 1080

FONT_PATHS = [
    "/usr/share/fonts/google-noto-sans-fonts/NotoSans-Bold.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation-sans/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
]
FONT_PATHS_REGULAR = [p.replace("-Bold", "-Regular").replace("Bold", "Regular") for p in FONT_PATHS]
FONT_PATHS_MONO = [
    "/usr/share/fonts/google-noto-sans-mono-fonts/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/dejavu-sans-mono-fonts/DejaVuSansMono.ttf",
    "/usr/share/fonts/liberation-mono/LiberationMono-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
]

def find_font(paths, size):
    for p in paths:
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    return ImageFont.load_default()

ft = find_font(FONT_PATHS, 64)
fh = find_font(FONT_PATHS, 44)
fsh = find_font(FONT_PATHS, 32)
fb = find_font(FONT_PATHS_REGULAR, 24)
fc = find_font(FONT_PATHS_MONO, 22)
fcl = find_font(FONT_PATHS_MONO, 20)
fs = find_font(FONT_PATHS_REGULAR, 20)
fl = find_font(FONT_PATHS, 22)
fbig = find_font(FONT_PATHS, 28)

BG = (13, 17, 23); CODE_BG = (22, 27, 34); ACCENT = (59, 130, 246)
WHITE = (255, 255, 255); GRAY = (148, 163, 184); LIGHT = (203, 213, 225)
GREEN = (34, 197, 94); ORANGE = (249, 115, 22); RED = (239, 68, 68)
CYAN = (34, 211, 238); YELLOW = (250, 204, 21); PURPLE = (168, 85, 247)

def tc(d, y, text, font, fill=WHITE):
    bb = d.textbbox((0, 0), text, font=font)
    d.text(((W - bb[2] + bb[0]) // 2, y), text, font=font, fill=fill)

def bar(d, n=4):
    for i in range(n): d.rectangle([0, i, W, i + 1], fill=ACCENT)

def div(d, y):
    d.line([(W // 2 - 80, y), (W // 2 + 80, y)], fill=ACCENT, width=3)

def code_block(d, x, y, w, lines):
    pad, lh = 16, 30
    h = pad * 2 + len(lines) * lh + 8
    d.rounded_rectangle([x, y, x + w, y + h], radius=10, fill=CODE_BG, outline=(50, 60, 75))
    for i, c in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        d.ellipse([x + 12 + i * 18, y + 10, x + 22 + i * 18, y + 20], fill=c)
    cy = y + pad + 16
    for line in lines:
        if line.startswith("#"):
            d.text((x + pad, cy), line, font=fcl, fill=GRAY)
        elif line.startswith("$"):
            d.text((x + pad, cy), "$ ", font=fcl, fill=GREEN)
            d.text((x + pad + 28, cy), line[2:], font=fcl, fill=LIGHT)
        elif line.startswith(">"):
            d.text((x + pad, cy), line[1:].strip(), font=fcl, fill=CYAN)
        else:
            d.text((x + pad, cy), line, font=fcl, fill=LIGHT)
        cy += lh
    return h

def card(d, x, y, w, h, title, items, color):
    d.rounded_rectangle([x, y, x + w, y + h], radius=14, outline=color, width=2)
    d.rounded_rectangle([x, y, x + w, y + 52], radius=14, fill=color)
    d.rectangle([x, y + 38, x + w, y + 52], fill=color)
    bb = d.textbbox((0, 0), title, font=fsh)
    tw = bb[2] - bb[0]
    d.text((x + (w - tw) // 2, y + 10), title, font=fsh, fill=WHITE)
    cy = y + 70
    for item in items:
        d.text((x + 20, cy), "  " + item, font=fb, fill=LIGHT)
        cy += 36

# ── Slides ──────────────────────────────────────────────────────────────

def slide_title():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d, 6)
    tc(d, 200, "machina", ft, WHITE)
    tc(d, 290, "Demo & Scripts Guide", fh, ACCENT)
    div(d, 370)
    tc(d, 430, "30-step API demo, status dashboard, backup & restore, bulk operations", fb, GRAY)
    tc(d, 510, "scripts/demo.sh    scripts/status.sh", fc, CYAN)
    tc(d, 555, "scripts/backup.sh  scripts/bulk.sh", fc, CYAN)
    tc(d, 650, "All scripts use the REST API — no direct libvirt access needed", fb, LIGHT)
    tc(d, 710, "Works locally or against any remote machina instance", fb, LIGHT)
    tc(d, 850, "https://github.com/zyvorai/machina", fs, ACCENT)
    return img

def slide_demo_overview():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "API Demo Script — 30 Steps", fh, WHITE)
    div(d, 110)

    d.text((80, 140), "Run it:", font=fsh, fill=GREEN)
    code_block(d, 80, 185, 500, [
        "$ sudo ./scripts/demo.sh",
        "",
        "# Or against a remote host:",
        "$ ./scripts/demo.sh http://server:5092/api/v1",
    ])

    steps = [
        ("Steps 1-3", "Health, Node Info, Templates", GREEN),
        ("Steps 4-7", "List VMs, Create VM, Details, Start", ACCENT),
        ("Steps 8-12", "Metrics, Console Info, Boot Config, Managed Save, Autostart", CYAN),
        ("Steps 13-15", "Create Snapshot, List Snapshots, Revert", PURPLE),
        ("Steps 16-17", "List Networks, List Storage Pools", ORANGE),
        ("Steps 18-23", "Capabilities, Sysinfo, Devices, Device Filter, NW Filters, Secrets", ACCENT),
        ("Steps 24-25", "Security: SSRF Blocked, Resize Validation Blocked", RED),
        ("Steps 26", "Prometheus Metrics", GREEN),
        ("Steps 27-30", "Shutdown, Delete Snapshot, Delete VM, Final VM List", ORANGE),
    ]

    x, y = 640, 140
    for label, desc, color in steps:
        d.rounded_rectangle([x, y, x + 1200, y + 42], radius=6, fill=CODE_BG)
        d.text((x + 15, y + 9), label, font=fl, fill=color)
        d.text((x + 200, y + 10), desc, font=fb, fill=LIGHT)
        y += 50

    d.text((80, 650), "What it tests:", font=fsh, fill=YELLOW)
    checks = [
        "Full VM lifecycle: create -> start -> metrics -> snapshot -> revert -> shutdown -> delete",
        "All infrastructure endpoints: capabilities, sysinfo, devices, nwfilters, secrets",
        "Device capability filtering (?capability=net)",
        "Security validation: SSRF prevention on migrate, negative capacity on resize",
        "Prometheus metrics export",
        "Automatic cleanup: demo VM and snapshot deleted at the end",
    ]
    y = 700
    for check in checks:
        d.text((100, y), "  " + check, font=fb, fill=LIGHT)
        y += 34
    return img

def slide_demo_output():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Demo Output Examples", fh, WHITE)

    d.text((80, 110), "VM Created & Started", font=fsh, fill=GREEN)
    code_block(d, 80, 155, 860, [
        "[5] Create a Demo VM",
        '  POST /vms',
        '  {"name":"demo-vm-466465","status":"created"}',
        "",
        "[7] Start VM",
        '  POST /vms/demo-vm-466465/start',
        '  {"name":"demo-vm-466465","status":"started"}',
    ])

    d.text((980, 110), "Metrics & Console", font=fsh, fill=CYAN)
    code_block(d, 980, 155, 860, [
        "[8] Get VM Metrics",
        '  GET /metrics/demo-vm-466465',
        '  {"cpu_time_ns":450000000,',
        '   "memory_pct":0.0,',
        '   "disk_rd_bytes":512,',
        '   "net_rx_bytes":90}',
    ])

    d.text((80, 440), "Security Validation", font=fsh, fill=RED)
    code_block(d, 80, 485, 860, [
        "[24] Migration URI Validation",
        '  POST /vms/demo-vm/migrate',
        '  {"dest_uri":"http://evil.com"}',
        "",
        '  BLOCKED: Invalid migration URI scheme.',
        '  Allowed: qemu://, qemu+ssh://,',
        '           qemu+tcp://, qemu+tls://',
    ])

    d.text((980, 440), "Capabilities Discovery", font=fsh, fill=ACCENT)
    code_block(d, 980, 485, 860, [
        "[18] Hypervisor Capabilities",
        "  Host arch: x86_64",
        "  CPU model: Skylake-Client-v3",
        "  Guest types: 2",
        "",
        "[20] Node Devices",
        "  Total: 56 devices",
        "    pci:25 usb:12 net:8 drm:2 ...",
    ])

    d.text((80, 800), "Final result:", font=fsh, fill=YELLOW)
    d.text((300, 805), "30/30 steps pass, demo VM cleaned up, back to original VM list", font=fb, fill=LIGHT)
    return img

def slide_status():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Status Script", fh, WHITE)
    div(d, 110)

    d.text((80, 140), "One-screen environment overview:", font=fsh, fill=GREEN)
    code_block(d, 80, 185, 500, [
        "$ ./scripts/status.sh",
        "",
        "# Remote daemon:",
        "$ ./scripts/status.sh http://server:5092/api/v1",
    ])

    d.text((640, 140), "Shows:", font=fsh, fill=ACCENT)
    sections = [
        ("Host info", "hostname, hypervisor, CPU cores, RAM"),
        ("VM list", "name, state (color-coded), vCPUs, memory"),
        ("Live metrics", "memory %, disk read/write, net RX/TX per VM"),
        ("Networks", "name, active/inactive, bridge, autostart"),
        ("Storage pools", "name, state, usage bar with percentage"),
        ("Snapshots", "VM name, snapshot name, state, current"),
        ("Service", "systemd status, PID, memory usage"),
    ]
    y = 185
    for label, desc in sections:
        d.rounded_rectangle([640, y, 1840, y + 42], radius=6, fill=CODE_BG)
        d.text((655, y + 9), label, font=fl, fill=CYAN)
        d.text((900, y + 10), desc, font=fb, fill=LIGHT)
        y += 50

    d.text((80, 580), "Example output:", font=fsh, fill=ORANGE)
    code_block(d, 80, 625, 1760, [
        "machina status",
        "",
        "Daemon: healthy  (http://localhost:5092/api/v1)",
        "Host:       myserver.example.com",
        "Hypervisor: QEMU 10.1.4  (libvirt 11.6.0)",
        "Hardware:   14 CPUs, 30 GB RAM",
        "",
        "Virtual Machines (5 total, 2 running, 3 stopped)",
        "  * fedora-test        running       4 vCPU   4096 MB",
        "    photon-os          shutoff       2 vCPU   2048 MB",
        "",
        "Storage Pools (1)",
        "  default         running   [#############-------]  65.7%  312.3/475.3 GB",
    ])
    return img

def slide_backup():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Backup & Restore", fh, WHITE)
    div(d, 110)

    d.text((80, 130), "Backup modes:", font=fsh, fill=GREEN)
    code_block(d, 80, 172, 860, [
        "# Full backup (all VMs, networks, pools)",
        "$ ./scripts/backup.sh",
        "",
        "# Per-VM backup",
        "$ ./scripts/backup.sh --vm photon-os",
        "",
        "# With disk images + incremental",
        "$ ./scripts/backup.sh --with-disks --incremental",
        "",
        "# NFS target with retention",
        "$ ./scripts/backup.sh --nfs 192.168.1.10:/bk --retain 7",
    ])

    d.text((980, 130), "Restore & Verify:", font=fsh, fill=ORANGE)
    code_block(d, 980, 172, 860, [
        "# Restore VMs, networks, pools, disks",
        "$ ./scripts/backup.sh --restore \\",
        "    /var/lib/machina/backups/20260324-020000",
        "",
        "# Verify checksums",
        "$ ./scripts/backup.sh --verify \\",
        "    /var/lib/machina/backups/20260324-020000",
        "",
        "# Scheduled backups (systemd timer)",
        "$ sudo systemctl enable --now machina-backup.timer",
    ])

    d.text((80, 560), "Features:", font=fsh, fill=CYAN)
    features = [
        "SHA-256 checksums on every backup",
        "Status tracking with progress %",
        "Incremental: rsync hardlinks for unchanged disks",
        "NFS auto-mount/unmount",
        "Retention policy (keep last N)",
        "Download as tar.gz from web UI",
        "Per-VM backup button on VM details page",
        "Schedule enable/disable from web UI",
    ]
    y = 610
    for f in features:
        d.text((100, y), "  " + f, font=fb, fill=LIGHT)
        y += 34

    d.text((980, 560), "Config file:", font=fsh, fill=PURPLE)
    code_block(d, 980, 605, 860, [
        "# /etc/machina/backup.conf",
        "backup_dir = /var/lib/machina/backups",
        "nfs_target =",
        "with_disks = false",
        "retain = 7",
    ])
    return img

def slide_bulk():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Bulk Operations Script", fh, WHITE)
    div(d, 110)

    actions = [
        ("start", "Start all stopped VMs", "shutoff -> running", GREEN),
        ("stop", "Force stop all running VMs", "running -> shutoff (immediate)", RED),
        ("shutdown", "Graceful shutdown all running", "running -> shutoff (ACPI)", ORANGE),
        ("pause", "Pause all running VMs", "running -> paused", YELLOW),
        ("resume", "Resume all paused VMs", "paused -> running", GREEN),
        ("reboot", "Reboot all running VMs", "running -> restart", CYAN),
        ("snapshot", "Auto-timestamped snapshot", "Creates auto-YYYYMMDD-HHMMSS", PURPLE),
        ("snapshot-clean", "Delete all auto-* snapshots", "Removes auto-prefixed only", RED),
        ("status", "Quick VM status table", "Shows all VMs with state", ACCENT),
    ]

    y = 140
    d.rounded_rectangle([80, y, 1840, y + 36], radius=6, fill=ACCENT)
    d.text((95, y + 6), "Action", font=fl, fill=WHITE)
    d.text((320, y + 6), "Description", font=fl, fill=WHITE)
    d.text((780, y + 6), "Effect", font=fl, fill=WHITE)
    d.text((1300, y + 6), "Command", font=fl, fill=WHITE)
    y += 42

    for action, desc, effect, color in actions:
        bg = CODE_BG if actions.index((action, desc, effect, color)) % 2 == 0 else BG
        d.rounded_rectangle([80, y, 1840, y + 40], radius=4, fill=bg)
        d.text((95, y + 8), action, font=fl, fill=color)
        d.text((320, y + 10), desc, font=fb, fill=LIGHT)
        d.text((780, y + 10), effect, font=fb, fill=GRAY)
        d.text((1300, y + 10), f"./scripts/bulk.sh {action}", font=fcl, fill=CYAN)
        y += 44

    d.text((80, 560), "Examples:", font=fsh, fill=ORANGE)
    code_block(d, 80, 605, 860, [
        "# Start all stopped VMs",
        "$ ./scripts/bulk.sh start",
        "",
        "# Shutdown only specific VMs",
        "$ ./scripts/bulk.sh shutdown vm1 vm2 vm3",
        "",
        "# Snapshot all running, then clean old ones",
        "$ ./scripts/bulk.sh snapshot",
        "$ ./scripts/bulk.sh snapshot-clean",
    ])

    d.text((980, 605), "Quick status:", font=fsh, fill=ACCENT)
    code_block(d, 980, 650, 860, [
        "$ ./scripts/bulk.sh status",
        "",
        "Total: 5  Running: 2  Stopped: 3",
        "",
        "  running   fedora-test    4 vCPU  4096 MB",
        "  running   photon-os      2 vCPU  2048 MB",
        "  shutoff   ubuntu-test    4 vCPU  4096 MB",
    ])
    return img

def slide_all_scripts():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "All Scripts at a Glance", fh, WHITE)
    div(d, 110)

    scripts = [
        ("scripts/demo.sh", "30-step API demo", "Tests all endpoints, creates & deletes a VM, validates security", GREEN),
        ("scripts/status.sh", "Environment overview", "Host, VMs, metrics, networks, storage, service status", CYAN),
        ("scripts/backup.sh", "Backup & restore", "Per-VM, NFS, incremental, checksums, scheduled timer", ORANGE),
        ("scripts/bulk.sh", "Batch operations", "Start/stop/shutdown/snapshot all VMs at once", PURPLE),
        ("install.sh", "Automated installer", "Dependencies, build, install, start, 15 verification tests", ACCENT),
    ]

    col_w = 520
    gap = 50
    for i, (name, title, desc, color) in enumerate(scripts):
        col = i % 3
        row = i // 3
        x = 80 + col * (col_w + gap)
        y = 160 + row * 380
        card(d, x, y, col_w, 340, title, [
            f"  {name}",
            "",
        ] + [f"  {line}" for line in desc.split(", ")], color)

    tc(d, 920, "All scripts use the REST API — work locally or against any remote machina instance", fb, GRAY)
    tc(d, 960, "Set MACHINA_API=http://remote:5092/api/v1 for remote operation", fc, CYAN)
    return img

def slide_summary():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d, 6)
    tc(d, 150, "Quick Reference", fh, WHITE)
    div(d, 220)

    code_block(d, 300, 270, 1320, [
        "# Run the 30-step API demo",
        "$ sudo ./scripts/demo.sh",
        "",
        "# Check environment status",
        "$ ./scripts/status.sh",
        "",
        "# Backup all VM configs",
        "$ ./scripts/backup.sh",
        "",
        "# Start all stopped VMs",
        "$ ./scripts/bulk.sh start",
        "",
        "# Snapshot all running VMs",
        "$ ./scripts/bulk.sh snapshot",
        "",
        "# Shutdown everything gracefully",
        "$ ./scripts/bulk.sh shutdown",
    ])

    tc(d, 870, "machina  —  Linux hypervisor host manager (libvirt/KVM, optional KubeVirt)", fsh, ACCENT)
    tc(d, 920, "https://github.com/zyvorai/machina", fs, ACCENT)
    return img

slides = [
    slide_title(),
    slide_demo_overview(),
    slide_demo_output(),
    slide_status(),
    slide_backup(),
    slide_bulk(),
    slide_all_scripts(),
    slide_summary(),
]

slides[0].save(OUT, save_all=True, append_images=slides[1:], resolution=150)
print(f"Generated {OUT} ({len(slides)} slides, {os.path.getsize(OUT) // 1024} KB)")
