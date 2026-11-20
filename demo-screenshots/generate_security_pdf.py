#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Generate Security & Architecture PDF for machina."""

from PIL import Image, ImageDraw, ImageFont
import os

DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(DIR, "machina-security-architecture.pdf")
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
fs = find_font(FONT_PATHS_REGULAR, 20)
fl = find_font(FONT_PATHS, 22)
fbig = find_font(FONT_PATHS, 28)

BG = (13, 17, 23); CODE_BG = (22, 27, 34); ACCENT = (59, 130, 246)
WHITE = (255, 255, 255); GRAY = (148, 163, 184); LIGHT = (203, 213, 225)
GREEN = (34, 197, 94); ORANGE = (249, 115, 22); RED = (239, 68, 68)
CYAN = (34, 211, 238); YELLOW = (250, 204, 21); PURPLE = (168, 85, 247)

def tc(d, y, text, font, fill=WHITE):
    bb = d.textbbox((0,0), text, font=font)
    d.text(((W - bb[2] + bb[0]) // 2, y), text, font=font, fill=fill)

def bar(d, n=4):
    for i in range(n): d.rectangle([0, i, W, i+1], fill=ACCENT)

def div(d, y):
    d.line([(W//2-80, y), (W//2+80, y)], fill=ACCENT, width=3)

def card(d, x, y, w, h, title, items, color, icon=None):
    d.rounded_rectangle([x, y, x+w, y+h], radius=14, outline=color, width=2)
    d.rounded_rectangle([x, y, x+w, y+52], radius=14, fill=color)
    d.rectangle([x, y+38, x+w, y+52], fill=color)
    bb = d.textbbox((0,0), title, font=fsh)
    tw = bb[2]-bb[0]
    d.text((x + (w-tw)//2, y+10), title, font=fsh, fill=WHITE)
    cy = y + 70
    for item in items:
        d.text((x+20, cy), "  " + item, font=fb, fill=LIGHT)
        cy += 36
    return cy

def shield(d, x, y, label, color):
    d.rounded_rectangle([x, y, x+340, y+50], radius=10, fill=CODE_BG, outline=color, width=2)
    d.ellipse([x+12, y+13, x+36, y+37], fill=color)
    d.text((x+14, y+13), "\u2713", font=fl, fill=WHITE)
    d.text((x+48, y+12), label, font=fb, fill=LIGHT)

# ── Slides ──────────────────────────────────────────────────────────────

def slide_title():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d, 6)
    tc(d, 200, "machina", ft, WHITE)
    tc(d, 290, "Security & Architecture", fh, ACCENT)
    div(d, 370)
    tc(d, 430, "Defense-in-depth approach to VM management security", fb, GRAY)
    tc(d, 490, "Input validation | SSRF prevention | Audit logging | Panic-free design", fb, LIGHT)
    tc(d, 600, "Built with Rust for memory safety", fsh, GREEN)
    tc(d, 660, "Zero unsafe blocks | No C dependencies in core logic", fb, GRAY)
    tc(d, 800, "https://github.com/zyvorai/machina", fs, ACCENT)
    return img

def slide_architecture():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "System Architecture", fh, WHITE)

    # Browser -> Daemon -> libvirt -> QEMU/KVM
    boxes = [
        (60, 200, 340, 300, "Browser / Client", ["Web UI (React)", "REST API calls", "WebSocket events"], ACCENT),
        (440, 200, 340, 300, "machina-daemon", ["Axum HTTP server", "Input validation", "Route handlers"], GREEN),
        (840, 200, 340, 300, "machina-core", ["LibvirtManager", "XML parsing", "State management"], ORANGE),
        (1240, 200, 340, 300, "libvirt", ["Domain mgmt", "Storage mgmt", "Network mgmt"], PURPLE),
        (1640, 200, 220, 300, "QEMU/KVM", ["VM execution", "Hardware virt"], RED),
    ]
    for x, y, w, h, title, items, color in boxes:
        card(d, x, y, w, h, title, items, color)

    # Arrows
    for sx in [400, 780, 1180, 1580]:
        d.polygon([(sx, 345), (sx+30, 350), (sx, 355)], fill=GRAY)
        d.line([(sx-15, 350), (sx+30, 350)], fill=GRAY, width=2)

    # Bottom layer
    d.text((60, 550), "Data Flow", font=fsh, fill=CYAN)
    flows = [
        ("HTTP/WS Request", "JSON validation", "Rust type-safe call", "libvirt C API", "QEMU monitor"),
    ]
    x = 60
    for label in flows[0]:
        d.rounded_rectangle([x, 600, x+320, 640], radius=8, fill=CODE_BG)
        d.text((x+15, 608), label, font=fb, fill=LIGHT)
        if x < 1400:
            d.text((x+330, 608), "->", font=fb, fill=GRAY)
        x += 370

    # Security boundaries
    d.text((60, 700), "Security Boundaries", font=fsh, fill=RED)
    boundaries = [
        (60, "Same-origin policy (no CORS)"),
        (500, "Input validation + XML escaping"),
        (1000, "Mutex-protected connection"),
        (1500, "libvirt ACLs"),
    ]
    for bx, label in boundaries:
        d.rounded_rectangle([bx, 750, bx+400, 790], radius=8, fill=(40, 20, 20), outline=RED)
        d.text((bx+15, 758), label, font=fb, fill=RED)

    # Single binary
    d.text((60, 850), "Deployment: single binary + static web assets | systemd managed | < 10 MB total", font=fb, fill=GRAY)
    return img

def slide_input_validation():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Input Validation", fh, WHITE)
    div(d, 105)

    validations = [
        ("VM / Resource Names", "validate_name()", [
            "1-64 characters",
            "Alphanumeric, dash, underscore, dot only",
            "Cannot start with dash or dot",
            "Prevents XML/shell injection in names",
        ], GREEN),
        ("vCPU Count", "validate_vcpus()", [
            "Range: 1 - 256",
            "u32 type prevents negative values",
            "Validated before libvirt call",
        ], ACCENT),
        ("Memory (MB)", "validate_memory_mb()", [
            "Range: 64 MB - 1,048,576 MB (1 TB)",
            "Applied to both set_memory and balloon",
            "Prevents destabilizing guests with 0 MB",
        ], ORANGE),
        ("Disk Size (GB)", "validate_disk_gb()", [
            "Range: 1 GB - 10,240 GB (10 TB)",
            "checked_mul() prevents integer overflow",
            "capacity_gb > 0 check in resize handler",
        ], CYAN),
        ("Migration URI", "validate_migrate_uri()", [
            "Allowed schemes: qemu://, qemu+ssh://,",
            "  qemu+tcp://, qemu+tls://, qemu+unix://",
            "Blocks SSRF via http://, file://, etc.",
        ], RED),
        ("Network IPs", "validate_ip() + validate_subnet()", [
            "DHCP start/end: valid IPv4 format",
            "Subnet: 3-octet prefix (e.g. 192.168.100)",
            "Prevents malformed XML injection",
        ], PURPLE),
    ]

    col_w = 560
    gap = 50
    for i, (title, func, items, color) in enumerate(validations):
        col = i % 3
        row = i // 3
        x = 80 + col * (col_w + gap)
        y = 140 + row * 370
        card(d, x, y, col_w, 340, title, [func] + [""] + items, color)

    return img

def slide_ssrf_injection():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "SSRF & Injection Prevention", fh, WHITE)

    d.text((80, 120), "Migration SSRF Prevention", font=fsh, fill=RED)
    card(d, 80, 165, 840, 280, "Attack Vector", [
        "Attacker sends: POST /vms/myvm/migrate",
        '  {"dest_uri": "http://169.254.169.254/metadata"}',
        "",
        "Without validation, daemon connects to",
        "internal AWS/GCP metadata endpoints",
        "or scans internal network ports",
    ], RED)

    card(d, 980, 165, 840, 280, "Protection", [
        "URI scheme allowlist enforced:",
        "  qemu://, qemu+ssh://, qemu+tcp://",
        "  qemu+tls://, qemu+unix://",
        "",
        "Rejects http://, https://, file://,",
        "ftp://, and all non-libvirt schemes",
    ], GREEN)

    d.text((80, 500), "Path Traversal Prevention", font=fsh, fill=ORANGE)
    card(d, 80, 545, 840, 240, "CD-ROM ISO Path", [
        "Must be absolute path",
        "canonicalize() resolves symlinks",
        "Must point to existing regular file",
        "Blocks: /var/../etc/shadow",
        "Blocks: symlinks to sensitive files",
    ], ORANGE)

    card(d, 980, 545, 840, 240, "Console PTY Path", [
        "Must start with /dev/pts/",
        "Prevents socat connecting to",
        "arbitrary files or devices",
        "PTY path from libvirt XML only",
        "Existence check before use",
    ], CYAN)

    d.text((80, 840), "XML Escaping", font=fsh, fill=YELLOW)
    d.text((80, 885), "All user inputs XML-escaped before insertion:  & < > ' \"  ->  &amp; &lt; &gt; &apos; &quot;", font=fb, fill=LIGHT)
    d.text((80, 925), "Prevents XML injection in VM names, disk paths, network configs, and all libvirt XML operations", font=fb, fill=GRAY)
    return img

def slide_memory_safety():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Memory Safety & Panic Prevention", fh, WHITE)

    shields_data = [
        ("Integer overflow: checked_mul()", GREEN),
        ("UTF-8 safe string truncation", GREEN),
        ("Bounds-checked array access", GREEN),
        ("No unsafe blocks in core", GREEN),
        ("let-else instead of unwrap()", ACCENT),
        ("Result/Option propagation", ACCENT),
    ]
    y = 130
    for i, (label, color) in enumerate(shields_data):
        col = i % 2
        x = 80 + col * 500
        shield(d, x, y + (i // 2) * 65, label, color)

    d.text((80, 360), "Panic Prevention Measures", font=fsh, fill=ORANGE)
    measures = [
        ("Storage capacity", "capacity_gb.checked_mul(1024*1024*1024) prevents u64 overflow at 16 EB", GREEN),
        ("Template index", "templates.get(index) with bounds check, not direct indexing", GREEN),
        ("Form fields", "let Some(field) = form.fields.get_mut(i) else { return }", GREEN),
        ("String truncation", "s.chars().take(n).collect() instead of &s[..n] byte slicing", GREEN),
        ("Parse errors", "Report 'Invalid vCPU count' instead of silently defaulting to 0", ORANGE),
        ("Signal handling", "Error logging instead of expect() / unwrap() panics", ORANGE),
        ("Connection recovery", "Close old connection before replacing on reconnect", ACCENT),
        ("Pool refresh", "Log and continue on non-fatal refresh errors", ACCENT),
    ]
    y = 410
    for label, desc, color in measures:
        d.rounded_rectangle([80, y, 1840, y+44], radius=6, fill=CODE_BG)
        d.text((95, y+10), label, font=fl, fill=color)
        d.text((360, y+12), desc, font=fb, fill=LIGHT)
        y += 52

    d.text((80, 860), "Rust guarantees: no buffer overflows, no use-after-free, no data races, no null pointer dereferences", font=fb, fill=GRAY)
    tc(d, 910, "Zero CVEs  |  Zero unsafe  |  Compiler-enforced safety", fbig, GREEN)
    return img

def slide_audit_logging():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Audit Logging & Observability", fh, WHITE)

    card(d, 80, 120, 840, 300, "Audit Log", [
        "Every operation logged with timestamp",
        "Format: timestamp  action  target  result",
        "Stored at: ~/.machina/audit.log",
        "",
        "Logged operations:",
        "  create, delete, start, stop, shutdown",
        "  reboot, pause, resume, clone, rename",
        "  snapshot, resize, autostart, console",
    ], GREEN)

    card(d, 980, 120, 840, 300, "HTTP Tracing", [
        "tower-http TraceLayer on all requests",
        "Logs: method, path, status, latency",
        "Structured tracing with tracing crate",
        "",
        "View with:",
        "  journalctl -u machina-daemon -f",
        "  RUST_LOG=debug machina-daemon",
    ], ACCENT)

    card(d, 80, 470, 840, 260, "Real-time Monitoring", [
        "WebSocket /ws/v1/watch for live events",
        "State changes: running, shutoff, paused",
        "VM added/removed notifications",
        "Heartbeat every 2 seconds",
        "Web UI auto-refreshes on events",
    ], CYAN)

    card(d, 980, 470, 840, 260, "Metrics Collection", [
        "Per-VM: CPU time, memory %, disk I/O, net I/O",
        "/api/v1/metrics endpoint (JSON)",
        "Dashboard charts with 30-point history",
        "TUI sparkline trends per VM",
        "No external dependencies (Prometheus-ready)",
    ], ORANGE)

    tc(d, 790, "All audit events shown in Web UI Events page and TUI :events view", fb, GRAY)
    return img

def slide_comparison():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Why machina?", fh, WHITE)

    headers = ["Feature", "machina", "virt-manager", "Cockpit", "Proxmox"]
    colors = [GRAY, GREEN, ORANGE, ACCENT, PURPLE]
    rows = [
        ["Language", "Rust + React", "Python + GTK", "Python + JS", "Perl + JS"],
        ["Web UI", "Yes", "No (GUI only)", "Yes", "Yes"],
        ["Terminal TUI", "Yes", "No", "No", "No"],
        ["REST API", "30+ endpoints", "No", "Limited", "Yes"],
        ["Single binary", "Yes (<10 MB)", "No (many deps)", "No (many deps)", "No (OS distro)"],
        ["Memory safety", "Rust (compile)", "Python (runtime)", "Python (runtime)", "Perl (runtime)"],
        ["Real-time events", "WebSocket", "No", "Limited", "No"],
        ["VNC in browser", "Yes (noVNC)", "No", "Yes", "Yes"],
        ["Serial console", "Yes (WebSocket)", "No", "Yes", "Yes"],
        ["Input validation", "Comprehensive", "Basic", "Basic", "Moderate"],
        ["Audit logging", "Built-in", "No", "Limited", "Yes"],
        ["Setup time", "5 minutes", "Package install", "Package install", "Full OS install"],
        ["Footprint", "< 10 MB", "~200 MB", "~500 MB", "Full OS"],
    ]

    # Header
    y = 120
    col_widths = [260, 280, 280, 280, 280]
    x = 80
    d.rounded_rectangle([x, y, x+sum(col_widths)+80, y+38], radius=6, fill=ACCENT)
    cx = x + 10
    for i, h in enumerate(headers):
        d.text((cx, y+8), h, font=fl, fill=WHITE)
        cx += col_widths[i]
    y += 44

    for ri, row in enumerate(rows):
        bg = CODE_BG if ri % 2 == 0 else BG
        d.rounded_rectangle([x, y, x+sum(col_widths)+80, y+38], radius=4, fill=bg)
        cx = x + 10
        for ci, cell in enumerate(row):
            color = GREEN if ci == 1 and ("Yes" in cell or "Rust" in cell or "30+" in cell or "5 min" in cell or "< 10" in cell or "Comprehensive" in cell) else LIGHT
            d.text((cx, y+8), cell, font=fs, fill=color)
            cx += col_widths[ci]
        y += 42

    tc(d, 720, "machina: Purpose-built, lightweight, secure VM management", fbig, GREEN)
    tc(d, 770, "No bloat | No full OS required | Production-ready in minutes", fb, GRAY)
    return img

def slide_summary():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d, 6)
    tc(d, 120, "Security Summary", fh, WHITE)
    div(d, 190)

    checks = [
        "Rust memory safety - no buffer overflows, use-after-free, or data races",
        "Input validation on all API endpoints with descriptive error messages",
        "Migration URI allowlist prevents SSRF attacks",
        "Path canonicalization prevents directory traversal",
        "PTY path validation prevents command injection via socat",
        "XML escaping on all user-supplied values",
        "No CORS headers - same-origin policy enforced",
        "Integer overflow protection with checked arithmetic",
        "Panic-free design - no unwrap() on user input",
        "Audit logging for all operations with timestamps",
        "Graceful error handling with structured error types",
        "Connection auto-recovery with explicit cleanup",
    ]
    y = 230
    for item in checks:
        shield(d, (W - 700) // 2, y, item[:48], GREEN)
        y += 58

    tc(d, 950, "machina  --  Secure by Design  |  Safe by Default", fsh, ACCENT)
    return img

slides = [
    slide_title(),
    slide_architecture(),
    slide_input_validation(),
    slide_ssrf_injection(),
    slide_memory_safety(),
    slide_audit_logging(),
    slide_comparison(),
    slide_summary(),
]

slides[0].save(OUT, save_all=True, append_images=slides[1:], resolution=150)
print(f"Generated {OUT} ({len(slides)} slides, {os.path.getsize(OUT) // 1024} KB)")
