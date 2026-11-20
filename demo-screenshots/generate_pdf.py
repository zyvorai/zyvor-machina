#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Generate a polished demo PDF for machina."""

from PIL import Image, ImageDraw, ImageFont
import os, sys

DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(DIR, "machina-demo.pdf")
W, H = 1920, 1080

# Try to find a decent font
FONT_PATHS = [
    "/usr/share/fonts/google-noto-sans-fonts/NotoSans-Bold.ttf",
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation-sans/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
]
FONT_PATHS_REGULAR = [p.replace("-Bold", "-Regular").replace("Bold", "Regular") for p in FONT_PATHS]

def find_font(paths, size):
    for p in paths:
        if os.path.exists(p):
            return ImageFont.truetype(p, size)
    return ImageFont.load_default()

font_title = find_font(FONT_PATHS, 72)
font_subtitle = find_font(FONT_PATHS, 36)
font_heading = find_font(FONT_PATHS, 48)
font_body = find_font(FONT_PATHS_REGULAR, 28)
font_small = find_font(FONT_PATHS_REGULAR, 22)

BG = (13, 17, 23)        # Dark slate matching the UI
ACCENT = (59, 130, 246)   # Blue accent
WHITE = (255, 255, 255)
GRAY = (148, 163, 184)
LIGHT = (203, 213, 225)
GREEN = (34, 197, 94)
ORANGE = (249, 115, 22)

def text_center(draw, y, text, font, fill=WHITE):
    bbox = draw.textbbox((0, 0), text, font=font)
    tw = bbox[2] - bbox[0]
    draw.text(((W - tw) // 2, y), text, font=font, fill=fill)

def draw_pill(draw, x, y, text, font, bg, fg=WHITE):
    bbox = draw.textbbox((0, 0), text, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    pad = 12
    draw.rounded_rectangle([x, y, x + tw + pad*2, y + th + pad*2], radius=8, fill=bg)
    draw.text((x + pad, y + pad - 2), text, font=font, fill=fg)

def make_title_slide():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    # Gradient accent bar at top
    for i in range(6):
        draw.rectangle([0, i, W, i+1], fill=ACCENT)
    # Logo area
    text_center(draw, 280, "machina", font_title, WHITE)
    text_center(draw, 380, "Linux Hypervisor Host Manager", font_subtitle, GRAY)
    # Feature pills
    features = [
        ("Web UI", ACCENT), ("REST API", GREEN), ("Terminal TUI", ORANGE),
        ("Real-time Metrics", ACCENT), ("Optional KubeVirt workflows", GREEN),
    ]
    total_w = sum(draw.textbbox((0,0), f[0], font=font_body)[2] - draw.textbbox((0,0), f[0], font=font_body)[0] + 40 for f in features) + 20 * (len(features)-1)
    x = (W - total_w) // 2
    for text, color in features:
        bbox = draw.textbbox((0,0), text, font=font_body)
        tw = bbox[2] - bbox[0]
        draw_pill(draw, x, 500, text, font_body, color)
        x += tw + 40 + 20
    # Bottom info
    text_center(draw, 700, "Built with Rust + React + TypeScript", font_body, GRAY)
    text_center(draw, 760, "Secure | Fast | Production-Ready", font_body, LIGHT)
    text_center(draw, 860, "https://github.com/zyvorai/machina", font_small, ACCENT)
    return img

def make_section_slide(title, bullets):
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    for i in range(4):
        draw.rectangle([0, i, W, i+1], fill=ACCENT)
    text_center(draw, 120, title, font_heading, WHITE)
    draw.line([(W//2 - 100, 190), (W//2 + 100, 190)], fill=ACCENT, width=3)
    y = 260
    for bullet in bullets:
        bbox = draw.textbbox((0, 0), bullet, font=font_body)
        tw = bbox[2] - bbox[0]
        draw.text(((W - tw) // 2, y), bullet, font=font_body, fill=LIGHT)
        y += 55
    return img

def make_screenshot_slide(screenshot_path, title, description):
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    # Title bar
    draw.rectangle([0, 0, W, 60], fill=(20, 27, 36))
    for i in range(3):
        draw.rectangle([0, i, W, i+1], fill=ACCENT)
    draw.text((30, 15), title, font=font_subtitle, fill=WHITE)
    bbox = draw.textbbox((0, 0), description, font=font_small)
    dw = bbox[2] - bbox[0]
    draw.text((W - dw - 30, 22), description, font=font_small, fill=GRAY)
    # Screenshot
    try:
        ss = Image.open(screenshot_path)
        # Scale to fit within the remaining space (1920 x 1000) with padding
        max_w, max_h = W - 80, H - 100
        ratio = min(max_w / ss.width, max_h / ss.height)
        new_w, new_h = int(ss.width * ratio), int(ss.height * ratio)
        ss = ss.resize((new_w, new_h), Image.LANCZOS)
        x = (W - new_w) // 2
        y = 70 + (max_h - new_h) // 2
        # Drop shadow
        draw.rectangle([x+4, y+4, x+new_w+4, y+new_h+4], fill=(0, 0, 0))
        img.paste(ss, (x, y))
        # Border
        draw.rectangle([x-1, y-1, x+new_w, y+new_h], outline=(50, 60, 75), width=1)
    except Exception as e:
        text_center(draw, 400, f"Screenshot not found: {e}", font_body, ORANGE)
    return img

def make_architecture_slide():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    for i in range(4):
        draw.rectangle([0, i, W, i+1], fill=ACCENT)
    text_center(draw, 60, "Architecture", font_heading, WHITE)

    # Three columns
    cols = [
        ("Web UI", ["React 18 + TypeScript", "Tailwind CSS", "Recharts dashboards", "Real-time WebSocket", "VNC/Serial console"], ACCENT),
        ("REST API", ["Axum (Rust)", "30+ endpoints", "JSON responses", "Input validation", "SSRF protection"], GREEN),
        ("Core Engine", ["libvirt bindings", "Connection pooling", "XML parsing", "Audit logging", "Config management"], ORANGE),
    ]

    col_w = 480
    gap = (W - col_w * 3) // 4
    for i, (title, items, color) in enumerate(cols):
        x = gap + i * (col_w + gap)
        y = 200
        draw.rounded_rectangle([x, y, x+col_w, y+500], radius=16, outline=color, width=2)
        draw.rounded_rectangle([x, y, x+col_w, y+60], radius=16, fill=color)
        draw.rectangle([x, y+40, x+col_w, y+60], fill=color)
        bbox = draw.textbbox((0,0), title, font=font_subtitle)
        tw = bbox[2] - bbox[0]
        draw.text((x + (col_w - tw)//2, y + 12), title, font=font_subtitle, fill=WHITE)
        for j, item in enumerate(items):
            draw.text((x + 30, y + 90 + j * 50), f"  {item}", font=font_body, fill=LIGHT)

    # Bottom
    text_center(draw, 780, "Single binary deployment  |  systemd service  |  < 10MB footprint", font_body, GRAY)
    return img

def make_closing_slide():
    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img)
    for i in range(6):
        draw.rectangle([0, i, W, i+1], fill=ACCENT)
    text_center(draw, 300, "machina", font_title, WHITE)
    text_center(draw, 400, "Ready for Production", font_subtitle, GREEN)
    text_center(draw, 520, "make && sudo make deploy", font_body, LIGHT)
    text_center(draw, 600, "Web UI at http://localhost:5092", font_body, ACCENT)
    text_center(draw, 680, "TUI: machina", font_body, ACCENT)
    text_center(draw, 800, "Questions?", font_heading, GRAY)
    return img

# --- Build PDF ---
slides = []

# 1. Title
slides.append(make_title_slide())

# 2. Key features
slides.append(make_section_slide("Key Features", [
    "Full VM lifecycle: create, start, stop, reboot, pause, resume, delete",
    "Live metrics with CPU/memory charts and WebSocket updates",
    "Storage pool and volume management (create, resize, clone, delete)",
    "Network management with DHCP configuration",
    "Snapshot create, revert, and delete",
    "VM cloning with automatic MAC randomization",
    "CD-ROM insert/eject, boot order configuration",
    "VM migration with URI scheme validation",
    "Guest agent integration (IPs, hostname)",
    "Node device inventory and network filter management",
]))

# 3. Architecture
slides.append(make_architecture_slide())

# 4-13. Screenshots
screenshots = [
    ("01-dashboard.png", "Dashboard", "Real-time overview of your virtualization infrastructure"),
    ("02-vm-list.png", "Virtual Machines", "List, search, and manage all VMs with one-click actions"),
    ("03-vm-details.png", "VM Details", "Deep dive into VM configuration, boot settings, and metrics"),
    ("06-create-vm.png", "Create VM", "Template-based VM creation with customizable resources"),
    ("05-storage.png", "Storage Pools", "Visual storage management with capacity monitoring"),
    ("04-networks.png", "Networks", "Virtual network management with autostart control"),
    ("07-capabilities.png", "Capabilities", "Host and guest architecture discovery"),
    ("08-devices.png", "Node Devices", "Complete hardware inventory with PCI, USB, and network devices"),
    ("09-nwfilters.png", "Network Filters", "Built-in security rules for network traffic control"),
    ("10-console.png", "Console Access", "VNC and serial console access directly from the browser"),
]

for fname, title, desc in screenshots:
    path = os.path.join(DIR, fname)
    slides.append(make_screenshot_slide(path, title, desc))

# 14. Security
slides.append(make_section_slide("Security Hardened", [
    "Migration URI scheme validation (prevents SSRF)",
    "ISO path canonicalization (prevents path traversal)",
    "PTY path validation (socat injection prevention)",
    "Input validation on all API endpoints",
    "Integer overflow protection on storage operations",
    "No CORS headers (same-origin only)",
    "XML escaping on all user inputs",
    "Audit logging for all operations",
]))

# 15. Closing
slides.append(make_closing_slide())

# Save as PDF
slides[0].save(OUT, save_all=True, append_images=slides[1:], resolution=150)
print(f"Generated {OUT} ({len(slides)} slides, {os.path.getsize(OUT) // 1024} KB)")
