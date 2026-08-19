#!/usr/bin/env python3
"""Render ANSI terminal captures (tmux capture-pane -e -p) to PNG.

Full SGR support: basic 16, 256-color (38;5;N / 48;5;N), truecolor
(38;2;R;G;B), bold, foregrounds AND backgrounds.

Usage: render_ansi.py input.txt output.png [scale]
"""
import re
import sys
from PIL import Image, ImageDraw, ImageFont

# --- xterm 256-color palette --------------------------------------------
BASE16 = [
    (0, 0, 0), (205, 0, 0), (0, 205, 0), (205, 205, 0),
    (0, 0, 238), (205, 0, 205), (0, 205, 205), (229, 229, 229),
    (127, 127, 127), (255, 0, 0), (0, 255, 0), (255, 255, 0),
    (92, 92, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
]

def _build_palette():
    pal = list(BASE16)
    # 6x6x6 color cube (16..231)
    steps = [0, 95, 135, 175, 215, 255]
    for r in steps:
        for g in steps:
            for b in steps:
                pal.append((r, g, b))
    # grayscale ramp (232..255)
    for i in range(24):
        v = 8 + i * 10
        pal.append((v, v, v))
    return pal

PALETTE256 = _build_palette()

BASIC_FG = {**{str(30 + i): BASE16[i] for i in range(8)},
            **{str(90 + i): BASE16[8 + i] for i in range(8)}}
BASIC_BG = {**{str(40 + i): BASE16[i] for i in range(8)},
            **{str(100 + i): BASE16[8 + i] for i in range(8)}}

FONT_CANDIDATES = [
    "/usr/share/fonts/TTF/CaskaydiaMonoNerdFontMono-Regular.ttf",
    "/usr/share/fonts/TTF/CaskaydiaMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
]

ESC = re.compile(r"\x1b\[([0-9;]*)m")
CSI_OTHER = re.compile(r"\x1b\[[0-9;?]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b[()][A-B0]")


def apply_sgr(params, fg, bg, bold):
    """Apply one SGR sequence's parameter list to text state."""
    p = [x for x in params.split(";")] if params else ["0"]
    i = 0
    while i < len(p):
        v = p[i]
        if v in ("", "0"):
            fg, bg, bold = None, None, False
        elif v == "1":
            bold = True
        elif v == "22":
            bold = False
        elif v == "39":
            fg = None
        elif v == "49":
            bg = None
        elif v in BASIC_FG:
            fg = BASIC_FG[v]
        elif v in BASIC_BG:
            bg = BASIC_BG[v]
        elif v in ("38", "48"):
            # indexed next: 5;N  or  2;R;G;B
            if i + 1 < len(p):
                mode = p[i + 1]
                if mode == "5" and i + 2 < len(p):
                    n = int(p[i + 2])
                    color = PALETTE256[n] if 0 <= n < 256 else None
                    if v == "38":
                        fg = color
                    else:
                        bg = color
                    i += 2
                elif mode == "2" and i + 4 < len(p):
                    color = (int(p[i + 2]), int(p[i + 3]), int(p[i + 4]))
                    if v == "38":
                        fg = color
                    else:
                        bg = color
                    i += 4
        i += 1
    return fg, bg, bold


def parse_ansi(text):
    """→ list of rows; row = list of (char, fg, bg, bold)."""
    rows = []
    for line in text.split("\n"):
        row = []
        fg = bg = None
        bold = False
        i = 0
        while i < len(line):
            m = ESC.match(line, i)
            if m:
                fg, bg, bold = apply_sgr(m.group(1), fg, bg, bold)
                i = m.end()
                continue
            m2 = CSI_OTHER.match(line, i)
            if m2:
                i = m2.end()
                continue
            row.append((line[i], fg, bg, bold))
            i += 1
        rows.append(row)
    return rows


def render(text, out_path, scale=1):
    rows = parse_ansi(text)
    cols = max((len(r) for r in rows), default=0)
    font_size = 20 * scale
    font, true_font = load_font(font_size)
    cw = (font_size + 1) if true_font else int(font_size * 0.6)
    ch = int(font_size * 1.25)
    pad = 12 * scale
    W = pad * 2 + cols * cw
    H = pad * 2 + len(rows) * ch
    img = Image.new("RGB", (W, H), (11, 13, 19))
    d = ImageDraw.Draw(img)
    default_fg = (200, 204, 210)
    for y, row in enumerate(rows):
        for x, (c, fg, bg, bold) in enumerate(row):
            px, py = pad + x * cw, pad + y * ch
            if bg:
                d.rectangle([px, py, px + cw - 1, py + ch - 1], fill=bg)
            if c in (" ", "\t"):
                continue
            col = fg or (default_fg if not bold else (240, 240, 240))
            if bold:
                col = tuple(min(255, int(v * 1.08) + 18) for v in col)
            d.text((px, py), c, fill=col, font=font)
    img.save(out_path)
    print(f"{out_path}: {W}x{H}, {len(rows)} rows, {cols} cols")


def load_font(size):
    for p in FONT_CANDIDATES:
        try:
            return ImageFont.truetype(p, size), True
        except OSError:
            continue
    return ImageFont.load_default(), False


if __name__ == "__main__":
    scale = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    render(open(sys.argv[1]).read(), sys.argv[2], scale)
