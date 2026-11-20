#!/usr/bin/env python3
"""Regenerate assets/gotop-blackbox.ico (a pink flower). Needs Pillow: python3 assets/make_icon.py"""
import math
from pathlib import Path
from PIL import Image, ImageDraw

S = 1024  # supersampled canvas
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
c = S / 2

# soft round background so the icon reads on dark and light taskbars
d.ellipse([S * 0.04, S * 0.04, S * 0.96, S * 0.96], fill=(255, 240, 246, 255))

# 8 petals, two tones
n, r_off, r_pet = 8, S * 0.25, S * 0.17
for i in range(n):
    a = 2 * math.pi * i / n
    x, y = c + r_off * math.cos(a), c + r_off * math.sin(a)
    col = (224, 82, 156, 255) if i % 2 == 0 else (242, 123, 181, 255)
    d.ellipse([x - r_pet, y - r_pet, x + r_pet, y + r_pet], fill=col)

# centre
r_c = S * 0.13
d.ellipse([c - r_c, c - r_c, c + r_c, c + r_c], fill=(255, 201, 51, 255))
d.ellipse([c - r_c * 0.45, c - r_c * 0.45, c + r_c * 0.45, c + r_c * 0.45], fill=(240, 160, 20, 255))

out = Path(__file__).with_name("gotop-blackbox.ico")
sizes = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]
img.resize((256, 256), Image.LANCZOS).save(out, format="ICO", sizes=sizes)
print("wrote", out, out.stat().st_size, "bytes")
