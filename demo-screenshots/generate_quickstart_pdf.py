#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Generate a Quick Start Guide PDF for machina."""

from PIL import Image, ImageDraw, ImageFont
import os

DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(DIR, "machina-quickstart.pdf")
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

font_title = find_font(FONT_PATHS, 64)
font_heading = find_font(FONT_PATHS, 44)
font_subheading = find_font(FONT_PATHS, 32)
font_body = find_font(FONT_PATHS_REGULAR, 26)
font_code = find_font(FONT_PATHS_MONO, 24)
font_code_lg = find_font(FONT_PATHS_MONO, 28)
font_small = find_font(FONT_PATHS_REGULAR, 20)
font_label = find_font(FONT_PATHS, 22)

BG       = (13, 17, 23)
CODE_BG  = (22, 27, 34)
ACCENT   = (59, 130, 246)
WHITE    = (255, 255, 255)
GRAY     = (148, 163, 184)
LIGHT    = (203, 213, 225)
GREEN    = (34, 197, 94)
ORANGE   = (249, 115, 22)
RED      = (239, 68, 68)
CYAN     = (34, 211, 238)
YELLOW   = (250, 204, 21)
PURPLE   = (168, 85, 247)

def text_center(draw, y, text, font, fill=WHITE):
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    draw.text(((W - tw) // 2, y), text, font=font, fill=fill)

def draw_code_block(draw, x, y, w, lines, prompt="$"):
    pad = 20
    line_h = 36
    block_h = pad * 2 + len(lines) * line_h + 10
    # Background
    draw.rounded_rectangle([x, y, x + w, y + block_h], radius=12, fill=CODE_BG, outline=(50, 60, 75))
    # Terminal dots
    for i, color in enumerate([(255, 95, 86), (255, 189, 46), (39, 201, 63)]):
        draw.ellipse([x + 15 + i * 22, y + 12, x + 27 + i * 22, y + 24], fill=color)
    cy = y + pad + 20
    for line in lines:
        if line.startswith("#"):
            draw.text((x + pad, cy), line, font=font_code, fill=GRAY)
        elif line.startswith("$") or line.startswith(prompt):
            draw.text((x + pad, cy), prompt + " ", font=font_code, fill=GREEN)
            draw.text((x + pad + len(prompt + " ") * 14, cy), line[len(prompt)+1:], font=font_code, fill=LIGHT)
        elif line.startswith(">"):
            draw.text((x + pad, cy), line[1:].strip(), font=font_code, fill=CYAN)
        else:
            draw.text((x + pad, cy), line, font=font_code, fill=LIGHT)
        cy += line_h
    return block_h

def draw_step_number(draw, x, y, num, color=ACCENT):
    draw.ellipse([x, y, x + 44, y + 44], fill=color)
    bbox = draw.textbbox((0, 0), str(num), font=font_label)
    tw = bbox[2] - bbox[0]
    th = bbox[3] - bbox[1]
    draw.text((x + (44 - tw) // 2, y + (44 - th) // 2 - 2), str(num), font=font_label, fill=WHITE)

def draw_accent_bar(draw, n=4):
    for i in range(n):
        draw.rectangle([0, i, W, i + 1], fill=ACCENT)

def draw_divider(draw, y, color=ACCENT):
    draw.line([(W // 2 - 80, y), (W // 2 + 80, y)], fill=color, width=3)

# ── Slides ──────────────────────────────────────────────────────────────

def slide_title():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw, 6)
    text_center(draw, 220, "machina", font_title, WHITE)
    text_center(draw, 310, "Quick Start Guide", font_heading, ACCENT)
    draw_divider(draw, 390)
    text_center(draw, 440, "How to build, install, run, and operate your hypervisor control plane", font_body, GRAY)
    text_center(draw, 540, "Prerequisites: Fedora/RHEL/Ubuntu with libvirt + QEMU", font_body, LIGHT)
    text_center(draw, 590, "Rust toolchain (rustup) + Node.js 18+", font_body, LIGHT)
    text_center(draw, 700, "Estimated setup time: 5 minutes", font_subheading, GREEN)
    text_center(draw, 800, "https://github.com/zyvorai/machina", font_small, ACCENT)
    return img

def slide_prerequisites():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Step 0: Prerequisites", font_heading, WHITE)
    draw_divider(draw, 120)

    # Left column - Fedora/RHEL
    draw.text((100, 160), "Fedora / RHEL", font=font_subheading, fill=ACCENT)
    draw_code_block(draw, 100, 210, 820, [
        "# Install libvirt and QEMU",
        "$ sudo dnf install -y libvirt-devel qemu-kvm virt-install",
        "$ sudo systemctl enable --now libvirtd",
        "",
        "# Install Rust",
        "$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
        "",
        "# Install Node.js",
        "$ sudo dnf install -y nodejs npm",
    ])

    # Right column - Ubuntu/Debian
    draw.text((1000, 160), "Ubuntu / Debian", font=font_subheading, fill=GREEN)
    draw_code_block(draw, 1000, 210, 820, [
        "# Install libvirt and QEMU",
        "$ sudo apt install -y libvirt-dev qemu-kvm virtinst",
        "$ sudo systemctl enable --now libvirtd",
        "",
        "# Install Rust",
        "$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
        "",
        "# Install Node.js",
        "$ sudo apt install -y nodejs npm",
    ])

    draw.text((100, 620), "Verify:", font=font_subheading, fill=ORANGE)
    draw_code_block(draw, 100, 670, 1720, [
        "$ virsh list --all              # Should show libvirt is working",
        "$ cargo --version               # Should show rustc version",
        "$ node --version                # Should show v18+ ",
    ])
    return img

def slide_build():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Step 1: Clone and Build", font_heading, WHITE)
    draw_divider(draw, 120)

    draw_step_number(draw, 100, 170, 1)
    draw.text((160, 175), "Clone the repository", font=font_subheading, fill=LIGHT)
    draw_code_block(draw, 100, 230, 1720, [
        "$ git clone https://github.com/zyvorai/machina.git",
        "$ cd machina",
    ])

    draw_step_number(draw, 100, 380, 2, GREEN)
    draw.text((160, 385), "Build everything (Rust backend + React frontend)", font=font_subheading, fill=LIGHT)
    draw_code_block(draw, 100, 440, 1720, [
        "# Build Rust binaries (daemon + TUI) in release mode",
        "$ make release",
        "",
        "# Build web frontend",
        "$ make web",
        "",
        "# Or build everything in one command",
        "$ make all",
    ])

    draw.text((100, 760), "Build output:", font=font_subheading, fill=ORANGE)
    draw_code_block(draw, 100, 810, 1720, [
        "> target/release/machina-daemon    # REST API daemon (~10 MB)",
        "> target/release/machina           # Terminal TUI (~8 MB)",
        "> web/dist/                          # Web UI static files",
    ])
    return img

def slide_install():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Step 2: Install and Start", font_heading, WHITE)
    draw_divider(draw, 120)

    # Option A
    draw_step_number(draw, 100, 170, "A", GREEN)
    draw.text((160, 175), "One-command deploy (install + start)", font=font_subheading, fill=LIGHT)
    draw_code_block(draw, 100, 230, 1720, [
        "$ sudo make deploy",
        "",
        "> Installs to /usr/local/bin/",
        "> Web UI to /usr/local/share/machina/web/",
        "> Config to /etc/machina/config.toml",
        "> Enables and starts systemd service",
    ])

    # Option B
    draw_step_number(draw, 100, 510, "B", ORANGE)
    draw.text((160, 515), "Manual step-by-step", font=font_subheading, fill=LIGHT)
    draw_code_block(draw, 100, 570, 1720, [
        "$ sudo make install              # Install binaries and files",
        "$ sudo systemctl start machina-daemon   # Start the daemon",
        "$ sudo systemctl enable machina-daemon  # Auto-start on boot",
    ])

    draw.text((100, 780), "Verify it is running:", font=font_subheading, fill=CYAN)
    draw_code_block(draw, 100, 830, 1720, [
        "$ sudo systemctl status machina-daemon",
        "> Active: active (running)  ...  listening on 0.0.0.0:5092",
    ])
    return img

def slide_access():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Step 3: Access machina", font_heading, WHITE)
    draw_divider(draw, 120)

    # Three access methods
    cards = [
        ("Web UI", ACCENT, [
            "Open in your browser:",
            "",
            "http://localhost:5092",
            "",
            "Full dashboard with charts,",
            "VM management, storage,",
            "networks, and console access.",
        ]),
        ("Terminal TUI", GREEN, [
            "Run from any terminal:",
            "",
            "$ machina",
            "",
            "Keyboard-driven interface.",
            "j/k to navigate, s to start,",
            "x to stop, ? for help.",
        ]),
        ("REST API", ORANGE, [
            "curl from scripts/tools:",
            "",
            "$ curl localhost:5092/api/v1/vms",
            "$ curl localhost:5092/api/v1/node",
            "",
            "30+ JSON endpoints for",
            "full automation.",
        ]),
    ]

    col_w = 500
    gap = (W - col_w * 3) // 4
    for i, (title, color, lines) in enumerate(cards):
        x = gap + i * (col_w + gap)
        y = 180
        # Card background
        draw.rounded_rectangle([x, y, x + col_w, y + 520], radius=16, outline=color, width=2)
        # Card header
        draw.rounded_rectangle([x, y, x + col_w, y + 60], radius=16, fill=color)
        draw.rectangle([x, y + 40, x + col_w, y + 60], fill=color)
        bbox = draw.textbbox((0, 0), title, font=font_subheading)
        tw = bbox[2] - bbox[0]
        draw.text((x + (col_w - tw) // 2, y + 14), title, font=font_subheading, fill=WHITE)
        # Card body
        cy = y + 85
        for line in lines:
            if line.startswith("http") or line.startswith("$"):
                draw.text((x + 30, cy), line, font=font_code, fill=CYAN)
            else:
                draw.text((x + 30, cy), line, font=font_body, fill=LIGHT)
            cy += 42

    # Bottom note
    text_center(draw, 780, "Remote access: replace localhost with your server IP", font_body, GRAY)
    text_center(draw, 830, "Default port: 5092  |  Config: /etc/machina/config.toml", font_body, GRAY)
    text_center(draw, 900, "All three interfaces connect to the same daemon", font_body, YELLOW)
    return img

def slide_web_ui_tour():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Web UI Pages", font_heading, WHITE)
    draw_divider(draw, 120)

    pages = [
        ("Dashboard", "/", "Overview with VM stats, CPU/Memory charts, quick actions", ACCENT),
        ("Virtual Machines", "/vms", "List, search, start/stop/reboot, console, delete", GREEN),
        ("VM Details", "/vms/{name}", "Configuration, boot config, disks, network, snapshots", CYAN),
        ("Create VM", "/create", "Template picker + custom config (vCPU, memory, disk, ISO)", ORANGE),
        ("Storage", "/storage", "Pool management, volume browsing, resize, clone", PURPLE),
        ("Networks", "/networks", "Virtual networks with autostart toggle", GREEN),
        ("Capabilities", "/capabilities", "Host architecture and supported guest types", ACCENT),
        ("Devices", "/devices", "PCI, USB, network device inventory", CYAN),
        ("Network Filters", "/nwfilters", "Security rules for VM network traffic", ORANGE),
        ("Console", "/vms/{name}/console", "VNC graphical + Serial text console in browser", RED),
    ]

    y = 160
    for name, path, desc, color in pages:
        draw.rounded_rectangle([100, y, 1820, y + 50], radius=8, fill=(22, 27, 34))
        draw.text((120, y + 10), name, font=font_label, fill=color)
        draw.text((380, y + 12), path, font=font_code, fill=GRAY)
        draw.text((720, y + 12), desc, font=font_small, fill=LIGHT)
        y += 60

    text_center(draw, 790, "Navigation: Top navbar with Dashboard, Virtual Machines,", font_body, GRAY)
    text_center(draw, 830, "Infrastructure (Storage, Networks, NW Filters), Monitoring (Capabilities, Devices)", font_body, GRAY)
    return img

def slide_api_reference():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "REST API Quick Reference", font_heading, WHITE)
    draw_divider(draw, 120)

    endpoints = [
        ("GET", "/api/v1/health", "Health check", GREEN),
        ("GET", "/api/v1/vms", "List all VMs", GREEN),
        ("GET", "/api/v1/vms/{name}", "VM details", GREEN),
        ("POST", "/api/v1/vms", "Create VM", ACCENT),
        ("POST", "/api/v1/vms/{name}/start", "Start VM", ACCENT),
        ("POST", "/api/v1/vms/{name}/shutdown", "Graceful shutdown", ACCENT),
        ("DELETE", "/api/v1/vms/{name}", "Delete VM", RED),
        ("GET", "/api/v1/networks", "List networks", GREEN),
        ("GET", "/api/v1/storage/pools", "List storage pools", GREEN),
        ("GET", "/api/v1/metrics", "Live VM metrics", GREEN),
        ("GET", "/api/v1/node", "Host node info", GREEN),
        ("GET", "/api/v1/capabilities", "Hypervisor capabilities", GREEN),
        ("POST", "/api/v1/vms/{name}/migrate", "Migrate VM", ORANGE),
    ]

    y = 150
    # Header
    draw.rounded_rectangle([100, y, 1820, y + 40], radius=6, fill=ACCENT)
    draw.text((120, y + 8), "Method", font=font_label, fill=WHITE)
    draw.text((280, y + 8), "Endpoint", font=font_label, fill=WHITE)
    draw.text((900, y + 8), "Description", font=font_label, fill=WHITE)
    y += 50

    for method, path, desc, color in endpoints:
        bg = (22, 27, 34) if endpoints.index((method, path, desc, color)) % 2 == 0 else BG
        draw.rounded_rectangle([100, y, 1820, y + 42], radius=4, fill=bg)
        draw.text((120, y + 9), method, font=font_label, fill=color)
        draw.text((280, y + 10), path, font=font_code, fill=LIGHT)
        draw.text((900, y + 10), desc, font=font_body, fill=GRAY)
        y += 44

    text_center(draw, 830, "Full API: 30+ endpoints covering VMs, networks, storage, snapshots, devices, and more", font_body, GRAY)
    text_center(draw, 880, "All responses are JSON  |  Errors include descriptive messages", font_body, GRAY)
    return img

def slide_tui_keys():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "TUI Keyboard Shortcuts", font_heading, WHITE)
    draw_divider(draw, 120)

    groups = [
        ("Navigation", [
            ("j / k", "Move down / up"),
            ("h / l", "Sidebar / Content focus"),
            ("Enter", "Select item"),
            ("Tab", "Cycle sub-tabs"),
            ("/ ", "Search (fuzzy)"),
            (": ", "Command mode"),
            ("?", "Help"),
        ], ACCENT),
        ("VM Actions", [
            ("s", "Start VM"),
            ("x", "Force stop"),
            ("H", "Graceful shutdown"),
            ("b", "Reboot"),
            ("p / u", "Pause / Resume"),
            ("d", "Delete"),
            ("n", "New VM dialog"),
        ], GREEN),
        ("Advanced", [
            ("v", "Open virt-viewer"),
            ("c", "Serial console"),
            ("y", "View XML"),
            ("l", "View logs"),
            ("t", "Toggle autostart"),
            ("Space", "Multi-select"),
            ("A", "Select all VMs"),
        ], ORANGE),
    ]

    col_w = 520
    gap = (W - col_w * 3) // 4
    for i, (title, keys, color) in enumerate(groups):
        x = gap + i * (col_w + gap)
        y = 170
        draw.text((x, y), title, font=font_subheading, fill=color)
        y += 50
        for key, desc in keys:
            draw.rounded_rectangle([x, y, x + col_w, y + 44], radius=6, fill=CODE_BG)
            draw.text((x + 15, y + 9), key, font=font_code, fill=CYAN)
            draw.text((x + 130, y + 10), desc, font=font_body, fill=LIGHT)
            y += 52

    text_center(draw, 600, "Commands (type : then the command):", font=font_subheading, fill=YELLOW)
    draw_code_block(draw, 200, 650, 1520, [
        ":create myvm              Create VM with defaults",
        ":create myvm 4 4096       Create VM with 4 vCPUs, 4 GB RAM",
        ":clone source-vm new-vm   Clone a VM",
        ":resize myvm vcpus 8      Change vCPU count",
        ":resize myvm memory 8192  Change memory (MB)",
        ":snap myvm backup1        Create snapshot",
        ":template linux-large vm1 Create from template",
    ])
    return img

def slide_config():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Configuration", font_heading, WHITE)
    draw_divider(draw, 120)

    draw.text((100, 170), "Config file: /etc/machina/config.toml", font=font_subheading, fill=ACCENT)
    draw_code_block(draw, 100, 220, 820, [
        "[general]",
        "refresh_interval_secs = 5",
        "",
        "[daemon]",
        'host = "0.0.0.0"',
        "port = 5092",
        "",
        "[libvirt]",
        'uri = "qemu:///system"',
    ])

    draw.text((1000, 170), "Systemd service management", font=font_subheading, fill=GREEN)
    draw_code_block(draw, 1000, 220, 820, [
        "# Start / stop / restart",
        "$ sudo make start",
        "$ sudo make stop",
        "$ sudo make restart",
        "",
        "# Check status",
        "$ sudo make status",
        "$ sudo systemctl status machina-daemon",
    ])

    draw.text((100, 610), "CLI options (override config):", font=font_subheading, fill=ORANGE)
    draw_code_block(draw, 100, 660, 1720, [
        "$ machina-daemon --host 0.0.0.0 --port 9090 --libvirt-uri qemu:///system",
        "$ machina-daemon --config /path/to/custom-config.toml",
        "",
        "# Run in foreground (debug mode)",
        "$ make run-daemon",
    ])
    return img

def slide_troubleshooting():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw)
    text_center(draw, 50, "Troubleshooting", font_heading, WHITE)
    draw_divider(draw, 120)

    issues = [
        ("Cannot connect to libvirt", RED, [
            "$ sudo systemctl start libvirtd",
            "$ sudo usermod -aG libvirt $USER   # then re-login",
            "$ virsh list --all                  # test connection",
        ]),
        ("Port 5092 already in use", ORANGE, [
            "$ sudo ss -tlnp | grep 5092        # find what's using it",
            "# Change port in /etc/machina/config.toml",
            "$ sudo systemctl restart machina-daemon",
        ]),
        ("Web UI not loading", YELLOW, [
            "$ ls /usr/local/share/machina/web/index.html",
            "# If missing, rebuild: make web && sudo make install",
            "$ curl -s http://localhost:5092/api/v1/health",
        ]),
        ("Permission denied errors", PURPLE, [
            "$ sudo systemctl status machina-daemon  # check logs",
            "$ sudo journalctl -u machina-daemon -f  # live logs",
            "# Daemon runs as root by default for libvirt access",
        ]),
    ]

    y = 160
    for title, color, cmds in issues:
        draw.text((100, y), title, font=font_subheading, fill=color)
        y += 45
        h = draw_code_block(draw, 100, y, 1720, cmds)
        y += h + 20
    return img

def slide_summary():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    draw_accent_bar(draw, 6)

    text_center(draw, 160, "Quick Reference", font_heading, WHITE)
    draw_divider(draw, 230)

    draw_code_block(draw, 300, 280, 1320, [
        "# Build",
        "$ git clone https://github.com/zyvorai/machina.git",
        "$ cd machina && make all",
        "",
        "# Deploy",
        "$ sudo make deploy",
        "",
        "# Access",
        "> Web UI:   http://localhost:5092",
        "> TUI:      machina",
        "> API:      curl http://localhost:5092/api/v1/health",
        "",
        "# Manage",
        "$ sudo make status | start | stop | restart",
    ])

    text_center(draw, 830, "machina  —  Linux hypervisor host manager (libvirt/KVM, optional KubeVirt)", font_subheading, ACCENT)
    text_center(draw, 890, "Built with Rust + React  |  Secure  |  Fast  |  Production-Ready", font_body, GRAY)
    text_center(draw, 950, "https://github.com/zyvorai/machina", font_small, ACCENT)
    return img

# ── Build PDF ───────────────────────────────────────────────────────────

slides = [
    slide_title(),
    slide_prerequisites(),
    slide_build(),
    slide_install(),
    slide_access(),
    slide_web_ui_tour(),
    slide_api_reference(),
    slide_tui_keys(),
    slide_config(),
    slide_troubleshooting(),
    slide_summary(),
]

slides[0].save(OUT, save_all=True, append_images=slides[1:], resolution=150)
print(f"Generated {OUT} ({len(slides)} slides, {os.path.getsize(OUT) // 1024} KB)")
