# Tornado Kasa

Mali Windows program (~1 MB exe, ~2–3 MB RAM) koji prima JSON komande od web appa,
sastavlja XML i šalje ga Tring.Fiscal.Server-u (TFS). Svaka komanda se upisuje u
`tornado-kasa.log`. Ikona u system trayu otvara prozor s logom i podešavanjima.

```
browser (web app s intraneta) ──JSON──► tornado-kasa 127.0.0.1:8086 ──XML──► TFS localhost:8085
```

## Instalacija na kasi

Pokreni `TornadoKasa-Setup-X.Y.Z.exe` i prati čarobnjak. Admin prava nisu potrebna.

- Instalira se u `%LOCALAPPDATA%\Programs\Tornado Kasa`. Tu su i `config.json` i log.
- Opcija „Pokreni automatski pri prijavi na Windows” je uključena po defaultu.
- Na kraju se forwarder pokrene i otvori prozor. Tamo u **Podešavanjima** upiši
  adresu web appa i token.
- **Nadogradnja:** pokreni noviji Setup preko postojećeg. Forwarder se uredno ugasi
  i ponovo pokrene, a `config.json` i log ostaju.
- **Sa starog imena (Kasa Forwarder, do 0.1.3):** isti Setup radi i kao nadogradnja.
  Instalacija ostaje u starom folderu `Programs\Kasa Forwarder`, a stari exe, autostart
  i prečice se uklone; `kasa-forwarder.log` se preimenuje u `tornado-kasa.log`.
- **Deinstalacija:** Postavke → Aplikacije → Tornado Kasa. Pita da li da obriše i
  podešavanja i log.

Tiha instalacija na više kasa (bez pitanja, forwarder se pokrene u pozadini):
```
TornadoKasa-Setup-X.Y.Z.exe /VERYSILENT /TASKS="autostart"
```

Installer nije digitalno potpisan, pa Windows SmartScreen pri prvom pokretanju
kaže „Windows je zaštitio vaš PC”. Klikni **Više informacija → Ipak pokreni**.

### Bez Inno Setupa (build na Macu)

```sh
tools/napravi-rucni-paket.sh
```
Napravi Windows exe (cross-build, treba `rustup target add x86_64-pc-windows-gnu`
i `brew install mingw-w64`) i `dist/TornadoKasa-<verzija>-rucno.zip`. Na kasi
raspakuj zip i dvoklikni `instaliraj.cmd`. Radi isto što i Setup: ugasi forwarder,
kopira exe u `%LOCALAPPDATA%\Programs\Tornado Kasa`, napravi prečicu u Start
meniju, uključi autostart, doda stavku u Postavke → Aplikacije i otvori prozor.
Opcije: `-Tiho`, `-BezAutostarta`, `-PrecicaNaDesktopu`. Deinstalacija:
`deinstaliraj.cmd` ili preko Aplikacija. Ako je forwarder ranije instaliran
Inno Setupom, ovo samo zamijeni exe, a Inno stavka za deinstalaciju ostaje.
Skripta je `installer/rucna-instalacija.ps1`. Kad mijenjaš `.iss`, promijeni i nju.

Bez installera radi i ručno: kopiraj `tornado-kasa.exe` u neki folder i pokreni
`tornado-kasa.exe install` (odnosno `uninstall`).

### Pravljenje installera

```powershell
powershell -ExecutionPolicy Bypass -File tools\napravi-installer.ps1
```
Radi `cargo build --release` pa Inno Setup (6.5+ ili 7), a rezultat je
`dist\TornadoKasa-Setup-<verzija iz Cargo.toml>.exe`. Za novu verziju promijeni
`version` u `Cargo.toml`. Skripta installera je `installer\tornado-kasa.iss`, a
slike čarobnjaka se prave iz ikone sa `python tools\napravi-installer-slike.py`.

## Tray i prozor

- **Dvoklik** na ikonu (ili ponovno pokretanje exe-a) otvara prozor. To je Edge u
  app modu na `http://127.0.0.1:PORT/ui`. Dok je prozor zatvoren, ne troši RAM.
- **Desni klik**: otvori prozor, otvori folder, ugasi forwarder.
- **Log**: današnji brojači, pretraga, filteri (uspješno / greške / odbijeno /
  sistem), listanje po 100 zapisa, osvježavanje uživo. Klik na red prikazuje
  primljeni JSON te XML poslan Tringu i njegov odgovor.
- **Obavijest o grešci**: kad komanda iz web appa završi greškom (Tring vratio
  grešku, TFS nedostupan, ishod nepoznat), Windows pokaže obavijest. Klik na nju
  otvori prozor s tim zapisom raširenim (filter Greške + pretraga po vremenu).
  Ako je uključen „Ne ometaj” / Focus assist, obavijest ide samo u centar obavijesti.
- **Testiraj kasu** (dugme gore desno): Tringov test `GET /test`. Uređaj odštampa
  probni isječak, a forwarder javi da li je TFS vratio `OK`. Zelena tačka
  „Tring dostupan” znači samo da TFS sluša na portu; ovo provjerava i uređaj.
- **Podešavanja**: sve iz `config.json` plus autostart. Snimanje ih provjeri
  (npr. da li je port slobodan) pa restartuje forwarder.

`/ui` rute rade samo s ovog računara i samo s vlastite stranice (provjera
IP-a, `Host` i `Origin` headera), jer log sadrži podatke o računima.

## config.json

```json
{
  "listen": "127.0.0.1:8086",
  "tringUrl": "http://localhost:8085",
  "timeoutSekundi": 30,
  "allowedOrigins": ["http://192.168.1.10"],
  "token": "neki-dugi-random-string",
  "logMaxMb": 5
}
```

| Polje | Značenje |
|---|---|
| `listen` | `127.0.0.1:PORT` = samo browser na ovom računaru. `0.0.0.0:PORT` = dostupno s mreže (tad obavezno `token` i pravilo u firewallu). |
| `tringUrl` | Adresa TFS-a. |
| `allowedOrigins` | Adrese web appa (tačno kako stoje u adresnoj traci, bez putanje). `"*"` = svi. Zahtjevi bez `Origin` headera (curl, backend) se ne provjeravaju. |
| `token` | Ako nije prazan, svaki POST mora imati header `X-Kasa-Token`. |
| `logMaxMb` | Kad log pređe ovu veličinu, prebacuje se u `.log.1`. |

## API

Svi POST-ovi primaju JSON (`Content-Type: application/json`). Nepoznata polja se
odbijaju (400), da se tipfeler ne izgubi kao nekad `VrstaPlacanja`/`VrstePlacanja`.

Odgovor na komandu:
```json
{ "success": true, "vrstaOdgovora": "OK", "statusCode": 200,
  "odgovori": { "BrojFiskalnogRacuna": "42", "DatumFiskalnogRacuna": "30.09.2026", "VrijemeFiskalnogRacuna": "10:15:02" } }
```
Kod greške `success: false` i `error` (npr. `"Nedovoljno novca u kasi [535]"`).
HTTP status kaže da li je komanda stigla do TFS-a:

| HTTP | Značenje | Web app |
|---|---|---|
| 200 | TFS je odgovorio. `success` kaže da li je uspjelo, `statusCode` je HTTP status TFS-a. | `success: false` = komanda nije izvršena, smije se ispraviti i poslati ponovo. |
| 502 | Komanda **sigurno nije stigla** do TFS-a (veza odbijena, DNS, neispravan `tringUrl`). `statusCode: null`, `error` počinje s `"Tring server nije dostupan:"`. | Smije se poslati ponovo kad TFS proradi. |
| 504 | **Ishod nepoznat**: komanda je poslana, ali odgovor nije stigao ili nije čitljiv (timeout nakon `timeoutSekundi`, prekinuta veza, npr. printer bez papira koji odštampa kasnije). Tijelo ima `"ishodNepoznat": true`, `statusCode` je `null` (ili status TFS-a ako je stigao), `error` počinje s `"Komanda je možda izvršena:"`. | **Nikad ne ponavljati automatski.** Pitati osobu na kasi da li je uređaj odštampao račun/izvještaj, pa tek onda odlučiti. |

`ishodNepoznat` postoji samo u 504 odgovoru. Kod 504 se ne probaju ni rezervne
putanje na TFS-u (npr. `/un` poslije `/unosnovca`); to se radi samo kad TFS vrati 404.
Neispravan zahtjev daje 400, pogrešan token 401, nedozvoljen origin 403.

| Ruta | Tring | Tijelo |
|---|---|---|
| `GET /api/health` | – | – → `{ ok, verzija, tringUrl, tringDostupan }` |
| `POST /api/test` | GET /test | – → isto kao komanda. **Uređaj odštampa probni isječak**, zato samo na zahtjev (npr. na početku smjene), nikad u petlji. |
| `POST /api/racun` | sfr | `{ stavke, placanja, kupac?, napomena?, brojRacuna? }` |
| `POST /api/reklamacija` | srr | `{ stavke, brojRacuna, placanja?, kupac?, napomena? }`, pravila su ispod |
| `POST /api/duplikat` | stampatiduplikatfiskalnogracuna (sdfr) | `{ brojRacuna }` — duplikat fiskalnog računa (uputstvo v3.0.1, 7.8); samo uređaji s Tring USB interfejsom |
| `POST /api/presjek-stanja` | sps (X) | – |
| `POST /api/dnevni-izvjestaj` | sdi (Z) | – |
| `POST /api/periodicni-izvjestaj` | spi | `{ "od": "2026-09-01", "do": "2026-09-30" }` |
| `POST /api/unos-novca` | unosnovca | `{ iznos, oznaka? }` (default Gotovina) |
| `POST /api/povrat-novca` | povratnovca | `{ iznos, oznaka? }` |
| `POST /api/inicijalizacija` | inicijalizacija | `{ brojOperatora, lozinka }` |
| `POST /api/artikal` | ua | artikal (vidi dolje) |

Tipovi:
```ts
artikal  = { sifra, naziv, cijena, stopa: "E" | "K", jm?: "kom", grupa?: 0, plu?: 0 }
stavka   = { artikal, kolicina, rabat?: 0 }            // rabat u %
placanje = { oznaka: "Gotovina" | "Cek" | "Kartica" | "Virman", iznos }  // "Ček" se prihvata
kupac    = { idBroj, naziv, adresa?, postanskiBroj?, grad? }
```

### Vrste plaćanja

**Fiskalni račun:** jedna ili više vrsta plaćanja, npr. `[{Kartica, 90}, {Gotovina, 10}]`.
Iznos `0` znači cijeli račun tom vrstom. Zbir i prekoračenja provjerava uređaj
(greške 521, 523 i 524 se vraćaju s opisom).

**Reklamacija** (smije imati tačno jednu vrstu plaćanja):

| Povrat kupcu | `placanja` |
|---|---|
| u gotovini (uobičajeno) | izostaviti, ili `[{ "oznaka": "Gotovina", "iznos": 0 }]` |
| na virman/karticu/ček | `[{ "oznaka": "Virman", "iznos": -10.33 }]`, **negativan** iznos reklamacije |

Pozitivan iznos se odbija, jer ga uređaj tretira kao doplatu kupca pa bi vratio više novca.
Povrat na virman je dokumentovan tek u uputstvu v3.0.1 i **nije potvrđen na FP1
(TFS 3.4.522)**. Tamo je sigurniji put koji je Tring dokumentovao: `unos-novca`
Gotovina X, pa reklamacija s gotovinskim povratom. Uređaj traži da u kasi ima
dovoljno novca te vrste plaćanja (greška 535).

**Kupac:** `idBroj` tačno 13 cifara, `naziv` i `adresa` do 32 znaka, `grad` do 26,
`postanskiBroj` 5 cifara ili prazno.

Primjer iz web appa:
```js
const r = await fetch("http://127.0.0.1:8086/api/racun", {
  method: "POST",
  headers: { "Content-Type": "application/json", "X-Kasa-Token": TOKEN },
  body: JSON.stringify({
    stavke: [{ artikal: { sifra: "101", naziv: "Kafa", cijena: 2.5, stopa: "E" }, kolicina: 2 }],
    placanja: [{ oznaka: "Gotovina", iznos: 5 }],
  }),
}).then((r) => r.json());
if (!r.success) alert(r.error); // kod r.ishodNepoznat: prvo provjeriti uređaj, ne slati ponovo
```

Komande se šalju TFS-u jedna po jedna. Ako dva zahtjeva stignu istovremeno,
drugi čeka dok se prvi ne odštampa.

**Chrome:** kad stranica s mreže (npr. `http://192.168.1.10`) prvi put pošalje
zahtjev na `127.0.0.1`, Chrome može pitati korisnika za dozvolu pristupa lokalnoj
mreži. Dozvolu treba odobriti jednom po kasi.

## Log

```
2026-09-30 09:52:08.325 127.0.0.1 POST /api/racun -> OK 2517ms BrojFiskalnogRacuna=1 ... | {json komande}
2026-09-30 09:52:28.042 127.0.0.1 POST /api/presjek-stanja -> GREŠKA 4035ms <poruka> | {json} | poslano: <xml> | odgovor: <xml>
2026-09-30 09:53:10.517 127.0.0.1 POST /api/racun -> GREŠKA 30012ms [ISHOD NEPOZNAT] Komanda je možda izvršena: ... | {json} | poslano: <xml> | odgovor:
```
Lozinka operatora se u logu maskira. Kod greške se loguje i poslani i primljeni XML.
`[ISHOD NEPOZNAT]` označava 504: komanda je možda izvršena, treba pogledati uređaj.
U prozoru je takav red pod filterom „greške”.

## Razvoj

```sh
cargo test                 # testovi XML-a, parsiranja odgovora i 502/504 (lažni TFS na 127.0.0.1)
cargo build --release      # target/release/tornado-kasa.exe
```
Debug build otvara konzolni prozor, release ne. Za testiranje bez uređaja koristi se
mock iz kasa-appa: `bun run src/services/tring-mock-server.ts 8085`.

Ikona: izvor je `assets/ikona.svg` (za 40 px i više) i `assets/ikona-mala.svg`
(za 16–32 px). Nakon izmjene pokreni `python tools/napravi-ikonu.py`, koji
rasterizuje SVG preko Edgea i pravi `assets/ikona.ico`. `build.rs` ugrađuje
ikonu u exe bez windres/rc alata.
