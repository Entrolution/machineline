#!/usr/bin/env python3
"""Render machineline's real ANSI output to assets/demo.svg (a terminal-style image for the
README). Seeds two crafted cache snapshots — a healthy machine and a throttled hot-day one — and
captures the binary's output for each. Re-run after changing the renderer:

    cargo build --release && python3 assets/gen-demo.py
"""
import json, os, re, subprocess, tempfile, html, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "machineline")

# ANSI SGR → hex (matches the shared quotaline/vastline palette on a dark terminal)
COLORS = {"0": None, "2": "#6e7681", "1": "#e6edf3", "32": "#3fb950",
          "31": "#f85149", "90": "#6e7681", "38;5;214": "#e3a23c"}
FG_DEFAULT = "#d4d4d4"

HEALTHY = dict(speed_limit=100, load1=0.5, ncpu=12, cpu_util=12.0, mem_used_pct=38.0,
               mem_pressure=1, swap_used=0.0, top_name="WindowServer", top_cpu=6.0,
               temp_c=41.0, charge_pct=78.0, on_ac=True, charging=False,
               draining_on_ac=False, adapter_w=96)
THROTTLED = dict(speed_limit=31, load1=6.5, ncpu=12, cpu_util=100.0, mem_used_pct=71.0,
                 mem_pressure=2, swap_used=2.4 * 1024**3, top_name="iTerm2", top_cpu=61.0,
                 temp_c=96.0, charge_pct=77.0, on_ac=True, charging=False,
                 draining_on_ac=True, adapter_w=96)


def capture(state):
    """Seed a state.json with `state`, render once, return the ANSI line."""
    now = time.time()
    sdir = tempfile.mkdtemp()
    cfg = tempfile.mkdtemp()  # empty → no delegated base line, just the machine line
    snap = dict(state, fetched_at=now, last_attempt=now,
                cpu_busy_ticks=None, cpu_idle_ticks=None)
    with open(os.path.join(sdir, "state.json"), "w") as f:
        json.dump(snap, f)
    env = dict(os.environ, MACHINELINE_STATE_DIR=sdir, MACHINELINE_CONFIG_DIR=cfg)
    out = subprocess.run([BIN], input="{}", capture_output=True, text=True, env=env).stdout
    return out.rstrip("\n")


def parse(line):
    """ANSI line → list of (text, color) segments."""
    segs, color, i = [], None, 0
    for m in re.finditer(r"\x1b\[([0-9;]*)m", line):
        if m.start() > i:
            segs.append((line[i:m.start()], color))
        code = m.group(1)
        color = FG_DEFAULT if code in ("", "0") else COLORS.get(code, color)
        i = m.end()
    if i < len(line):
        segs.append((line[i:], color))
    return segs


def main():
    lines = [parse(capture(HEALTHY)), parse(capture(THROTTLED))]
    cw, lh, pad = 8.4, 22, 16  # char width, line height, padding
    ncols = max(sum(len(t) for t, _ in segs) for segs in lines)
    W = round(ncols * cw + 2 * pad)
    H = round(2 * pad + len(lines) * lh)
    svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace" font-size="14">',
           f'<rect width="{W}" height="{H}" rx="8" fill="#0d1117"/>']
    for row, segs in enumerate(lines):
        y = pad + 0.72 * lh + row * lh
        parts, col = [], 0
        for text, color in segs:
            x = pad + col * cw
            fill = color or FG_DEFAULT
            parts.append(f'<tspan x="{x:.1f}" textLength="{len(text)*cw:.1f}" lengthAdjust="spacingAndGlyphs" fill="{fill}">{html.escape(text)}</tspan>')
            col += len(text)
        svg.append(f'<text y="{y}" xml:space="preserve">{"".join(parts)}</text>')
    svg.append("</svg>")
    with open(os.path.join(ROOT, "assets", "demo.svg"), "w") as f:
        f.write("\n".join(svg) + "\n")
    print(f"wrote assets/demo.svg ({W}x{H}, {ncols} cols)")


if __name__ == "__main__":
    main()
