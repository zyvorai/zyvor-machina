#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""List every video on the authorized channel and set privacyStatus=public.

Usage:
  python3 publish-all-channel-videos.py --token PATH [--dry-run]
"""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from google.auth.transport.requests import Request
from google.oauth2.credentials import Credentials
from googleapiclient.discovery import build
from googleapiclient.errors import HttpError

SCOPES = ["https://www.googleapis.com/auth/youtube"]


def get_creds(token_path: Path) -> Credentials:
    creds = Credentials.from_authorized_user_file(str(token_path), SCOPES)
    if creds and creds.expired and creds.refresh_token:
        creds.refresh(Request())
        token_path.write_text(creds.to_json())
    return creds


def list_all_video_ids(youtube) -> list[str]:
    ch = youtube.channels().list(part="contentDetails,snippet", mine=True).execute()
    items = ch.get("items") or []
    if not items:
        print("ERROR: authorized account has no YouTube channel", file=sys.stderr)
        sys.exit(2)
    channel = items[0]
    uploads_playlist = channel["contentDetails"]["relatedPlaylists"]["uploads"]
    print(f"Channel: {channel['snippet']['title']} ({channel['id']})")
    print(f"Uploads playlist: {uploads_playlist}")

    video_ids: list[str] = []
    page_token = None
    while True:
        resp = (
            youtube.playlistItems()
            .list(part="contentDetails", playlistId=uploads_playlist, maxResults=50, pageToken=page_token)
            .execute()
        )
        for item in resp.get("items", []):
            video_ids.append(item["contentDetails"]["videoId"])
        page_token = resp.get("nextPageToken")
        if not page_token:
            break
    return video_ids


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--token", required=True)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    creds = get_creds(Path(args.token))
    youtube = build("youtube", "v3", credentials=creds)

    video_ids = list_all_video_ids(youtube)
    print(f"Found {len(video_ids)} video(s) on channel")

    changed = 0
    already_public = 0
    failed = 0
    for i in range(0, len(video_ids), 50):
        batch = video_ids[i : i + 50]
        resp = youtube.videos().list(part="status,snippet", id=",".join(batch)).execute()
        for v in resp.get("items", []):
            vid = v["id"]
            title = v["snippet"]["title"]
            status = v["status"]["privacyStatus"]
            if status == "public":
                print(f"SKIP  (already public) {vid}  {title}")
                already_public += 1
                continue
            print(f"{'WOULD SET' if args.dry_run else 'SET'} {status} -> public  {vid}  {title}")
            if args.dry_run:
                changed += 1
                continue
            try:
                youtube.videos().update(
                    part="status",
                    body={"id": vid, "status": {"privacyStatus": "public", "selfDeclaredMadeForKids": False}},
                ).execute()
                changed += 1
            except HttpError as e:
                print(f"  FAIL {vid}: {e}", file=sys.stderr)
                failed += 1

    print(f"\nDone. already_public={already_public} changed={changed} failed={failed} total={len(video_ids)}")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
