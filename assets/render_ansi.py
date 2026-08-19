#!/usr/bin/env python3
"""Render ANSI terminal captures (tmux capture-pane -e -p) to PNG.

Usage: render_ansi.py input.txt output.png [scale]
"""
import re
import sys
from PIL import Image, ImageDraw, ImageFont

# 16-color xterm palette
PALETTE = {
    "30": (0, 0, 0), "31": (205, 49, 49), "32": (13, 188, 121), "33": (36, 173, 214),
    "34": (36, 114, 200), "35": (188, 82, 21), "36": (17, 168, 205), "37": (229, 229, 234),
    "90": (102, 102, 102), "91": (241, 76, 76), "92": (35, 209, 139), "93": (127, 214, 253),
    "94": (82, 139, 255), "95": (255, 163, 106), "96": (90, 200, 250), "97": (255, 255, 255),
}

FONT_CANDIDATES = [
    "/usr/share/fonts/TTF/CaskaydiaMonoNerdFontMono-Regular.ttf",
    "/usr/share/fonts/TTF/CaskaydiaMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
]

ESC = re.compile(r"\x1b\[([0-9;]*)m")
CSI_OTHER = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[()][A-B0]")


def load_font(size):
    for p in FONT_CANDIDATES:
        try:
            return ImageFont.truetype(p, size), True
        except OSError:
            continue
    return ImageFont.load_default(), False


def parse_ansi(text):
    """→ list of rows, each row a list of (char, fg, bold)."""
    rows = []
    cur = []
    fg, bold = None, False
    for line in text.split("\n"):
        cur = []
        fg, bold = None, False
        i = 0
        while i < len(line):
            m = ESC.match(line, i)
            if m:
                params = m.group(1)
                if params in ("", "0"):
                    fg, bold = None, False
                else:
                    for p in params.split(";"):
                        if p == "1":
                            bold = True
                        elif p == "22":
                            bold = False
                        elif p in PALETTE:
                            fg = p
                        elif p.startswith("38"):
                            # 38;5;n → map n into palette approx (ratatui base colors only)
                            pass
                        elif p == "39":
                            fg = None
                i = m.end()
                continue
            m2 = CSI_OTHER.match(line, i)
            if m2:
                i = m2.end()
                continue
            cur.append((line[i], fg, bold))
            i += 1
        rows.append(cur)
    return rows


def render(text, out_path, scale=1):
    rows = parse_ansi(text)
    cols = max((len(r) for r in rows), default=0)
    font_size = 20 * scale
    font, true_font = load_font(font_size)
    cw = (font_size + 1) if true_font else int(font_size * 0.6)
    ch = int(font_size * 1.25)
    pad = 12
    W = pad * 2 + cols * cw
    H = pad * 2 + len(rows) * ch
    img = Image.new("RGB", (W, H), (11, 13, 19))  # dark bg
    d = ImageDraw.Draw(img)
    default_fg = (200, 204, 210)
    for y, row in enumerate(rows):
        for x, (c, fg, bold) in enumerate(row):
            if c.strip() == "":
                continue
            col = PALETTE.get(fg, default_fg) if fg else (default_fg if not bold else (255, 255, 255))
            if bold:
                col = tuple(min(255, int(v * 1.1) + 20) for v in col)
            d.text((pad + x * cw, pad + y * ch), c, fill=col, font=font)
    img.save(out_path)
    print(f"{out_path}: {W}x{H}, {len(rows)} rows, {cols} cols")


if __name__ == "__main__":
    scale = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    render(open(sys.argv[1]).read(), sys.argv[2], scale)
