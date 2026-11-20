#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Email Machina client-presentation PDFs (hyper2kvm slide-deck format)."""
from __future__ import annotations

import argparse
import fnmatch
import os
import smtplib
import ssl
import sys
from datetime import datetime, timezone
from email.mime.application import MIMEApplication
from email.mime.multipart import MIMEMultipart
from email.mime.text import MIMEText
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_PDF_DIR = ROOT / "docs" / "client-presentations"
DEFAULT_GLOB = "0[7-9]-*.pdf,1[01]-*.pdf"


def load_env() -> None:
    candidates = [
        ROOT / "scripts" / "deploy-mailer.env",
        ROOT.parent / "hypersdk-web" / "contact-mailer.env",
    ]
    for path in candidates:
        if not path.is_file():
            continue
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, val = line.partition("=")
            key = key.strip()
            val = val.strip()
            if key and key not in os.environ:
                os.environ[key] = val
        break


def env_bool(name: str, default: bool = True) -> bool:
    raw = os.environ.get(name)
    if raw is None:
        return default
    return raw.strip().lower() not in {"0", "false", "no", "off"}


def parse_list(name: str, fallback: str) -> list[str]:
    raw = os.environ.get(name, fallback)
    return [x.strip() for x in raw.split(",") if x.strip()]


def match_any(name: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatch(name, p.strip()) for p in patterns if p.strip())


def send_mail(
    subject: str,
    text: str,
    html: str,
    to_addrs: list[str],
    cc_addrs: list[str],
    attachments: list[Path],
) -> None:
    host = os.environ.get("SMTP_HOST", "").strip()
    port = int(os.environ.get("SMTP_PORT", "587") or "587")
    user = os.environ.get("SMTP_USERNAME", "").strip()
    password = os.environ.get("SMTP_PASSWORD", "").strip()
    sender = os.environ.get("SMTP_FROM", user).strip()
    use_tls = env_bool("SMTP_USE_TLS", True)

    if not host or not user or not password or not sender:
        print("SMTP not configured — set SMTP_* in deploy-mailer.env or hypersdk contact-mailer.env", file=sys.stderr)
        sys.exit(2)
    if not to_addrs:
        print("No recipients — set FEATURE_PDF_TO", file=sys.stderr)
        sys.exit(2)

    msg = MIMEMultipart("mixed")
    msg["Subject"] = subject
    msg["From"] = sender
    msg["To"] = ", ".join(to_addrs)
    if cc_addrs:
        msg["Cc"] = ", ".join(cc_addrs)

    alt = MIMEMultipart("alternative")
    alt.attach(MIMEText(text, "plain", "utf-8"))
    alt.attach(MIMEText(html, "html", "utf-8"))
    msg.attach(alt)

    for path in attachments:
        att = MIMEApplication(path.read_bytes(), _subtype="pdf")
        att.add_header("Content-Disposition", "attachment", filename=path.name)
        msg.attach(att)

    all_rcpt = to_addrs + cc_addrs
    if use_tls and port == 465:
        context = ssl.create_default_context()
        with smtplib.SMTP_SSL(host, port, context=context, timeout=60) as smtp:
            smtp.login(user, password)
            smtp.send_message(msg, from_addr=sender, to_addrs=all_rcpt)
    else:
        with smtplib.SMTP(host, port, timeout=60) as smtp:
            if use_tls:
                smtp.starttls(context=ssl.create_default_context())
            smtp.login(user, password)
            smtp.send_message(msg, from_addr=sender, to_addrs=all_rcpt)


def main() -> None:
    parser = argparse.ArgumentParser(description="Send Machina client-presentation PDFs")
    parser.add_argument("--pdf-dir", type=Path, default=DEFAULT_PDF_DIR)
    parser.add_argument(
        "--glob",
        default=os.environ.get("FEATURE_PDF_GLOB", DEFAULT_GLOB),
        help="Comma-separated fnmatch patterns (default: new feature decks 07–10)",
    )
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    load_env()
    pdf_dir: Path = args.pdf_dir
    patterns = [p.strip() for p in args.glob.split(",") if p.strip()]
    pdfs = sorted(
        p for p in pdf_dir.glob("*.pdf") if match_any(p.name, patterns)
    ) if pdf_dir.is_dir() else []
    if not pdfs:
        print(
            f"No PDFs matched in {pdf_dir} (patterns: {args.glob}) — "
            "run ./scripts/generate-feature-pdfs.sh first",
            file=sys.stderr,
        )
        sys.exit(1)

    to_addrs = parse_list("FEATURE_PDF_TO", "sibu@zyvor.dev")
    cc_addrs = parse_list("FEATURE_PDF_CC", "ssahani@zyvor.dev")
    prefix = os.environ.get("FEATURE_PDF_SUBJECT_PREFIX", "[Machina Features]").strip()
    when = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")

    subject = f"{prefix} New platform decks — VM Detail UX, Cinema, Connect hub, QA matrix ({len(pdfs)} PDFs)"
    names = "\n".join(f"  - {p.name}" for p in pdfs)
    text = "\n".join(
        [
            "Machina client presentation PDFs (docs/client-presentations format)",
            "",
            f"Generated: {when}",
            "",
            "Attached:",
            names,
            "",
            "Covers: Platform VM Detail UX, ConsoleHub Cinema/Studio, Connect hub daily access, F01–F13 QA matrix.",
            "",
            "HTML source: machina/docs/client-presentations/",
            "",
            "zyvor.dev · © 2026",
        ]
    )
    html_body = f"""<html><body style="font-family:system-ui,sans-serif;color:#111">
<h2>Machina client presentation PDFs</h2>
<p><b>Generated:</b> {when}</p>
<p>Format: <code>docs/client-presentations</code> (hyper2kvm slide-deck style)</p>
<ul>{''.join(f'<li>{p.name}</li>' for p in pdfs)}</ul>
<p>Platform VM Detail UX · ConsoleHub Cinema/Studio · Connect hub · Feature QA matrix F01–F13.</p>
<p><a href="https://zyvor.dev">zyvor.dev</a> · © 2026</p>
</body></html>"""

    if args.dry_run:
        print(f"Subject: {subject}")
        print(f"To: {', '.join(to_addrs)}")
        print(f"Cc: {', '.join(cc_addrs)}")
        print(text)
        return

    send_mail(subject, text, html_body, to_addrs, cc_addrs, pdfs)
    print(f"Sent {len(pdfs)} PDFs to {', '.join(to_addrs)} (cc: {', '.join(cc_addrs)})")


if __name__ == "__main__":
    main()
