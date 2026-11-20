#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Generate a complete API Reference PDF for machina."""

from PIL import Image, ImageDraw, ImageFont
import os

DIR = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(DIR, "machina-api-reference.pdf")
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

BG = (13, 17, 23); CODE_BG = (22, 27, 34); ACCENT = (59, 130, 246)
WHITE = (255, 255, 255); GRAY = (148, 163, 184); LIGHT = (203, 213, 225)
GREEN = (34, 197, 94); ORANGE = (249, 115, 22); RED = (239, 68, 68)
CYAN = (34, 211, 238); YELLOW = (250, 204, 21); PURPLE = (168, 85, 247)

METHOD_COLORS = {"GET": GREEN, "POST": ACCENT, "DELETE": RED, "PUT": ORANGE, "PATCH": YELLOW}

def tc(draw, y, text, font, fill=WHITE):
    bb = draw.textbbox((0,0), text, font=font)
    draw.text(((W - bb[2] + bb[0]) // 2, y), text, font=font, fill=fill)

def bar(draw, n=4):
    for i in range(n): draw.rectangle([0, i, W, i+1], fill=ACCENT)

def div(draw, y):
    draw.line([(W//2-80, y), (W//2+80, y)], fill=ACCENT, width=3)

def code_block(draw, x, y, w, lines):
    pad, lh = 16, 30
    h = pad*2 + len(lines)*lh + 8
    draw.rounded_rectangle([x, y, x+w, y+h], radius=10, fill=CODE_BG, outline=(50,60,75))
    for i, c in enumerate([(255,95,86),(255,189,46),(39,201,63)]):
        draw.ellipse([x+12+i*18, y+10, x+22+i*18, y+20], fill=c)
    cy = y + pad + 16
    for line in lines:
        if line.startswith("#"):
            draw.text((x+pad, cy), line, font=fcl, fill=GRAY)
        elif line.startswith("$"):
            draw.text((x+pad, cy), "$ ", font=fcl, fill=GREEN)
            draw.text((x+pad+28, cy), line[2:], font=fcl, fill=LIGHT)
        elif line.startswith(">"):
            draw.text((x+pad, cy), line[1:].strip(), font=fcl, fill=CYAN)
        elif line.startswith("{") or line.startswith("}") or line.startswith('"'):
            draw.text((x+pad+10, cy), line, font=fcl, fill=ORANGE)
        else:
            draw.text((x+pad, cy), line, font=fcl, fill=LIGHT)
        cy += lh
    return h

def method_pill(draw, x, y, method):
    color = METHOD_COLORS.get(method, GRAY)
    bb = draw.textbbox((0,0), method, font=fl)
    tw = bb[2]-bb[0]
    draw.rounded_rectangle([x, y, x+tw+16, y+28], radius=6, fill=color)
    draw.text((x+8, y+2), method, font=fl, fill=WHITE)
    return tw + 24

def endpoint_table(draw, y, title, color, endpoints):
    draw.text((80, y), title, font=fsh, fill=color)
    y += 45
    # Header
    draw.rounded_rectangle([80, y, 1840, y+36], radius=6, fill=ACCENT)
    draw.text((95, y+6), "Method", font=fl, fill=WHITE)
    draw.text((220, y+6), "Endpoint", font=fl, fill=WHITE)
    draw.text((780, y+6), "Description", font=fl, fill=WHITE)
    draw.text((1250, y+6), "Request Body", font=fl, fill=WHITE)
    y += 42
    for i, (method, path, desc, body) in enumerate(endpoints):
        bg = CODE_BG if i % 2 == 0 else BG
        draw.rounded_rectangle([80, y, 1840, y+36], radius=4, fill=bg)
        method_pill(draw, 95, y+4, method)
        draw.text((220, y+8), path, font=fcl, fill=LIGHT)
        draw.text((780, y+8), desc, font=fb, fill=GRAY)
        draw.text((1250, y+8), body, font=fcl, fill=ORANGE if body != "-" else GRAY)
        y += 40
    return y

# ── Slides ──────────────────────────────────────────────────────────────

def slide_title():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d, 6)
    tc(d, 200, "machina", ft, WHITE)
    tc(d, 290, "REST API Reference", fh, ACCENT)
    div(d, 370)
    tc(d, 420, "Complete reference for all 40+ API endpoints", fb, GRAY)
    tc(d, 480, "Base URL: http://localhost:5092/api/v1", fc, CYAN)
    tc(d, 560, "All responses are JSON  |  Errors return {\"error\": \"message\"}", fb, LIGHT)
    pills = [("GET", GREEN), ("POST", ACCENT), ("DELETE", RED)]
    x = W//2 - 150
    for m, c in pills:
        d.rounded_rectangle([x, 660, x+80, 695], radius=8, fill=c)
        d.text((x+16, 665), m, font=fl, fill=WHITE)
        x += 100
    tc(d, 750, "Content-Type: application/json", fc, GRAY)
    tc(d, 850, "https://github.com/zyvorai/machina", fs, ACCENT)
    return img

def slide_vm_endpoints():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Virtual Machine Endpoints", fh, WHITE)
    endpoint_table(d, 110, "VM Lifecycle", ACCENT, [
        ("GET", "/vms", "List all VMs", "-"),
        ("GET", "/vms/{name}", "Get VM details", "-"),
        ("GET", "/vms/{name}/xml", "Get VM XML definition", "-"),
        ("POST", "/vms", "Create new VM", "{name, vcpus, memory_mb, disk_gb}"),
        ("POST", "/vms/{name}/start", "Start VM", "-"),
        ("POST", "/vms/{name}/stop", "Force stop VM", "-"),
        ("POST", "/vms/{name}/shutdown", "Graceful shutdown", "-"),
        ("POST", "/vms/{name}/reboot", "Reboot VM", "-"),
        ("POST", "/vms/{name}/pause", "Pause VM", "-"),
        ("POST", "/vms/{name}/resume", "Resume paused VM", "-"),
        ("DELETE", "/vms/{name}", "Delete VM", "-"),
        ("POST", "/vms/{name}/clone", "Clone VM", "{new_name}"),
        ("POST", "/vms/{name}/rename", "Rename VM (must be off)", "{new_name}"),
        ("POST", "/vms/{name}/autostart/{bool}", "Set autostart", "-"),
        ("POST", "/vms/{name}/vcpus/{n}", "Set vCPU count", "-"),
        ("POST", "/vms/{name}/memory/{mb}", "Set memory (MB)", "-"),
    ])
    return img

def slide_vm_examples():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "VM API Examples", fh, WHITE)

    d.text((80, 110), "List VMs", font=fsh, fill=GREEN)
    code_block(d, 80, 155, 860, [
        "$ curl -s http://localhost:5092/api/v1/vms",
        "",
        "> Response:",
        '[{"name":"myvm","state":"running",',
        '  "vcpus":4,"memory_mb":4096}]',
    ])

    d.text((980, 110), "Create VM", font=fsh, fill=ACCENT)
    code_block(d, 980, 155, 860, [
        "$ curl -X POST http://localhost:5092/api/v1/vms \\",
        '  -H "Content-Type: application/json" \\',
        '  -d \'{"name":"web-server",',
        '       "vcpus":4, "memory_mb":8192,',
        '       "disk_gb":40, "network":"default"}\'',
    ])

    d.text((80, 400), "Get VM Details", font=fsh, fill=CYAN)
    code_block(d, 80, 445, 860, [
        "$ curl -s localhost:5092/api/v1/vms/myvm",
        "",
        "> Response includes:",
        "> name, uuid, state, vcpus, memory_mb,",
        "> os_type, arch, autostart, persistent,",
        "> interfaces[], disks[]",
    ])

    d.text((980, 400), "VM Lifecycle", font=fsh, fill=ORANGE)
    code_block(d, 980, 445, 860, [
        "# Start",
        "$ curl -X POST localhost:5092/api/v1/vms/myvm/start",
        "",
        "# Graceful shutdown",
        "$ curl -X POST localhost:5092/api/v1/vms/myvm/shutdown",
        "",
        "# Force stop",
        "$ curl -X POST localhost:5092/api/v1/vms/myvm/stop",
    ])

    d.text((80, 720), "Clone & Resize", font=fsh, fill=PURPLE)
    code_block(d, 80, 760, 860, [
        '$ curl -X POST localhost:5092/api/v1/vms/myvm/clone \\',
        '  -d \'{"new_name":"myvm-clone"}\'',
        '$ curl -X POST localhost:5092/api/v1/vms/myvm/vcpus/8',
    ])

    d.text((980, 720), "Delete VM", font=fsh, fill=RED)
    code_block(d, 980, 760, 860, [
        "$ curl -X DELETE localhost:5092/api/v1/vms/myvm",
        "",
        '> {"status":"deleted","name":"myvm"}',
    ])
    return img

def slide_network_storage():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Network & Storage Endpoints", fh, WHITE)

    endpoint_table(d, 110, "Networks", GREEN, [
        ("GET", "/networks", "List all networks", "-"),
        ("GET", "/networks/{name}/xml", "Get network XML", "-"),
        ("POST", "/networks", "Create network", "{name, subnet, dhcp_start, dhcp_end}"),
        ("POST", "/networks/{name}/start", "Start network", "-"),
        ("POST", "/networks/{name}/stop", "Stop network", "-"),
        ("POST", "/networks/{name}/autostart/{bool}", "Set autostart", "-"),
        ("DELETE", "/networks/{name}", "Delete network", "-"),
    ])

    endpoint_table(d, 470, "Storage", ORANGE, [
        ("GET", "/storage/pools", "List storage pools", "-"),
        ("POST", "/storage/pools", "Create pool", "{name, pool_type, target_path}"),
        ("DELETE", "/storage/pools/{name}", "Delete pool", "-"),
        ("GET", "/storage/pools/{name}/xml", "Get pool XML", "-"),
        ("GET", "/storage/pools/{p}/volumes", "List volumes in pool", "-"),
        ("POST", "/storage/pools/{p}/volumes", "Create volume", "{name, capacity_gb, format}"),
        ("DELETE", "/storage/pools/{p}/volumes/{v}", "Delete volume", "-"),
        ("POST", "/storage/pools/{p}/volumes/{v}/resize", "Resize volume", "{capacity_gb}"),
        ("POST", "/storage/pools/{p}/volumes/{v}/clone", "Clone volume", "{new_name}"),
    ])
    return img

def slide_snapshots_metrics():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Snapshots, Metrics & Node Endpoints", fh, WHITE)

    endpoint_table(d, 110, "Snapshots", PURPLE, [
        ("GET", "/snapshots", "List all snapshots", "-"),
        ("GET", "/vms/{name}/snapshots", "List snapshots for VM", "-"),
        ("POST", "/vms/{name}/snapshots", "Create snapshot", "{name, description}"),
        ("POST", "/vms/{n}/snapshots/{s}/revert", "Revert to snapshot", "-"),
        ("DELETE", "/vms/{n}/snapshots/{s}", "Delete snapshot", "-"),
    ])

    endpoint_table(d, 380, "Metrics & Node", CYAN, [
        ("GET", "/metrics", "All VM metrics", "-"),
        ("GET", "/metrics/{name}", "Single VM metrics", "-"),
        ("GET", "/node", "Host node info", "-"),
        ("GET", "/health", "Health check", "-"),
        ("GET", "/templates", "List VM templates", "-"),
    ])

    endpoint_table(d, 610, "WebSocket", YELLOW, [
        ("WS", "/ws/v1/watch", "Real-time VM state changes", "subscribe"),
        ("WS", "/ws/v1/console/{name}", "Serial console I/O", "bidirectional"),
        ("WS", "/ws/v1/vnc/{name}", "VNC proxy (binary)", "bidirectional"),
    ])

    tc(d, 880, "Metrics response: cpu_time_ns, memory_total_mb, memory_used_mb, memory_pct, disk/net I/O", fb, GRAY)
    return img

def slide_advanced():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Advanced Endpoints", fh, WHITE)

    endpoint_table(d, 110, "Guest Agent & Boot", ACCENT, [
        ("GET", "/vms/{name}/interfaces", "Guest IP addresses (via agent)", "-"),
        ("GET", "/vms/{name}/hostname", "Guest hostname", "-"),
        ("GET", "/vms/{name}/boot", "Get boot configuration", "-"),
        ("POST", "/vms/{name}/boot", "Set boot order", "{devices: [\"hd\",\"cdrom\"]}"),
    ])

    endpoint_table(d, 350, "CD-ROM, Save & Migration", ORANGE, [
        ("POST", "/vms/{name}/cdrom/insert", "Insert CD-ROM ISO", "{iso_path, target}"),
        ("POST", "/vms/{name}/cdrom/eject/{target}", "Eject CD-ROM", "-"),
        ("POST", "/vms/{name}/managed-save", "Managed save (hibernate)", "-"),
        ("DELETE", "/vms/{name}/managed-save", "Remove saved state", "-"),
        ("GET", "/vms/{name}/managed-save/status", "Check if save exists", "-"),
        ("POST", "/vms/{name}/migrate", "Migrate VM", "{dest_uri, live}"),
        ("POST", "/vms/{name}/balloon/{mb}", "Memory balloon", "-"),
    ])

    endpoint_table(d, 680, "Infrastructure", GREEN, [
        ("GET", "/capabilities", "Hypervisor capabilities", "-"),
        ("GET", "/sysinfo", "System BIOS/SMBIOS info (XML)", "-"),
        ("GET", "/devices", "List node devices", "?capability=pci|net|usb"),
        ("GET", "/devices/{name}", "Get device XML", "-"),
        ("GET", "/nwfilters", "List network filters", "-"),
        ("DELETE", "/nwfilters/{name}", "Delete network filter", "-"),
        ("GET", "/secrets", "List secrets", "-"),
        ("DELETE", "/secrets/{uuid}", "Delete secret", "-"),
    ])
    return img

def slide_response_formats():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Response Formats", fh, WHITE)

    d.text((80, 110), "VM Details Response", font=fsh, fill=ACCENT)
    code_block(d, 80, 160, 860, [
        '{',
        '  "name": "web-server",',
        '  "uuid": "a1b2c3d4-...",',
        '  "state": "running",',
        '  "vcpus": 4,',
        '  "memory_mb": 8192,',
        '  "os_type": "hvm",',
        '  "arch": "x86_64",',
        '  "autostart": true,',
        '  "persistent": true,',
        '  "interfaces": [',
        '    {"mac_address":"52:54:00:...",',
        '     "source":"default","model":"virtio"}',
        '  ],',
        '  "disks": [',
        '    {"device":"disk","source":"/var/...",',
        '     "driver":"qcow2","target":"vda"}',
        '  ]',
        '}',
    ])

    d.text((980, 110), "Metrics Response", font=fsh, fill=GREEN)
    code_block(d, 980, 160, 860, [
        '{',
        '  "name": "web-server",',
        '  "cpu_time_ns": 58230000000,',
        '  "vcpus": 4,',
        '  "memory_total_mb": 8192,',
        '  "memory_used_mb": 3421,',
        '  "memory_pct": 41.8,',
        '  "disk_rd_bytes": 1048576,',
        '  "disk_wr_bytes": 524288,',
        '  "net_rx_bytes": 204800,',
        '  "net_tx_bytes": 102400',
        '}',
    ])

    d.text((980, 560), "Error Response", font=fsh, fill=RED)
    code_block(d, 980, 610, 860, [
        '# 404 Not Found',
        '{"error": "VM \'myvm\' not found: ..."}',
        '',
        '# 500 Internal Server Error',
        '{"error": "Failed to start VM: ..."}',
        '',
        '# Validation Error',
        '{"error": "capacity_gb must be > 0"}',
    ])

    d.text((80, 710), "Node Info Response", font=fsh, fill=CYAN)
    code_block(d, 80, 755, 860, [
        '{"hostname":"server1",',
        ' "hypervisor":"QEMU",',
        ' "hypervisor_version":"10.1.4",',
        ' "lib_version":"11.6.0",',
        ' "cpu_model":"x86_64",',
        ' "cpu_cores":14,"memory_mb":31524}',
    ])
    return img

def slide_websocket():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "WebSocket API", fh, WHITE)

    d.text((80, 120), "VM State Watch  ws://host:5092/ws/v1/watch", font=fsh, fill=ACCENT)
    code_block(d, 80, 170, 860, [
        "# Heartbeat (every 2s)",
        '{"event":"heartbeat","vm_count":5}',
        "",
        "# State change",
        '{"event":"changes","changes":[',
        '  {"event":"state_change",',
        '   "name":"myvm",',
        '   "old_state":"shutoff",',
        '   "new_state":"running"}',
        ']}',
        "",
        "# VM added/removed",
        '{"event":"changes","changes":[',
        '  {"event":"vm_added","name":"new-vm",',
        '   "state":"running"}',
        ']}',
    ])

    d.text((980, 120), "Serial Console  ws://host:5092/ws/v1/console/{name}", font=fsh, fill=GREEN)
    code_block(d, 980, 170, 860, [
        "# Bidirectional text I/O",
        "# Send: keystrokes as text frames",
        "# Receive: terminal output as text frames",
        "",
        "# Connection example (JavaScript):",
        'const ws = new WebSocket(',
        '  "ws://localhost:5092/ws/v1/console/myvm"',
        ');',
        'ws.onmessage = (e) => term.write(e.data);',
        'term.onData((d) => ws.send(d));',
    ])

    d.text((80, 680), "VNC Proxy  ws://host:5092/ws/v1/vnc/{name}", font=fsh, fill=ORANGE)
    code_block(d, 80, 730, 1760, [
        "# Binary WebSocket proxy to VM's VNC port. Used by noVNC in the Web UI.",
        "# Send/receive: binary VNC protocol frames. Connects to qemu VNC on 127.0.0.1:{port}",
    ])

    tc(d, 880, "All WebSocket connections auto-close when the client disconnects or the VM stops", fb, GRAY)
    return img

def slide_auth_errors():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Status Codes & Error Handling", fh, WHITE)

    codes = [
        ("200", "OK", "Successful GET/POST/DELETE", GREEN),
        ("404", "Not Found", "VM, network, pool, volume, or snapshot not found", ORANGE),
        ("500", "Internal Server Error", "Libvirt operation failed, connection error", RED),
    ]
    y = 130
    for code, name, desc, color in codes:
        draw = d
        draw.rounded_rectangle([80, y, 1840, y+60], radius=10, fill=CODE_BG)
        draw.rounded_rectangle([95, y+12, 165, y+48], radius=6, fill=color)
        draw.text((105, y+15), code, font=fl, fill=WHITE)
        draw.text((185, y+18), name, font=fsh, fill=WHITE)
        draw.text((500, y+20), desc, font=fb, fill=GRAY)
        y += 72

    d.text((80, 380), "Input Validation", font=fsh, fill=YELLOW)
    items = [
        "VM/resource names: alphanumeric, dash, underscore, dot (1-64 chars)",
        "vCPUs: 1-256",
        "Memory: 64 MB - 1 TB (1,048,576 MB)",
        "Disk size: 1 GB - 10 TB (10,240 GB)",
        "Migration URI: must start with qemu://, qemu+ssh://, qemu+tcp://, qemu+tls://, qemu+unix://",
        "Volume capacity: must be > 0, checked_mul for overflow protection",
        "ISO paths: must be absolute, canonicalized, and point to existing file",
        "Network IPs: validated as IPv4 format, subnet as 3-octet prefix",
        "Memory balloon: validated against 64 MB - 1 TB range",
    ]
    y = 430
    for item in items:
        d.text((100, y), "  " + item, font=fb, fill=LIGHT)
        y += 38

    d.text((80, 810), "Security:", font=fsh, fill=RED)
    d.text((240, 815), "No CORS headers (same-origin only)  |  XML-escaped inputs  |  PTY path validated  |  Audit logged", font=fb, fill=GRAY)
    return img

def slide_automation():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Automation Examples", fh, WHITE)

    d.text((80, 110), "Bash: Bulk create VMs", font=fsh, fill=GREEN)
    code_block(d, 80, 155, 860, [
        '#!/bin/bash',
        'API="http://localhost:5092/api/v1"',
        'for i in $(seq 1 5); do',
        '  curl -s -X POST "$API/vms" \\',
        '    -H "Content-Type: application/json" \\',
        '    -d "{',
        '      \\"name\\": \\"worker-$i\\",',
        '      \\"vcpus\\": 2,',
        '      \\"memory_mb\\": 4096,',
        '      \\"disk_gb\\": 20',
        '    }"',
        '  curl -s -X POST "$API/vms/worker-$i/start"',
        '  echo "Created and started worker-$i"',
        'done',
    ])

    d.text((980, 110), "Python: Monitor VMs", font=fsh, fill=ACCENT)
    code_block(d, 980, 155, 860, [
        'import requests, time',
        '',
        'API = "http://localhost:5092/api/v1"',
        '',
        'while True:',
        '    vms = requests.get(f"{API}/vms").json()',
        '    metrics = requests.get(f"{API}/metrics").json()',
        '    for m in metrics:',
        '        if m["memory_pct"] > 90:',
        '            print(f"ALERT: {m[\\"name\\"]} at',
        '                   {m[\\"memory_pct\\"]}% memory")',
        '    time.sleep(30)',
    ])

    d.text((80, 680), "Snapshot backup script", font=fsh, fill=ORANGE)
    code_block(d, 80, 725, 860, [
        'API="http://localhost:5092/api/v1"',
        'DATE=$(date +%Y%m%d)',
        'for vm in $(curl -s $API/vms | jq -r ".[].name"); do',
        '  curl -s -X POST "$API/vms/$vm/snapshots" \\',
        '    -d "{\\"name\\":\\"backup-$DATE\\"}"',
        'done',
    ])

    d.text((980, 680), "Health check (Nagios/Zabbix)", font=fsh, fill=CYAN)
    code_block(d, 980, 725, 860, [
        '#!/bin/bash',
        'HEALTH=$(curl -sf localhost:5092/api/v1/health)',
        'if [ $? -ne 0 ]; then',
        '  echo "CRITICAL: machina daemon down"',
        '  exit 2',
        'fi',
        'echo "OK: $HEALTH"',
    ])
    return img

def slide_backup_endpoints():
    img = Image.new("RGB", (W, H), BG)
    d = ImageDraw.Draw(img)
    bar(d)
    tc(d, 40, "Backup & Restore Endpoints", fh, WHITE)

    endpoint_table(d, 110, "Backup Operations", ACCENT, [
        ("GET", "/backups", "List all backups with status/size", "-"),
        ("POST", "/backups", "Trigger backup", "{vm_name, with_disks, incremental, retain}"),
        ("GET", "/backups/{id}/status", "Get backup progress & status", "-"),
        ("POST", "/backups/{id}/verify", "Verify SHA-256 checksums", "-"),
        ("GET", "/backups/{id}/download", "Download backup as tar.gz", "-"),
        ("POST", "/backups/restore", "Restore from backup", "{backup_id}"),
        ("DELETE", "/backups/{id}", "Delete a backup", "-"),
    ])

    endpoint_table(d, 480, "Schedule Management", GREEN, [
        ("GET", "/backups/schedule", "Get systemd timer status", "-"),
        ("POST", "/backups/schedule", "Enable/disable timer", "{enabled: true|false}"),
    ])

    d.text((80, 650), "Backup Examples", font=fsh, fill=CYAN)
    code_block(d, 80, 700, 860, [
        "# Backup all VMs",
        '$ curl -X POST localhost:5092/api/v1/backups \\',
        '  -d \'{"retain": 7}\'',
        "",
        "# Per-VM with disks",
        '$ curl -X POST localhost:5092/api/v1/backups \\',
        '  -d \'{"vm_name":"myvm","with_disks":true}\'',
    ])

    code_block(d, 980, 700, 860, [
        "# Verify checksums",
        '$ curl -X POST localhost:5092/api/v1/backups/20260324-020000/verify',
        "",
        "# Download as tar.gz",
        '$ curl -O localhost:5092/api/v1/backups/20260324-020000/download',
        "",
        "# Enable scheduled backups",
        '$ curl -X POST localhost:5092/api/v1/backups/schedule \\',
        '  -d \'{"enabled":true}\'',
    ])
    return img

slides = [
    slide_title(),
    slide_vm_endpoints(),
    slide_vm_examples(),
    slide_network_storage(),
    slide_snapshots_metrics(),
    slide_advanced(),
    slide_backup_endpoints(),
    slide_response_formats(),
    slide_websocket(),
    slide_auth_errors(),
    slide_automation(),
]

slides[0].save(OUT, save_all=True, append_images=slides[1:], resolution=150)
print(f"Generated {OUT} ({len(slides)} slides, {os.path.getsize(OUT) // 1024} KB)")
