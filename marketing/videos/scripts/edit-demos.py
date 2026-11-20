#!/usr/bin/env python3
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

"""Edit raw demo webms into publish-ready MP4s (title/end cards + caption overlays + music).

Uses Pillow for text (this Homebrew ffmpeg lacks drawtext/subtitles) and ffmpeg overlay.

  python3 marketing/videos/scripts/edit-demos.py [01|all]
"""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RAW = ROOT / "out" / "raw"
FINAL = ROOT / "out" / "final"
THUMBS = ROOT / "out" / "thumbs"
CAP = ROOT / "captions"
ASSETS = ROOT / "assets"
BED = ASSETS / "bed.mp3"

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:
    sys.exit("Pillow required: pip3 install --user Pillow")


def run(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True)


def ensure_bed() -> None:
    ASSETS.mkdir(parents=True, exist_ok=True)
    if BED.exists():
        return
    print(f"Generating ambient bed → {BED}")
    run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=110:sample_rate=44100",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=165:sample_rate=44100",
            "-f",
            "lavfi",
            "-i",
            "anoisesrc=color=pink:amplitude=0.015:sample_rate=44100",
            "-filter_complex",
            "[0][1]amix=inputs=2:duration=longest,volume=0.12[a];[a][2]amix=inputs=2:duration=longest,volume=0.55,afade=t=in:st=0:d=2",
            "-t",
            "120",
            "-c:a",
            "libmp3lame",
            "-q:a",
            "4",
            str(BED),
        ]
    )


def font(size: int) -> ImageFont.ImageFont:
    for path in (
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    ):
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def card(path: Path, lines: list[tuple[str, int, str]], bg: str = "#0b1220") -> None:
    img = Image.new("RGB", (1920, 1080), bg)
    draw = ImageDraw.Draw(img)
    y = 340
    for text, size, color in lines:
        f = font(size)
        bbox = draw.textbbox((0, 0), text, font=f)
        w = bbox[2] - bbox[0]
        draw.text(((1920 - w) / 2, y), text, fill=color, font=f)
        y += size + 28
    img.save(path, "PNG")


def caption_png(path: Path, text: str) -> None:
    """Semi-transparent lower-third caption bar."""
    img = Image.new("RGBA", (1920, 1080), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    f = font(36)
    bbox = draw.textbbox((0, 0), text, font=f)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    pad_x, pad_y = 28, 16
    bar_w, bar_h = tw + pad_x * 2, th + pad_y * 2
    x = (1920 - bar_w) // 2
    y = 1080 - 120 - bar_h
    draw.rounded_rectangle((x, y, x + bar_w, y + bar_h), radius=12, fill=(11, 18, 32, 200))
    draw.text((x + pad_x, y + pad_y - 2), text, fill=(94, 234, 212, 255), font=f)
    img.save(path, "PNG")


def parse_srt_lines(srt: Path) -> list[str]:
    if not srt.exists():
        return []
    texts: list[str] = []
    for block in srt.read_text().strip().split("\n\n"):
        lines = [ln for ln in block.splitlines() if ln.strip() and "-->" not in ln and not ln.strip().isdigit()]
        if lines:
            texts.append(" ".join(lines))
    return texts[:4]


def png_to_video(png: Path, out: Path, seconds: float) -> None:
    run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-loop",
            "1",
            "-i",
            str(png),
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=44100:cl=stereo",
            "-t",
            str(seconds),
            "-r",
            "30",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
            str(out),
        ]
    )


def edit_one(clip: dict) -> None:
    cid, slug, title = clip["id"], clip["slug"], clip["title"]
    # Prefer mp4 when both exist (partial .webm from a killed encode can truncate the edit).
    raw_mp4 = RAW / f"{cid}-{slug}.mp4"
    raw_webm = RAW / f"{cid}-{slug}.webm"
    raw = raw_mp4 if raw_mp4.exists() else raw_webm
    out = FINAL / f"{cid}-{slug}.mp4"
    thumb = THUMBS / f"{cid}-{slug}.jpg"
    if not raw.exists():
        print(f"skip {cid} — missing {cid}-{slug}.webm/.mp4")
        return
    print(f"▶ Editing {cid}-{slug}")
    FINAL.mkdir(parents=True, exist_ok=True)
    THUMBS.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="machina-edit-") as tmp:
        tdir = Path(tmp)
        title_png = tdir / "title.png"
        end_png = tdir / "end.png"
        card(
            title_png,
            [
                ("MACHINA", 28, "#5eead4"),
                (title, 48, "#ffffff"),
                ("zyvor.dev", 22, "#94a3b8"),
            ],
        )
        card(
            end_png,
            [
                ("Try Machina", 44, "#ffffff"),
                ("zyvor.dev/machina", 26, "#5eead4"),
            ],
        )
        title_mp4 = tdir / "title.mp4"
        end_mp4 = tdir / "end.mp4"
        body_mp4 = tdir / "body.mp4"
        silent = tdir / "silent.mp4"
        png_to_video(title_png, title_mp4, 2.2)
        png_to_video(end_png, end_mp4, 2.0)

        # Body: scale + fade + optional caption overlays
        caps = parse_srt_lines(CAP / f"{cid}-{slug}.srt")
        cap_pngs: list[Path] = []
        for i, text in enumerate(caps):
            p = tdir / f"cap{i}.png"
            caption_png(p, text)
            cap_pngs.append(p)

        # Timing windows for captions on the body timeline
        windows = [(0.5, 3.5), (4.0, 8.0), (12.0, 17.0), (28.0, 33.0)]
        inputs = ["-i", str(raw)]
        for p in cap_pngs:
            inputs += ["-i", str(p)]

        fc_parts = [
            "[0:v]fade=t=in:st=0:d=0.5,scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,fps=30[base]"
        ]
        last = "base"
        for i, _ in enumerate(cap_pngs):
            start, end = windows[i] if i < len(windows) else (40 + i * 5, 44 + i * 5)
            nxt = f"v{i}"
            fc_parts.append(
                f"[{last}][{i+1}:v]overlay=0:0:enable='between(t\\,{start}\\,{end})'[{nxt}]"
            )
            last = nxt
        filter_complex = ";".join(fc_parts)

        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                *inputs,
                "-t",
                "70",
                "-filter_complex",
                filter_complex,
                "-map",
                f"[{last}]",
                "-an",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
                str(body_mp4),
            ]
        )

        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                str(title_mp4),
                "-i",
                str(body_mp4),
                "-i",
                str(end_mp4),
                "-filter_complex",
                "[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]",
                "-map",
                "[v]",
                "-an",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                str(silent),
            ]
        )

        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                str(silent),
                "-stream_loop",
                "-1",
                "-i",
                str(BED),
                "-filter_complex",
                "[1:a]volume=0.18,afade=t=in:st=0:d=1.5[a];[0:v]format=yuv420p[v]",
                "-map",
                "[v]",
                "-map",
                "[a]",
                "-shortest",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "19",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "160k",
                "-movflags",
                "+faststart",
                str(out),
            ]
        )

        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                "00:00:04",
                "-i",
                str(out),
                "-frames:v",
                "1",
                "-q:v",
                "2",
                str(thumb),
            ]
        )
    print(f"✓ {out}")


def main() -> None:
    want = sys.argv[1] if len(sys.argv) > 1 else "all"
    ensure_bed()
    clips = json.loads((ROOT / "clips.json").read_text())["clips"]
    for clip in clips:
        if want != "all" and clip["id"] != want and clip["slug"] != want:
            continue
        try:
            edit_one(clip)
        except subprocess.CalledProcessError as e:
            print(f"✗ {clip['id']}: ffmpeg failed ({e})", file=sys.stderr)
    print(f"Done → {FINAL}")


if __name__ == "__main__":
    main()
