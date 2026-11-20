#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Upload a Machina demo MP4 to YouTube (reuses Zeus OS OAuth token)."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from google.auth.exceptions import RefreshError
from google.auth.transport.requests import Request
from google.oauth2.credentials import Credentials
from google_auth_oauthlib.flow import InstalledAppFlow
from googleapiclient.discovery import build
from googleapiclient.http import MediaFileUpload

SCOPES = ["https://www.googleapis.com/auth/youtube"]

DEFAULT_TITLE = "Machina — KVM fleet desktop wow reel (Cinema + Mission Control)"
DEFAULT_DESCRIPTION = """Machina operator desktop for libvirt / QEMU / KVM — sign in, Mission Control launchpad, Linux + Windows guests, Machina Cinema live console, Live Preview Wall.

Pilot-ready single-host KVM. GuestKit agent on Linux. Windows golden via hyper2kvm.

→ https://zyvor.dev/machina
→ Trial: https://zyvor.dev/contact?intent=trial&product=machina

#Machina #KVM #libvirt #Cinema #Zyvor #virtualization
"""
TAGS = [
    "Machina",
    "KVM",
    "libvirt",
    "Cinema",
    "Zyvor",
    "virtualization",
    "GuestKit",
    "demo",
    "golden image",
]


def get_creds(client_secrets: Path, token_path: Path) -> Credentials:
    creds = None
    if token_path.exists():
        creds = Credentials.from_authorized_user_file(str(token_path), SCOPES)
    if creds and creds.expired and creds.refresh_token:
        try:
            creds.refresh(Request())
            token_path.write_text(creds.to_json())
        except RefreshError as e:
            print(f"Refresh token invalid/revoked ({e}) — falling back to interactive consent", flush=True)
            creds = None
    if not creds or not creds.valid:
        flow = InstalledAppFlow.from_client_secrets_file(str(client_secrets), SCOPES)
        print("Opening browser for Google/YouTube consent…", flush=True)
        try:
            creds = flow.run_local_server(port=8080, prompt="consent", open_browser=True)
        except OSError:
            creds = flow.run_local_server(port=0, prompt="consent", open_browser=True)
        token_path.write_text(creds.to_json())
    return creds


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--video", required=True)
    ap.add_argument("--client-secrets", required=True)
    ap.add_argument("--token", required=True)
    ap.add_argument("--privacy", default="public", choices=["private", "unlisted", "public"])
    ap.add_argument("--state", default="")
    ap.add_argument("--state-key", default="machina-wow-reel")
    ap.add_argument("--title", default=DEFAULT_TITLE)
    ap.add_argument("--description", default=DEFAULT_DESCRIPTION)
    args = ap.parse_args()

    path = Path(args.video)
    if not path.exists():
        print(f"missing {path}", file=sys.stderr)
        return 2

    creds = get_creds(Path(args.client_secrets), Path(args.token))
    youtube = build("youtube", "v3", credentials=creds)
    mine = youtube.channels().list(part="id,snippet", mine=True).execute()
    ch = (mine.get("items") or [None])[0]
    if not ch:
        print("ERROR: no YouTube channel on authorized account", file=sys.stderr)
        return 2
    print(f"Authorized channel: {ch['snippet']['title']} ({ch['id']})")

    body = {
        "snippet": {
            "title": args.title[:100],
            "description": args.description,
            "tags": TAGS,
            "categoryId": "28",
        },
        "status": {
            "privacyStatus": args.privacy,
            "selfDeclaredMadeForKids": False,
        },
    }
    media = MediaFileUpload(str(path), mimetype="video/mp4", resumable=True, chunksize=8 * 1024 * 1024)
    request = youtube.videos().insert(part="snippet,status", body=body, media_body=media)
    response = None
    print(f"UPLOAD {path.name} ({path.stat().st_size // 1024} KiB) privacy={args.privacy}")
    while response is None:
        status, response = request.next_chunk()
        if status:
            print(f"  … {int(status.progress() * 100)}%", flush=True)
    vid = response["id"]
    url = f"https://youtu.be/{vid}"
    print(f"OK {url}")
    if args.state:
        state_path = Path(args.state)
        state = {}
        if state_path.exists():
            state = json.loads(state_path.read_text())
        state[args.state_key] = {
            "id": vid,
            "url": url,
            "privacy": args.privacy,
            "title": args.title,
        }
        state_path.write_text(json.dumps(state, indent=2) + "\n")
        print(f"Wrote state → {state_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
