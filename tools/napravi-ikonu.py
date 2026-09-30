"""Pravi assets/ikona.ico iz assets/ikona.svg (>= 40 px) i assets/ikona-mala.svg (<= 32 px).

SVG rasterizuje Edge u headless modu (canvas.toDataURL), pa ne trebaju Pillow ni
Inkscape. Pokreni nakon izmjene SVG-a:  python tools/napravi-ikonu.py [--pregled DIR]
"""

import base64
import json
import os
import pathlib
import re
import struct
import subprocess
import sys
import tempfile

KORIJEN = pathlib.Path(__file__).resolve().parent.parent
VELICINE = [16, 20, 24, 32, 40, 48, 64, 128, 256]
MALA_DO = 32

STRANICA = """<!doctype html><meta charset="utf-8"><body><pre id="out"></pre><script>
const velika = __VELIKA__, mala = __MALA__;
(async () => {
  const izlaz = [];
  for (const s of __VELICINE__) {
    const svg = s <= __MALA_DO__ ? mala : velika;
    const img = new Image();
    img.src = "data:image/svg+xml;base64," + btoa(unescape(encodeURIComponent(svg)));
    await img.decode();
    const c = document.createElement("canvas");
    c.width = c.height = s;
    c.getContext("2d").drawImage(img, 0, 0, s, s);
    izlaz.push("IKONA " + s + " " + c.toDataURL("image/png").split(",")[1]);
  }
  document.getElementById("out").textContent = izlaz.join("\\n");
})();
</script>"""


def edge() -> str:
    for var in ("ProgramFiles(x86)", "ProgramFiles"):
        p = pathlib.Path(os.environ.get(var, "")) / "Microsoft/Edge/Application/msedge.exe"
        if p.exists():
            return str(p)
    sys.exit("Microsoft Edge nije pronađen")


def main() -> None:
    velika = (KORIJEN / "assets/ikona.svg").read_text(encoding="utf-8")
    mala = (KORIJEN / "assets/ikona-mala.svg").read_text(encoding="utf-8")
    stranica = (
        STRANICA.replace("__VELIKA__", json.dumps(velika))
        .replace("__MALA__", json.dumps(mala))
        .replace("__VELICINE__", json.dumps(VELICINE))
        .replace("__MALA_DO__", str(MALA_DO))
    )

    with tempfile.TemporaryDirectory() as tmp:
        html = pathlib.Path(tmp) / "render.html"
        html.write_text(stranica, encoding="utf-8")
        rez = subprocess.run(
            [edge(), "--headless", "--disable-gpu", f"--user-data-dir={tmp}\\profil",
             "--virtual-time-budget=10000", "--dump-dom", html.as_uri()],
            capture_output=True, text=True, timeout=120,
        )
    slike = {int(s): base64.b64decode(b) for s, b in re.findall(r"IKONA (\d+) ([A-Za-z0-9+/=]+)", rez.stdout)}
    if sorted(slike) != VELICINE:
        sys.exit(f"Edge nije vratio sve veličine: {sorted(slike)}\n{rez.stderr[-2000:]}")

    # ICO: zaglavlje, pa po jedan 16-bajtni opis, pa PNG podaci.
    zaglavlje = struct.pack("<HHH", 0, 1, len(VELICINE))
    opisi, podaci = b"", b""
    pomak = 6 + 16 * len(VELICINE)
    for s in VELICINE:
        png = slike[s]
        opisi += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(png), pomak + len(podaci))
        podaci += png
    (KORIJEN / "assets/ikona.ico").write_bytes(zaglavlje + opisi + podaci)
    print(f"assets/ikona.ico: {len(VELICINE)} veličina, {len(zaglavlje + opisi + podaci)} bajta")

    if "--pregled" in sys.argv:
        d = pathlib.Path(sys.argv[sys.argv.index("--pregled") + 1])
        d.mkdir(parents=True, exist_ok=True)
        for s, png in slike.items():
            (d / f"ikona-{s}.png").write_bytes(png)


if __name__ == "__main__":
    main()
