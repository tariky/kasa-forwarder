"""Pravi slike za čarobnjak installera (installer/wizard-*.bmp) iz assets/ikona.svg.

Crta Edge u headless modu (canvas), a Python piše 24-bitne BMP-ove — Inno Setup
ih prima bez dodatnih alata. Pokreni nakon izmjene ikone:
    python tools/napravi-installer-slike.py
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

# (širina, visina) za 100 %, 150 % i 200 % DPI.
VELIKI = [(164, 314), (246, 471), (328, 628)]
MALI = [(55, 55), (83, 83), (110, 110)]

STRANICA = """<!doctype html><meta charset="utf-8"><body><pre id="out"></pre><script>
const ikona = __IKONA__;
const velicine = __VELICINE__;
(async () => {
  const img = new Image();
  img.src = "data:image/svg+xml;base64," + btoa(unescape(encodeURIComponent(ikona)));
  await img.decode();
  const izlaz = [];
  for (const [vrsta, w, h] of velicine) {
    const c = document.createElement("canvas");
    c.width = w; c.height = h;
    const g = c.getContext("2d");
    const s = w / (vrsta === "veliki" ? 164 : 55);
    if (vrsta === "veliki") {
      const bg = g.createLinearGradient(0, 0, w * 0.35, h);
      bg.addColorStop(0, "#2dd4bf"); bg.addColorStop(0.5, "#0d9488"); bg.addColorStop(1, "#134e4a");
      g.fillStyle = bg; g.fillRect(0, 0, w, h);
      // blagi krugovi u pozadini
      g.fillStyle = "rgba(255,255,255,0.07)";
      g.beginPath(); g.arc(w * 0.95, h * 0.1, 70 * s, 0, 7); g.fill();
      g.beginPath(); g.arc(w * 0.05, h * 0.92, 90 * s, 0, 7); g.fill();
      const iv = 92 * s;
      g.shadowColor = "rgba(4,47,46,0.45)"; g.shadowBlur = 14 * s; g.shadowOffsetY = 5 * s;
      g.drawImage(img, (w - iv) / 2, 64 * s, iv, iv);
      g.shadowColor = "transparent";
      g.fillStyle = "#ffffff"; g.textAlign = "center";
      g.font = `600 ${21 * s}px "Segoe UI Variable Display", "Segoe UI", sans-serif`;
      g.fillText("Kasa", w / 2, 196 * s);
      g.fillText("Forwarder", w / 2, 222 * s);
      g.fillStyle = "rgba(255,255,255,0.78)";
      g.font = `${10.5 * s}px "Segoe UI", sans-serif`;
      g.fillText("most do Tring fiskalnog", w / 2, 250 * s);
      g.fillText("uređaja", w / 2, 264 * s);
    } else {
      g.fillStyle = "#ffffff"; g.fillRect(0, 0, w, h);
      g.drawImage(img, 0, 0, w, h);
    }
    const px = g.getImageData(0, 0, w, h).data;
    let bin = "";
    for (let i = 0; i < px.length; i += 8192) bin += String.fromCharCode.apply(null, px.subarray(i, i + 8192));
    izlaz.push("SLIKA " + vrsta + " " + w + " " + h + " " + btoa(bin));
  }
  document.getElementById("out").textContent = izlaz.join("\\n");
})();
</script>"""


def edge_putanja() -> str:
    for var in ("ProgramFiles(x86)", "ProgramFiles"):
        p = pathlib.Path(os.environ.get(var, "")) / "Microsoft/Edge/Application/msedge.exe"
        if p.exists():
            return str(p)
    sys.exit("Microsoft Edge nije pronađen")


def bmp(w: int, h: int, rgba: bytes) -> bytes:
    """24-bitni BMP, redovi odozdo prema gore, poravnati na 4 bajta."""
    red = (w * 3 + 3) & ~3
    podaci = bytearray()
    for y in range(h - 1, -1, -1):
        linija = bytearray()
        for x in range(w):
            r, g, b, _ = rgba[(y * w + x) * 4:(y * w + x) * 4 + 4]
            linija += bytes((b, g, r))
        podaci += linija + b"\0" * (red - w * 3)
    zaglavlje = struct.pack("<2sIHHI", b"BM", 54 + len(podaci), 0, 0, 54)
    info = struct.pack("<IiiHHIIiiII", 40, w, h, 1, 24, 0, len(podaci), 2835, 2835, 0, 0)
    return zaglavlje + info + bytes(podaci)


def main() -> None:
    ikona = (KORIJEN / "assets/ikona.svg").read_text(encoding="utf-8")
    velicine = [["veliki", w, h] for w, h in VELIKI] + [["mali", w, h] for w, h in MALI]
    stranica = STRANICA.replace("__IKONA__", json.dumps(ikona)).replace("__VELICINE__", json.dumps(velicine))

    with tempfile.TemporaryDirectory() as tmp:
        html = pathlib.Path(tmp) / "render.html"
        html.write_text(stranica, encoding="utf-8")
        rez = subprocess.run(
            [edge_putanja(), "--headless", "--disable-gpu", f"--user-data-dir={tmp}\\profil",
             "--virtual-time-budget=10000", "--dump-dom", html.as_uri()],
            capture_output=True, text=True, timeout=120,
        )
    slike = re.findall(r"SLIKA (\w+) (\d+) (\d+) ([A-Za-z0-9+/=]+)", rez.stdout)
    if len(slike) != len(velicine):
        sys.exit(f"Edge je vratio {len(slike)} od {len(velicine)} slika\n{rez.stderr[-2000:]}")

    izlaz = KORIJEN / "installer"
    for vrsta, w, h, b64 in slike:
        ime = izlaz / f"wizard-{vrsta}-{w}.bmp"
        ime.write_bytes(bmp(int(w), int(h), base64.b64decode(b64)))
        print(ime.relative_to(KORIJEN))


if __name__ == "__main__":
    main()
