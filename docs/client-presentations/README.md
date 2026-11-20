# Machina Client Presentations

**6 HTML+PDF slide decks** covering business value, architecture, security, technical deep dives, ROI analysis, and quick-start guides. Perfect for sales pitches, stakeholder briefings, technical evaluations, and POC planning.

All presentations available as **interactive HTML** (viewable in any browser) and **PDF** (printable/shareable).

**License:** Proprietary (HyperSDK)

---

## Quick Links

- **Executive Overview** → Start here: [01-business-value.html](01-business-value.html)
- **Technical Deep Dive** → [03-technical-architecture.html](03-technical-architecture.html)
- **Security & Compliance** → [05-security-compliance.html](05-security-compliance.html)
- **Getting Started** → [04-quickstart-guide.html](04-quickstart-guide.html)
- **Financial ROI** → [06-roi-calculator.html](06-roi-calculator.html)
- **Pricing & Licensing** → [02-pricing-licensing.html](02-pricing-licensing.html)

---

## Presentation Index

### Executive & Business (3 decks)

| # | Title | Use Case | Audience | Pages |
|---|-------|----------|----------|-------|
| **01** | **Business Value** | Market value proposition, key differentiation, competitive positioning | C-suite, VP Infrastructure, Board | 8 |
| **02** | **Pricing & Licensing** | TCO comparison, licensing models, cost-benefit analysis vs VMware/Proxmox | Finance, Procurement, Legal | 6 |
| **06** | **ROI Calculator** | Financial justification tool, implementation costs, payback period | Finance, Project Managers | 6 |

### Architecture & Technical (1 deck)

| # | Title | Focus | Audience | Pages |
|---|-------|-------|----------|-------|
| **03** | **Technical Architecture** | Full platform: Rust daemon, libvirt bindings, web client, API design | Architects, DevOps, Platform Engineers | 8 |

### Security (1 deck)

| # | Title | Focus | Standards | Pages |
|---|-------|-------|-----------|-------|
| **05** | **Security & Compliance** | PAM auth, RBAC, API tokens, session management, audit trails, TLS | SOC2, HIPAA considerations | 6 |

### Quick Start & Sales (1 deck)

| # | Title | Use Case | Audience | Pages |
|---|-------|----------|----------|-------|
| **04** | **Quickstart Guide** | Installation, initial VM setup, POC workflow, first console session | New users, Solutions Architects | 6 |

### Platform UX & QA (new — June 2026)

| # | Title | Focus | Audience | Pages |
|---|-------|-------|----------|-------|
| **07** | **Platform VM Detail UX** | Hero, action bar, attention stack, Connect hub, Access tab | Platform operators, solutions architects | 6 |
| **08** | **ConsoleHub Cinema & Studio** | Machina Cinema, Studio, Mission Control wall, Ops Shelf | Console operators, NOC | 5 |
| **09** | **Connect Hub & Daily Access** | SSH, NAT laptop path, export, Access tab deep work | Daily VM operators | 4 |
| **10** | **Platform Feature QA Matrix** | F01–F18 Playwright matrix (mock + live) | QA, release engineering | 5 |
| **11** | **VM Disks, Compute & Devices** | Attach/resize disk, Edit CPU/memory, ISO, NIC, snapshots, migrate | Platform operators, infra engineers | 5 |

---

## Generating PDFs

HTML decks live in this directory (same format as `hyper2kvm-/docs/client-presentations`). Generate PDFs with Playwright:

```bash
# All decks (01–10)
./scripts/generate-client-presentation-pdfs.sh

# New feature decks only (07–11)
./scripts/generate-feature-pdfs.sh

# Mail to sibu@zyvor.dev (cc ssahani@zyvor.dev)
./scripts/mail-feature-pdfs.sh
```

### Option 1: Browser Print (Manual)
```bash
# On Mac: Cmd+P → Save as PDF
# On Windows: Ctrl+P → Print to PDF
# Recommended: Use Chrome/Chromium for best results
```

### Option 2: Headless Chrome (Automated)
```bash
# Install Puppeteer
npm install -g puppeteer

# Generate single PDF
npx puppeteer print 01-business-value.html 01-business-value.pdf

# Batch generate all
for file in *.html; do
  npx puppeteer print "$file" "${file%.html}.pdf"
done
```

### Option 3: wkhtmltopdf (Batch)
```bash
# Install wkhtmltopdf (requires system install)
brew install --cask wkhtmltopdf  # macOS
sudo apt install wkhtmltopdf     # Ubuntu/Debian

# Generate all PDFs
for file in *.html; do
  wkhtmltopdf "$file" "${file%.html}.pdf"
done
```

---

## How to Use

### 📺 Viewing

1. **In Browser** — Open any `.html` file directly in your web browser
2. **Print to PDF** — Use browser print function (Cmd+P / Ctrl+P) → "Save as PDF"
3. **Presentation Mode** — Press `F` for fullscreen in most browsers

### 📤 Sharing

- **Email** — Attach HTML file or generated PDF
- **Hosted** — Upload to S3, SharePoint, or internal Wiki
- **Embedded** — Embed HTML iframe in Salesforce, HubSpot, or portals
- **Print** — Print on demand for in-person meetings

### 🖨️ Customization

All presentations use **consistent branding** and can be customized:

- **Logo** — Replace in cover slide and footer
- **Color Scheme** — Edit CSS variables (currently using Machina brand colors)
- **Company Name** — Search & replace "Machina" or add company footer
- **Contact Info** — Add company contact/website to final slides

---

## Audience Guide

### 👔 For C-Suite / Board
- Start with **01-Business-Value** for strategic overview
- Follow with **06-ROI-Calculator** for financial impact
- Reference **02-Pricing-Licensing** for budget discussion

### 🏗️ For Infrastructure / DevOps Teams
- **03-Technical-Architecture** covers full platform details
- **04-Quickstart-Guide** enables hands-on POC setup
- **05-Security-Compliance** addresses operational concerns

### 💰 For Finance / Procurement
- **01-Business-Value** provides market justification
- **06-ROI-Calculator** enables scenario modeling
- **02-Pricing-Licensing** discusses investment options

### 🔐 For Security / Compliance
- **05-Security-Compliance** covers auth, RBAC, audit
- **03-Technical-Architecture** shows security design
- Reference company's compliance requirements

---

## Support & Updates

- **Latest Version** — This directory is updated with every Machina release
- **Feedback** — Report issues or request new decks via GitHub Issues
- **Custom Deck** — Need a specific presentation? Submit a feature request

---

**Happy presenting! 🎉**
