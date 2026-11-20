// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0
// Generates the README/site deploy animations as self-contained SVGs (CSS keyframes only, no script, so GitHub renders
// them in <img>). Each scene is a terminal on the left and a diagram on the right; every element fades in at its time
// and the whole scene loops. Run: node gen.mjs   (build.sh also renders GIF fallbacks)
import { writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const W = 1600, H = 720
const C = { blue: '#2997ff', green: '#30d158', amber: '#ff9f0a', teal: '#40c8e0', violet: '#a78bfa', red: '#ff453a', ink: '#f5f5f7', soft: '#a1a1a6' }
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

const scenes = {
  'deploy-single-host': {
    title: 'One host. One command.', sub: 'machinactl deploy builds, installs, starts and verifies',
    dur: 14,
    lines: [
      { t: 0.5, text: '$ git clone \\', c: C.ink },
      { t: 0.7, text: '    https://github.com/zyvorai/zyvor-machina.git machina', c: C.ink },
      { t: 1.5, text: '$ cd machina && ./machinactl deploy', c: C.ink },
      { t: 3, text: 'deps · build · install · start · verify', c: C.soft },
      { t: 5, text: '✓ machina-daemon      :5092', c: C.green },
      { t: 6, text: '✓ machina-controller  :5093', c: C.green },
      { t: 7, text: '✓ machina-agent       :50051', c: C.green },
      { t: 8, text: '✓ machina-bpfd        unix socket', c: C.green },
      { t: 10, text: '# open https://<host>:5092', c: C.blue },
    ],
    nodes: [
      { id: 'hw', x: 700, y: 520, w: 820, h: 90, label: 'Linux host · libvirt · QEMU/KVM', sub: 'your bare metal', c: C.blue, t: 3 },
      { id: 'd', x: 700, y: 250, w: 380, h: 100, label: 'machina-daemon', sub: 'API · auth · consoles  :5092', c: C.blue, t: 5 },
      { id: 'c', x: 1120, y: 250, w: 400, h: 100, label: 'machina-controller', sub: 'fleet · cloud · AI  :5093', c: C.amber, t: 6 },
      { id: 'a', x: 700, y: 390, w: 380, h: 100, label: 'machina-agent', sub: 'gRPC  :50051', c: C.green, t: 7 },
      { id: 'b', x: 1120, y: 390, w: 400, h: 100, label: 'machina-bpfd', sub: 'eBPF datapath', c: C.teal, t: 8 },
      { id: 'u', x: 700, y: 130, w: 820, h: 80, label: 'Browser: Mission Control', sub: 'sign in with a local (PAM) account', c: C.violet, t: 10 },
    ],
  },
  'deploy-fleet': {
    title: 'Then a fleet.', sub: 'Agents on every hypervisor; the controller keeps them healthy',
    dur: 15,
    lines: [
      { t: 0.5, text: '$ ./scripts/deploy-remote.sh admin@kvm-1 --platform', c: C.ink },
      { t: 2, text: '$ ./scripts/deploy-remote.sh admin@kvm-2 --platform', c: C.ink },
      { t: 4, text: 'sources rsync\'d, built on the server', c: C.soft },
      { t: 6, text: '✓ kvm-1  agent connected (gRPC · TLS)', c: C.green },
      { t: 7, text: '✓ kvm-2  agent connected (gRPC · TLS)', c: C.green },
      { t: 10, text: '! kvm-1 stopped answering', c: C.red },
      { t: 11.5, text: '→ HA restarts web-1 on kvm-2', c: C.amber },
      { t: 13, text: '✓ web-1 running again', c: C.green },
    ],
    nodes: [
      { id: 'c', x: 700, y: 140, w: 820, h: 90, label: 'machina-controller', sub: 'fleet · HA · DRS · Fleet Cloud · state in SQLite or PostgreSQL', c: C.amber, t: 4 },
      { id: 'h1', x: 700, y: 330, w: 390, h: 180, label: 'kvm-1', sub: 'agent + libvirt', c: C.green, t: 6 },
      { id: 'h2', x: 1130, y: 330, w: 390, h: 180, label: 'kvm-2', sub: 'agent + libvirt', c: C.green, t: 7 },
      { id: 'v1', x: 730, y: 430, w: 160, h: 50, label: 'web-1', sub: '', c: C.blue, t: 7.5, off: 10 },
      { id: 'v2', x: 1160, y: 430, w: 160, h: 50, label: 'db-1', sub: '', c: C.blue, t: 7.5 },
      { id: 'v3', x: 1340, y: 430, w: 160, h: 50, label: 'web-1', sub: 'recovered', c: C.amber, t: 12.5 },
      { id: 'x', x: 700, y: 560, w: 390, h: 60, label: 'kvm-1 down', sub: '', c: C.red, t: 10, hide: false },
    ],
  },
  'deploy-postgres': {
    title: 'Outgrow SQLite without a rewrite.', sub: 'One command moves the controller to PostgreSQL',
    dur: 15,
    lines: [
      { t: 0.5, text: '$ machinactl db status', c: C.ink },
      { t: 1.5, text: 'backend: sqlite · migrations applied', c: C.soft },
      { t: 3.5, text: '$ machinactl db setup pod --migrate', c: C.ink },
      { t: 5, text: 'PostgreSQL 16 in a Podman pod · 127.0.0.1', c: C.soft },
      { t: 6.5, text: 'copying SQLite rows → PostgreSQL', c: C.soft },
      { t: 8, text: '✓ controller restarted on PostgreSQL', c: C.green },
      { t: 10, text: '# second controller, same database', c: C.blue },
      { t: 11.5, text: '✓ one leader, one standby', c: C.green },
    ],
    nodes: [
      { id: 's', x: 700, y: 180, w: 340, h: 120, label: 'SQLite', sub: 'controller.db · default', c: C.green, t: 0.5 },
      { id: 'p', x: 1180, y: 180, w: 340, h: 120, label: 'PostgreSQL 16', sub: 'managed pod · daily dumps', c: C.blue, t: 5 },
      { id: 'c1', x: 1180, y: 400, w: 340, h: 90, label: 'controller A', sub: 'leader', c: C.amber, t: 8 },
      { id: 'c2', x: 1180, y: 520, w: 340, h: 90, label: 'controller B', sub: 'standby, takes over', c: C.amber, t: 11.5 },
      { id: 'm', x: 700, y: 400, w: 340, h: 90, label: '--migrate', sub: 'rows copied, counts checked', c: C.violet, t: 6.5 },
    ],
  },
  'ec2-launch': {
    title: 'Use the tools you already know.', sub: 'The EC2-compatible endpoint answers awscli, boto3 and Terraform',
    dur: 14,
    lines: [
      { t: 0.5, text: '$ export AWS_ACCESS_KEY_ID=MCAK…', c: C.soft },
      { t: 0.8, text: '$ export AWS_SECRET_ACCESS_KEY=…', c: C.soft },
      { t: 1.5, text: '$ EC2=http://127.0.0.1:5093/ec2', c: C.soft },
      { t: 3, text: '$ aws --endpoint-url $EC2 ec2 run-instances \\', c: C.ink },
      { t: 3.4, text: '    --image-id <id> --count 3', c: C.ink },
      { t: 6, text: 'i-… pending  i-… pending  i-… pending', c: C.soft },
      { t: 8, text: '$ aws --endpoint-url $EC2 ec2 describe-instances', c: C.ink },
      { t: 10, text: '3 × running', c: C.green },
    ],
    nodes: [
      { id: 'cli', x: 700, y: 140, w: 240, h: 90, label: 'awscli', sub: 'boto3 · Terraform', c: C.violet, t: 3 },
      { id: 'ep', x: 1000, y: 140, w: 520, h: 90, label: 'controller :5093 /ec2', sub: 'SigV4 · roles · project scope', c: C.amber, t: 4.5 },
      { id: 'h', x: 700, y: 330, w: 820, h: 90, label: 'machina-agent on your hosts', sub: 'libvirt / KVM', c: C.green, t: 5.5 },
      { id: 'i1', x: 700, y: 470, w: 250, h: 110, label: 'instance 1', sub: 'running', c: C.blue, t: 8 },
      { id: 'i2', x: 975, y: 470, w: 250, h: 110, label: 'instance 2', sub: 'running', c: C.blue, t: 8.6 },
      { id: 'i3', x: 1250, y: 470, w: 270, h: 110, label: 'instance 3', sub: 'running', c: C.blue, t: 9.2 },
    ],
  },
  'openstack-vs-machina': {
    title: 'Nine services collapse into four.', sub: 'Same private-cloud primitives, a fraction of the moving parts',
    dur: 14,
    lines: [
      { t: 0.5, text: 'OpenStack, typical IaaS', c: C.red },
      { t: 1, text: 'Keystone · Nova · Neutron · Glance · Cinder', c: C.soft },
      { t: 1.5, text: 'Placement · Horizon · Heat · Octavia', c: C.soft },
      { t: 2, text: '+ MariaDB/Galera · RabbitMQ · Memcached', c: C.soft },
      { t: 6, text: 'Machina', c: C.blue },
      { t: 6.5, text: 'daemon · controller · agent · bpfd', c: C.ink },
      { t: 7.5, text: '+ SQLite or PostgreSQL · NATS optional', c: C.ink },
      { t: 10, text: '$ ./machinactl deploy', c: C.green },
    ],
    nodes: [
      ...['Keystone', 'Nova', 'Neutron', 'Glance', 'Cinder', 'Placement', 'Horizon', 'Heat', 'Octavia'].map((n, i) => ({
        id: 'o' + i, x: 700 + (i % 3) * 270, y: 140 + Math.floor(i / 3) * 90, w: 250, h: 74, label: n, sub: '', c: C.red, t: 0.5 + i * 0.25, off: 5 })),
      ...['MariaDB', 'RabbitMQ', 'Memcached'].map((n, i) => ({ id: 'b' + i, x: 700 + i * 270, y: 420, w: 250, h: 60, label: n, sub: '', c: C.amber, t: 3 + i * 0.25, off: 5 })),
      { id: 'm1', x: 700, y: 200, w: 400, h: 90, label: 'machina-daemon', sub: ':5092', c: C.blue, t: 6.5 },
      { id: 'm2', x: 1120, y: 200, w: 400, h: 90, label: 'machina-controller', sub: ':5093', c: C.amber, t: 7 },
      { id: 'm3', x: 700, y: 310, w: 400, h: 90, label: 'machina-agent', sub: ':50051', c: C.green, t: 7.5 },
      { id: 'm4', x: 1120, y: 310, w: 400, h: 90, label: 'machina-bpfd', sub: 'eBPF', c: C.teal, t: 8 },
      { id: 'm5', x: 700, y: 430, w: 820, h: 70, label: 'SQLite or PostgreSQL', sub: 'in-memory task bus, NATS optional', c: C.violet, t: 8.5 },
    ],
  },
}

function build(name, s) {
  const css = []
  let n = 0
  // element appears at t, stays until `off` (default scene end minus a short fade), then fades out for the loop
  const anim = (t, off) => {
    const id = 'a' + n++
    const p = (x) => ((x / s.dur) * 100).toFixed(2)
    const end = off ?? s.dur - 0.8
    css.push(`@keyframes ${id}{0%,${p(t)}%{opacity:0}${p(t + 0.35)}%,${p(end)}%{opacity:1}${p(end + 0.3)}%,100%{opacity:0}}`)
    css.push(`.${id}{opacity:0;animation:${id} ${s.dur}s linear infinite}`)
    return id
  }
  let out = ''
  s.lines.forEach((l, i) => {
    out += `<text class="${anim(l.t)}" x="64" y="${196 + i * 46}" font-size="18" fill="${l.c}">${esc(l.text)}</text>`
  })
  for (const nd of s.nodes) {
    const cls = anim(nd.t, nd.off)
    const cx = nd.x + nd.w / 2
    out += `<g class="${cls}"><rect x="${nd.x}" y="${nd.y}" width="${nd.w}" height="${nd.h}" rx="14" fill="${nd.c}" fill-opacity=".13" stroke="${nd.c}" stroke-opacity=".7" stroke-width="2"/>`
    if (nd.sub) {
      const ly = nd.h >= 150 ? nd.y + 44 : nd.y + nd.h / 2 - 2
      out += `<text x="${cx}" y="${ly}" text-anchor="middle" class="h" font-size="25" fill="${C.ink}">${esc(nd.label)}</text>`
      out += `<text x="${cx}" y="${ly + 28}" text-anchor="middle" font-size="16" fill="${C.soft}">${esc(nd.sub)}</text></g>`
    } else {
      out += `<text x="${cx}" y="${nd.y + nd.h / 2 + 9}" text-anchor="middle" class="h" font-size="24" fill="${C.ink}">${esc(nd.label)}</text></g>`
    }
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" width="${W}" height="${H}" role="img" aria-label="${esc(s.title)} ${esc(s.sub)}">
<!-- SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0 · generated by docs/social/anim/gen.mjs, do not edit by hand -->
<style>
text{font-family:"SF Mono",Menlo,Consolas,monospace}.h{font-family:"SF Pro Display","Helvetica Neue",Helvetica,Arial,sans-serif;font-weight:700}
.t{font-family:"SF Pro Display","Helvetica Neue",Helvetica,Arial,sans-serif}
${css.join('\n')}
@media (prefers-reduced-motion:reduce){*{animation:none!important;opacity:1!important}}
</style>
<defs><radialGradient id="g1" cx="88%" cy="-10%" r="70%"><stop offset="0" stop-color="#2997ff" stop-opacity=".30"/><stop offset="1" stop-color="#2997ff" stop-opacity="0"/></radialGradient>
<linearGradient id="bar" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2997ff"/><stop offset="1" stop-color="#64d2ff"/></linearGradient></defs>
<rect width="${W}" height="${H}" fill="#000"/><rect width="${W}" height="${H}" fill="url(#g1)"/><rect width="6" height="${H}" fill="url(#bar)"/>
<text x="64" y="64" font-size="17" letter-spacing="3" fill="${C.soft}">ZYVOR · MACHINA</text>
<text x="64" y="118" class="t" font-size="46" font-weight="800" fill="${C.ink}" style="font-family:'SF Pro Display','Helvetica Neue',Arial,sans-serif">${esc(s.title)}</text>
<text x="1536" y="64" text-anchor="end" font-size="17" fill="${C.blue}">${esc(s.sub)}</text>
<rect x="40" y="150" width="610" height="${H - 190}" rx="16" fill="#0b0b0e" stroke="#ffffff" stroke-opacity=".12"/>
<circle cx="66" cy="172" r="6" fill="#ff5f57"/><circle cx="88" cy="172" r="6" fill="#febc2e"/><circle cx="110" cy="172" r="6" fill="#28c840"/>
${out.replace(/y="(\d+)" font-size="18"/g, (m, y) => `y="${+y + 40}" font-size="18"`)}
<text x="64" y="${H - 20}" font-size="16" fill="${C.soft}">github.com/zyvorai/zyvor-machina</text>
</svg>
`
  writeFileSync(join(HERE, name + '.svg'), svg)
  console.log('wrote', name + '.svg', (svg.length / 1024).toFixed(1) + ' KB', 'dur', s.dur)
}

for (const [k, v] of Object.entries(scenes)) build(k, v)
export { scenes }
