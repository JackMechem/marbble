#!/usr/bin/env python3
"""Draws Marbble's logo: a dark glass marble with a ribbon inside shaped like one cycle of a sine
wave, a cat's-eye marble and a waveform at once. Writes src-tauri/icons/source.svg (the app icon
on its rounded square) and site/public/favicon.svg. After changing it, render source.svg to a
1024 px source.png (`rsvg-convert -w 1024 src-tauri/icons/source.svg -o src-tauri/icons/source.png`)
and run `pnpm tauri icon src-tauri/icons/source.png`, then delete the android/ and ios/ folders it
makes."""
import math
from pathlib import Path
def ribbon(cx, cy, half, amp, thick, n=120):
    """The ribbon as a closed path: a sine centre line `2 * half` wide, `thick` across."""
    up, dn = [], []
    for i in range(n+1):
        t = i/n
        x = cx - half + 2*half*t
        y = cy + amp*math.sin(2*math.pi*t)
        w = thick*(0.8+0.2*math.cos(4*math.pi*t))/2
        # offset along the normal of the curve
        dy = amp*2*math.pi*math.cos(2*math.pi*t)/(2*half)
        nx, ny = -dy, 1
        l = math.hypot(nx, ny); nx/=l; ny/=l
        up.append((x+nx*w, y-ny*w)); dn.append((x-nx*w, y+ny*w))
    pts = up + dn[::-1]
    return "M" + " L".join(f"{x:.1f} {y:.1f}" for x,y in pts) + " Z"

def svg(size=1024, bg=True):
    """The logo, on its rounded square when `bg`."""
    s = size/1024
    C, R = 512, 368
    rib = ribbon(C, C, R*1.15, R*0.40, R*0.34)
    rib2 = ribbon(C, C-R*0.035, R*1.15, R*0.40, R*0.08)
    out = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" width="{size}" height="{size}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#2c2c31"/><stop offset="1" stop-color="#19191c"/></linearGradient>
    <radialGradient id="glass" cx="0.38" cy="0.32" r="0.78"><stop offset="0" stop-color="#6a6a78"/><stop offset="0.55" stop-color="#2c2c34"/><stop offset="1" stop-color="#0d0d10"/></radialGradient>
    <linearGradient id="vane" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ffe07a"/><stop offset="0.6" stop-color="#f2c230"/><stop offset="1" stop-color="#d9930f"/></linearGradient>
    <radialGradient id="shine" cx="0.5" cy="0.5" r="0.5"><stop offset="0" stop-color="#fff" stop-opacity="0.85"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></radialGradient>
    <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="0.35"/><stop offset="0.5" stop-color="#fff" stop-opacity="0"/><stop offset="1" stop-color="#f2c230" stop-opacity="0.25"/></linearGradient>
    <radialGradient id="glow" cx="0.5" cy="1" r="0.6"><stop offset="0" stop-color="#f2c230" stop-opacity="0.32"/><stop offset="1" stop-color="#f2c230" stop-opacity="0"/></radialGradient>
    <clipPath id="ball"><circle cx="{C}" cy="{C}" r="{R}"/></clipPath>
  </defs>
'''
    if bg:
        out += '  <rect width="1024" height="1024" rx="228" fill="url(#bg)"/>\n'
        out += f'  <ellipse cx="{C}" cy="{C+R+22}" rx="{R*0.8:.0f}" ry="26" fill="#000" opacity="0.35"/>\n'
    out += f'''  <circle cx="{C}" cy="{C}" r="{R}" fill="url(#glass)"/>
  <g clip-path="url(#ball)">
    <rect x="0" y="{C}" width="1024" height="{R}" fill="url(#glow)"/>
    <path d="{rib}" fill="url(#vane)"/>
    <path d="{rib2}" fill="#fff6d6" opacity="0.55"/>
  </g>
  <circle cx="{C}" cy="{C}" r="{R-5}" fill="none" stroke="url(#rim)" stroke-width="10"/>
  <ellipse cx="{C-130}" cy="{C-190}" rx="125" ry="70" transform="rotate(-32 {C-130} {C-190})" fill="url(#shine)"/>
</svg>
'''
    return out
root = Path(__file__).resolve().parent.parent
(root / "src-tauri/icons/source.svg").write_text(svg())
(root / "site/public/favicon.svg").write_text(svg())
