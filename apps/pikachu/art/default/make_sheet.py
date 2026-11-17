"""Draws the stand-in Pikachu art (a plug for a tail) as
sheet.png next to this file. Run with: python3 make_sheet.py (needs Pillow).

Frames are 40x40, facing right. Rows match sheet.toml.
"""

from pathlib import Path

from PIL import Image, ImageDraw

W = H = 40
YELLOW = (255, 212, 59)
SHADE = (232, 163, 23)
BELLY = (255, 241, 184)
EAR_TIP = (46, 155, 255)
CHEEK = (79, 195, 255)
EYE = (27, 27, 47)
WHITE = (255, 255, 255)
OUTLINE = (59, 42, 18)
FOOT = (232, 155, 28)
CABLE = (74, 79, 90)
PLUG = (184, 194, 204)
PRONG = (230, 235, 240)
MOUTH = (122, 46, 31)
TONGUE = (255, 107, 107)


def oval(d, cx, cy, rx, ry, fill):
    d.ellipse([cx - rx, cy - ry, cx + rx, cy + ry], fill=fill)


def frame(bob=0, step=0, eyes="open", mouth="smile", arms="down", ears="up", sleep=False, tail=0):
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    low = 4 if sleep else 0  # curled up: everything above the feet sinks
    top = bob + low

    # Tail: a cable from the back that ends in a plug.
    sway = tail
    d.line([(9, 31), (5, 28), (4 + sway, 22)], fill=CABLE, width=2)
    d.rectangle([1 + sway, 14 + low, 7 + sway, 20 + low], fill=PLUG)
    d.rectangle([2 + sway, 11 + low, 3 + sway, 13 + low], fill=PRONG)
    d.rectangle([5 + sway, 11 + low, 6 + sway, 13 + low], fill=PRONG)

    # Feet, which move when walking.
    if not sleep:
        oval(d, 14 + step, 36, 3, 2, FOOT)
        oval(d, 23 - step, 36, 3, 2, FOOT)

    # Body and head, shaded on the lower left.
    oval(d, 18, 28 + low // 2, 11, 9 - low // 2, SHADE)
    oval(d, 19, 27 + low // 2, 10, 8 - low // 2, YELLOW)
    oval(d, 21, 30 + low // 2, 6, 5 - low // 2, BELLY)
    oval(d, 24, 18 + top, 10, 9, SHADE)
    oval(d, 25, 17 + top, 9, 8, YELLOW)

    # Ears with blue tips; drooped when sleepy or dizzy.
    for ex in (19, 29):
        if ears == "up":
            oval(d, ex, 8 + top, 3, 5, YELLOW)
            oval(d, ex, 5 + top, 2, 2, EAR_TIP)
        else:
            oval(d, ex + (-3 if ex < 24 else 3), 11 + top, 4, 2, YELLOW)
            oval(d, ex + (-6 if ex < 24 else 6), 11 + top, 1, 1, EAR_TIP)

    # Arms.
    if arms == "up":
        oval(d, 13, 21 + top, 2, 3, YELLOW)
        oval(d, 33, 22 + top, 2, 3, YELLOW)
    elif not sleep:
        oval(d, 27, 28, 2, 2, SHADE)

    # Cheeks.
    oval(d, 20, 21 + top, 2, 2, CHEEK)
    oval(d, 32, 20 + top, 2, 2, CHEEK)

    # Eyes.
    for ex in (23, 29):
        ey = 16 + top
        if eyes == "open":
            d.rectangle([ex - 1, ey - 1, ex, ey + 1], fill=EYE)
            d.point((ex, ey - 1), fill=WHITE)
        elif eyes == "closed":
            d.line([(ex - 1, ey + 1), (ex + 1, ey + 1)], fill=EYE)
        elif eyes == "happy":
            d.point([(ex - 1, ey + 1), (ex, ey), (ex + 1, ey + 1)], fill=EYE)
        elif eyes == "x":
            d.point([(ex - 1, ey - 1), (ex + 1, ey + 1), (ex, ey), (ex + 1, ey - 1), (ex - 1, ey + 1)], fill=EYE)

    # Mouth.
    my = 20 + top
    if mouth == "smile":
        d.point([(25, my), (26, my + 1), (27, my)], fill=MOUTH)
    elif mouth == "open":
        oval(d, 26, my + 1, 2, 2, MOUTH)
        d.point([(26, my + 2), (25, my + 2), (27, my + 2)], fill=TONGUE)
    elif mouth == "wavy":
        d.point([(24, my + 1), (25, my), (26, my + 1), (27, my), (28, my + 1)], fill=MOUTH)
    elif mouth == "sleep":
        d.point([(26, my + 1)], fill=MOUTH)

    return outlined(img)


def outlined(img):
    """A one-pixel dark outline around everything opaque."""
    px = img.load()
    edge = []
    for y in range(H):
        for x in range(W):
            if px[x, y][3]:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if 0 <= nx < W and 0 <= ny < H and px[nx, ny][3]:
                    edge.append((x, y))
                    break
    for p in edge:
        px[p] = OUTLINE + (255,)
    return img


ROWS = [
    [frame(), frame(bob=1)],  # idle
    [frame(eyes="closed")],  # blink
    [frame(step=-2, tail=0), frame(step=0, bob=1, tail=1), frame(step=2, tail=0), frame(step=0, bob=1, tail=-1)],  # walk
    [frame(bob=-1, eyes="happy", mouth="open", arms="up"), frame(eyes="happy", mouth="open", arms="up", tail=1)],  # happy
    [frame(eyes="x", mouth="wavy", ears="down", tail=-1), frame(bob=1, eyes="x", mouth="wavy", ears="down", tail=1)],  # dizzy
    [frame(eyes="closed", mouth="sleep", ears="down", sleep=True), frame(bob=1, eyes="closed", mouth="sleep", ears="down", sleep=True)],  # sleep
]

sheet = Image.new("RGBA", (W * 4, H * len(ROWS)), (0, 0, 0, 0))
for r, row in enumerate(ROWS):
    for c, f in enumerate(row):
        sheet.paste(f, (c * W, r * H))
sheet.save(Path(__file__).with_name("sheet.png"))
