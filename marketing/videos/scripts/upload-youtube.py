#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Upload Machina demo MP4s to YouTube (reuses Zeus OS OAuth token)."""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

from google.auth.transport.requests import Request
from google.oauth2.credentials import Credentials
from google_auth_oauthlib.flow import InstalledAppFlow
from googleapiclient.discovery import build
from googleapiclient.errors import HttpError
from googleapiclient.http import MediaFileUpload

SCOPES = ["https://www.googleapis.com/auth/youtube"]
CHANNEL_ID = "UCzTms7SYM_NpwTGbBHHGGYw"


def get_creds(client_secrets: Path, token_path: Path) -> Credentials:
    creds = None
    if token_path.exists():
        creds = Credentials.from_authorized_user_file(str(token_path), SCOPES)
    if creds and creds.expired and creds.refresh_token:
        creds.refresh(Request())
        token_path.write_text(creds.to_json())
    if not creds or not creds.valid:
        flow = InstalledAppFlow.from_client_secrets_file(str(client_secrets), SCOPES)
        print("Opening browser for Google/YouTube consent…", flush=True)
        print(f"Sign in with the account that owns channel {CHANNEL_ID}", flush=True)
        try:
            creds = flow.run_local_server(port=8080, prompt="consent", open_browser=True)
        except OSError:
            creds = flow.run_local_server(port=0, prompt="consent", open_browser=True)
        token_path.write_text(creds.to_json())
    return creds


def upload_one(youtube, path: Path, title: str, description: str, tags: list[str], privacy: str) -> dict:
    body = {
        "snippet": {
            "title": title[:100],
            "description": description,
            "tags": tags,
            "categoryId": "28",  # Science & Technology
        },
        "status": {
            "privacyStatus": privacy,
            "selfDeclaredMadeForKids": False,
        },
    }
    media = MediaFileUpload(str(path), mimetype="video/mp4", resumable=True, chunksize=8 * 1024 * 1024)
    request = youtube.videos().insert(part="snippet,status", body=body, media_body=media)
    response = None
    while response is None:
        status, response = request.next_chunk()
        if status:
            print(f"  … {int(status.progress() * 100)}%", flush=True)
    return response


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--videos-dir", required=True)
    ap.add_argument("--clips-json", required=True)
    ap.add_argument("--client-secrets", required=True)
    ap.add_argument("--token", required=True)
    ap.add_argument("--state", required=True)
    ap.add_argument("--privacy", default="unlisted", choices=["private", "unlisted", "public"])
    ap.add_argument("--ids-out", default="")
    args = ap.parse_args()

    videos_dir = Path(args.videos_dir)
    clips = json.loads(Path(args.clips_json).read_text())["clips"]
    state_path = Path(args.state)
    state = {"uploaded": {}}
    if state_path.exists():
        state = json.loads(state_path.read_text())
    state.setdefault("uploaded", {})

    creds = get_creds(Path(args.client_secrets), Path(args.token))
    youtube = build("youtube", "v3", credentials=creds)
    mine = youtube.channels().list(part="id,snippet", mine=True).execute()
    items = mine.get("items") or []
    if not items:
        print("ERROR: authorized account has no YouTube channel", file=sys.stderr)
        return 2
    ch = items[0]
    print(f"Authorized channel: {ch['snippet']['title']} ({ch['id']})")

    id_map: dict[str, str] = {}
    ok = 0
    for clip in clips:
        fname = f"{clip['id']}-{clip['slug']}.mp4"
        path = videos_dir / fname
        if not path.exists():
            print(f"MISSING {fname}", file=sys.stderr)
            continue
        if fname in state["uploaded"]:
            vid = state["uploaded"][fname]["id"]
            print(f"SKIP {fname} -> https://youtu.be/{vid}")
            id_map[clip["id"]] = vid
            ok += 1
            continue

        title = clip.get("youtubeTitle") or clip["title"]
        desc = (
            f"{clip.get('description', '')}\n\n"
            "Machina — enterprise Linux hypervisor platform for libvirt / QEMU / KVM.\n"
            "→ https://zyvor.dev/machina\n"
            "→ Trial: https://zyvor.dev/contact?intent=trial&product=machina\n\n"
            "#Machina #KVM #libvirt #Zyvor #virtualization"
        )
        tags = list(clip.get("tags") or ["Machina", "Zyvor", "KVM", "libvirt", "demo"])
        print(f"UPLOAD {fname} ({path.stat().st_size // (1024*1024)} MiB) privacy={args.privacy}")
        try:
            resp = upload_one(youtube, path, title, desc, tags, args.privacy)
            vid = resp["id"]
            url = f"https://youtu.be/{vid}"
            state["uploaded"][fname] = {
                "id": vid,
                "url": url,
                "title": resp["snippet"]["title"],
                "clipId": clip["id"],
            }
            state_path.parent.mkdir(parents=True, exist_ok=True)
            state_path.write_text(json.dumps(state, indent=2))
            id_map[clip["id"]] = vid
            print(f"  OK {url}")
            ok += 1
        except HttpError as e:
            print(f"  FAIL {e}", file=sys.stderr)
            return 3
        time.sleep(2)

    if args.ids_out:
        Path(args.ids_out).write_text(json.dumps(id_map, indent=2) + "\n")
        print(f"Wrote IDs → {args.ids_out}")

    print(f"Done {ok}/{len(clips)}")
    print(json.dumps(id_map, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
