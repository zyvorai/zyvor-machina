#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Flip YouTube video privacyStatus (e.g. unlisted -> public) for given video IDs.

Usage:
  python3 set-privacy.py --client-secrets PATH --token PATH --privacy public ID [ID ...]
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


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--token", required=True)
    ap.add_argument("--privacy", default="public", choices=["private", "unlisted", "public"])
    ap.add_argument("video_ids", nargs="+")
    args = ap.parse_args()

    creds = get_creds(Path(args.token))
    youtube = build("youtube", "v3", credentials=creds)

    ok = 0
    for vid in args.video_ids:
        try:
            youtube.videos().update(
                part="status",
                body={"id": vid, "status": {"privacyStatus": args.privacy, "selfDeclaredMadeForKids": False}},
            ).execute()
            print(f"OK {vid} -> {args.privacy}")
            ok += 1
        except HttpError as e:
            print(f"FAIL {vid}: {e}", file=sys.stderr)

    print(f"Done {ok}/{len(args.video_ids)}")
    return 0 if ok == len(args.video_ids) else 1


if __name__ == "__main__":
    raise SystemExit(main())
