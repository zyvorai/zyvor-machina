#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Send Machina deploy / live UX E2E summary email via SMTP (Zoho-compatible)."""
from __future__ import annotations

import argparse
import json
import os
import smtplib
import ssl
import sys
from datetime import datetime, timezone
from email.message import EmailMessage
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_REPORT = ROOT / "docs" / "ux-wiring-live-report.json"


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


def read_report(path: Path) -> dict:
    if not path.is_file():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return {}


def build_body(args: argparse.Namespace, report: dict) -> tuple[str, str]:
    host = args.host or report.get("host") or "unknown"
    generated = report.get("generated_at") or datetime.now(timezone.utc).isoformat()
    passed = report.get("passed", 0)
    failed = report.get("failed", 0)
    skipped = report.get("skipped", 0)
    total = passed + failed + skipped

    lines = [
        f"Machina deploy / E2E report",
        f"Host: {host}",
        f"Generated: {generated}",
        "",
        f"Live UX wiring: {passed} passed, {failed} failed, {skipped} skipped ({total} total)",
    ]
    if args.api_e2e_summary:
        lines.extend(["", f"API E2E: {args.api_e2e_summary}"])
    if args.vm_e2e_summary:
        lines.extend(["", f"Live VM lifecycle: {args.vm_e2e_summary}"])
    if args.services_summary:
        lines.extend(["", "Services:", args.services_summary])
    if args.overall:
        lines.extend(["", f"Overall: {args.overall}"])

    failures = [r for r in report.get("results", []) if r.get("status") == "failed"]
    if failures:
        lines.extend(["", "Failed routes:"])
        for row in failures[:40]:
            lines.append(f"  - {row.get('id', row.get('path'))}: {row.get('reason', 'failed')}")
        if len(failures) > 40:
            lines.append(f"  ... and {len(failures) - 40} more")

    text = "\n".join(lines) + "\n"
    html = "<pre style=\"font-family:monospace;font-size:13px\">" + (
        "\n".join(lines).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    ) + "</pre>"
    return text, html


def send_mail(subject: str, text: str, html: str, recipients: list[str]) -> None:
    host = os.environ.get("SMTP_HOST", "").strip()
    port = int(os.environ.get("SMTP_PORT", "587") or "587")
    user = os.environ.get("SMTP_USERNAME", "").strip()
    password = os.environ.get("SMTP_PASSWORD", "").strip()
    sender = os.environ.get("SMTP_FROM", user).strip()
    use_tls = env_bool("SMTP_USE_TLS", True)

    if not host or not user or not password or not sender:
        print("SMTP not configured — set SMTP_HOST, SMTP_USERNAME, SMTP_PASSWORD, SMTP_FROM", file=sys.stderr)
        sys.exit(2)
    if not recipients:
        print("No recipients configured — set DEPLOY_REPORT_TO or CONTACT_TO", file=sys.stderr)
        sys.exit(2)

    msg = EmailMessage()
    msg["Subject"] = subject
    msg["From"] = sender
    msg["To"] = ", ".join(recipients)
    msg.set_content(text)
    msg.add_alternative(html, subtype="html")

    if use_tls and port == 465:
        context = ssl.create_default_context()
        with smtplib.SMTP_SSL(host, port, context=context, timeout=60) as smtp:
            smtp.login(user, password)
            smtp.send_message(msg)
    else:
        with smtplib.SMTP(host, port, timeout=60) as smtp:
            if use_tls:
                smtp.starttls(context=ssl.create_default_context())
            smtp.login(user, password)
            smtp.send_message(msg)


def parse_recipients() -> list[str]:
    raw = os.environ.get("DEPLOY_REPORT_TO") or os.environ.get("CONTACT_TO") or ""
    return [x.strip() for x in raw.split(",") if x.strip()]


def main() -> None:
    parser = argparse.ArgumentParser(description="Send Machina deploy/E2E report email")
    parser.add_argument("--host", help="Target host/IP")
    parser.add_argument("--report", type=Path, default=DEFAULT_REPORT)
    parser.add_argument("--api-e2e-summary")
    parser.add_argument("--vm-e2e-summary")
    parser.add_argument("--services-summary")
    parser.add_argument("--overall")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    load_env()
    report = read_report(args.report)
    prefix = os.environ.get("DEPLOY_REPORT_SUBJECT_PREFIX", "[Machina Deploy]").strip()
    host = args.host or report.get("host") or "unknown"
    failed = report.get("failed", 0)
    status = "PASS" if failed == 0 and args.overall != "FAIL" else "FAIL"
    subject = f"{prefix} {status} — {host}"

    text, html = build_body(args, report)
    recipients = parse_recipients()

    if args.dry_run:
        print(f"Subject: {subject}")
        print(f"To: {', '.join(recipients)}")
        print(text)
        return

    send_mail(subject, text, html, recipients)
    print(f"Sent deploy report to {', '.join(recipients)}")


if __name__ == "__main__":
    main()
