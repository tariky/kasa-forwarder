// Sastavljanje XML zahtjeva za Tring.Fiscal.Server (TFS) i parsiranje odgovora.
// Preneseno iz kasa-app/src/services/tring.ts — oblik XML-a mora ostati isti,
// jer TFS nepoznate elemente tiho ignoriše umjesto da vrati grešku.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const XML_DECL: &str = r#"<?xml version="1.0" encoding="utf-8"?>"#;
const XMLNS: &str = r#"xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:xsd="http://www.w3.org/2001/XMLSchema""#;

static BROJ_ZAHTJEVA: AtomicU64 = AtomicU64::new(0);

fn sljedeci_broj() -> u64 {
    BROJ_ZAHTJEVA.fetch_add(1, Ordering::Relaxed) + 1
}

// ---------------------------------------------------------------------------
// JSON ulaz
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Stopa {
    E,
    K,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artikal {
    pub sifra: String,
    pub naziv: String,
    #[serde(default = "default_jm")]
    pub jm: String,
    pub cijena: f64,
    pub stopa: Stopa,
    #[serde(default)]
    pub grupa: u32,
    #[serde(default)]
    pub plu: u32,
}

fn default_jm() -> String {
    "kom".into()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stavka {
    pub artikal: Artikal,
    pub kolicina: f64,
    #[serde(default)]
    pub rabat: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Placanje {
    pub oznaka: String,
    pub iznos: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Kupac {
    /// JIB, 13 cifara.
    pub id_broj: String,
    pub naziv: String,
    #[serde(default)]
    pub adresa: String,
    #[serde(default)]
    pub postanski_broj: String,
    #[serde(default)]
    pub grad: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Racun {
    pub stavke: Vec<Stavka>,
    pub placanja: Vec<Placanje>,
    pub kupac: Option<Kupac>,
    pub napomena: Option<String>,
    pub broj_racuna: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reklamacija {
    pub stavke: Vec<Stavka>,
    #[serde(default)]
    pub placanja: Vec<Placanje>,
    pub kupac: Option<Kupac>,
    pub napomena: Option<String>,
    /// Broj originalnog fiskalnog računa.
    pub broj_racuna: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Inicijalizacija {
    pub broj_operatora: u32,
    pub lozinka: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Novac {
    pub iznos: f64,
    pub oznaka: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Period {
    /// YYYY-MM-DD
    pub od: String,
    /// YYYY-MM-DD
    #[serde(rename = "do")]
    pub do_: String,
}

// ---------------------------------------------------------------------------
// Komanda = gotov XML + HTTP putanja(e) na TFS-u
// ---------------------------------------------------------------------------

pub struct Komanda {
    /// Prva putanja je primarna; ostale se probaju samo ako TFS vrati 404.
    pub putanje: &'static [&'static str],
    pub xml: String,
}

pub fn inicijalizacija(z: &Inicijalizacija) -> Result<Komanda, String> {
    Ok(Komanda {
        putanje: &["/inicijalizacija"],
        xml: format!(
            "{XML_DECL}<Operator {XMLNS}><BrojOperatora>{}</BrojOperatora><Lozinka>{}</Lozinka></Operator>",
            z.broj_operatora,
            esc(&z.lozinka)
        ),
    })
}

// VrstaZahtjeva=105
pub fn upisi_artikal(a: &Artikal) -> Result<Komanda, String> {
    provjeri_artikal(a)?;
    Ok(Komanda {
        putanje: &["/ua"],
        xml: format!(
            "{XML_DECL}<RacunZahtjev {XMLNS}><BrojZahtjeva>{}</BrojZahtjeva><VrstaZahtjeva>105</VrstaZahtjeva><NoviObjekat>{}</NoviObjekat></RacunZahtjev>",
            sljedeci_broj(),
            artikal_xml(a)
        ),
    })
}

// VrstaZahtjeva=0
pub fn fiskalni_racun(r: &Racun) -> Result<Komanda, String> {
    provjeri_stavke(&r.stavke)?;
    if let Some(k) = &r.kupac {
        provjeri_kupca(k)?;
    }
    if r.placanja.is_empty() {
        return Err("placanja: potrebna je bar jedna vrsta plaćanja (iznos 0 = cijeli račun tom vrstom)".into());
    }
    let placanja = placanja_xml(&r.placanja)?;
    Ok(Komanda {
        putanje: &["/sfr"],
        xml: racun_xml(0, r.kupac.as_ref(), &r.stavke, &placanja, r.napomena.as_deref(), r.broj_racuna.unwrap_or(0)),
    })
}

// VrstaZahtjeva=2
pub fn reklamirani_racun(r: &Reklamacija) -> Result<Komanda, String> {
    provjeri_stavke(&r.stavke)?;
    if r.broj_racuna == 0 {
        return Err("brojRacuna: broj fiskalnog računa koji se reklamira je obavezan".into());
    }
    if let Some(k) = &r.kupac {
        provjeri_kupca(k)?;
    }
    // Reklamacija mora nositi tačno jednu vrstu plaćanja (inače TFS greška 573):
    //  - povrat u gotovini: Gotovina s iznosom 0 (i default kad placanja nisu poslana)
    //  - povrat drugim sredstvom: ta oznaka s NEGATIVNIM iznosom reklamacije
    //    (uputstvo v3.0.1, 7.4 c); na FP1 / TFS 3.4.522 nije potvrđeno da radi)
    //  - pozitivan iznos NIJE povrat nego doplata kupca — uređaj bi vratio više
    //    novca nego što treba, pa se odbija.
    let placanja = match r.placanja.as_slice() {
        [] => vec![("Gotovina", 0.0)],
        [p] => {
            let o = oznaka(&p.oznaka)?;
            if o == "Gotovina" && p.iznos != 0.0 {
                return Err("placanja: povrat u gotovini se šalje kao Gotovina s iznosom 0 (uređaj sam računa iznos iz stavki)".into());
            }
            if o != "Gotovina" && !(p.iznos < 0.0) {
                return Err(format!(
                    "placanja: povrat na {o} se šalje s negativnim iznosom reklamacije (npr. -10.33); pozitivan iznos uređaj tretira kao doplatu kupca"
                ));
            }
            vec![(o, p.iznos)]
        }
        _ => return Err("placanja: reklamacija smije imati samo jednu vrstu plaćanja (TFS greška 573)".into()),
    };
    let placanja: String = placanja
        .into_iter()
        .map(|(o, iznos)| format!("<VrstaPlacanja><Oznaka>{o}</Oznaka><Iznos>{}</Iznos></VrstaPlacanja>", broj(iznos, 2)))
        .collect();
    Ok(Komanda {
        putanje: &["/srr"],
        xml: racun_xml(2, r.kupac.as_ref(), &r.stavke, &placanja, r.napomena.as_deref(), r.broj_racuna),
    })
}

// VrstaZahtjeva=3 (X izvještaj)
pub fn presjek_stanja() -> Komanda {
    Komanda { putanje: &["/sps"], xml: zahtjev_xml(3, "<Parametri />") }
}

// VrstaZahtjeva=4 (Z izvještaj)
pub fn dnevni_izvjestaj() -> Komanda {
    Komanda { putanje: &["/sdi"], xml: zahtjev_xml(4, "<Parametri />") }
}

// VrstaZahtjeva=5
pub fn periodicni_izvjestaj(p: &Period) -> Result<Komanda, String> {
    let od = tring_datum(&p.od).ok_or("od: datum mora biti u obliku YYYY-MM-DD")?;
    let do_ = tring_datum(&p.do_).ok_or("do: datum mora biti u obliku YYYY-MM-DD")?;
    // Uređaj za obrnut opseg nema dokumentovanu grešku — provjerava se ovdje.
    // YYYY-MM-DD se leksikografski poredi kao datum.
    if p.od.trim() > p.do_.trim() {
        return Err("od: datum \"od\" je poslije datuma \"do\"".into());
    }
    let parametri = format!(
        "<Parametri><Parametar><Naziv>odDatuma</Naziv><Vrijednost>{od} 00:00:00</Vrijednost></Parametar><Parametar><Naziv>doDatuma</Naziv><Vrijednost>{do_} 23:59:59</Vrijednost></Parametar></Parametri>"
    );
    Ok(Komanda { putanje: &["/spi"], xml: zahtjev_xml(5, &parametri) })
}

// UnosNovca je VrstaZahtjeva 7, PovratNovca 8 (isporučeni povratnovca.xml kaže 8,
// iako tekst uputstva za obje piše 7). Kratki path je rezerva ako TFS puni ne zna.
pub fn unos_novca(n: &Novac) -> Result<Komanda, String> {
    Ok(Komanda { putanje: &["/unosnovca", "/un"], xml: novac_xml(7, n)? })
}

pub fn povrat_novca(n: &Novac) -> Result<Komanda, String> {
    Ok(Komanda { putanje: &["/povratnovca", "/pn"], xml: novac_xml(8, n)? })
}

// ---------------------------------------------------------------------------
// XML pomoćne funkcije
// ---------------------------------------------------------------------------

fn zahtjev_xml(vrsta: u32, parametri: &str) -> String {
    format!(
        "{XML_DECL}<Zahtjev {XMLNS}><BrojZahtjeva>{}</BrojZahtjeva><VrstaZahtjeva>{vrsta}</VrstaZahtjeva>{parametri}</Zahtjev>",
        sljedeci_broj()
    )
}

fn racun_xml(vrsta: u32, kupac: Option<&Kupac>, stavke: &[Stavka], placanja: &str, napomena: Option<&str>, broj_racuna: u64) -> String {
    let mut s = format!(
        "{XML_DECL}<RacunZahtjev {XMLNS}><BrojZahtjeva>{}</BrojZahtjeva><VrstaZahtjeva>{vrsta}</VrstaZahtjeva><NoviObjekat>",
        sljedeci_broj()
    );
    if let Some(k) = kupac {
        s += &format!(
            "<Kupac><IDbroj>{}</IDbroj><Naziv>{}</Naziv><Adresa>{}</Adresa><PostanskiBroj>{}</PostanskiBroj><Grad>{}</Grad></Kupac>",
            esc(&k.id_broj), esc(&k.naziv), esc(&k.adresa), esc(&k.postanski_broj), esc(&k.grad)
        );
    }
    s += "<StavkeRacuna>";
    for st in stavke {
        s += &format!(
            "<RacunStavka><artikal>{}</artikal><Kolicina>{}</Kolicina><Rabat>{}</Rabat></RacunStavka>",
            artikal_xml(&st.artikal),
            broj(st.kolicina, 3),
            broj(st.rabat, 2)
        );
    }
    // Omotač je VrstePlacanja (množina) — ime iz stampatifiskalniracun.xsd.
    // Sa VrstaPlacanja TFS svaki račun knjiži kao gotovinski.
    s += &format!(
        "</StavkeRacuna><VrstePlacanja>{placanja}</VrstePlacanja><Napomena>{}</Napomena><BrojRacuna>{broj_racuna}</BrojRacuna></NoviObjekat></RacunZahtjev>",
        esc(napomena.unwrap_or(""))
    );
    s
}

fn artikal_xml(a: &Artikal) -> String {
    let stopa = match a.stopa {
        Stopa::E => "E",
        Stopa::K => "K",
    };
    format!(
        "<Sifra>{}</Sifra><Naziv>{}</Naziv><JM>{}</JM><Cijena>{}</Cijena><Stopa>{stopa}</Stopa><Grupa>{}</Grupa><PLU>{}</PLU>",
        esc(&a.sifra), esc(&a.naziv), esc(&a.jm), broj(a.cijena, 2), a.grupa, a.plu
    )
}

fn placanja_xml(placanja: &[Placanje]) -> Result<String, String> {
    let mut s = String::new();
    for p in placanja {
        if !(p.iznos >= 0.0) {
            return Err(format!("placanja: iznos ne može biti negativan ({})", p.iznos));
        }
        s += &format!(
            "<VrstaPlacanja><Oznaka>{}</Oznaka><Iznos>{}</Iznos></VrstaPlacanja>",
            oznaka(&p.oznaka)?,
            broj(p.iznos, 2)
        );
    }
    Ok(s)
}

fn novac_xml(vrsta: u32, n: &Novac) -> Result<String, String> {
    if !(n.iznos > 0.0) {
        return Err("iznos: mora biti veći od 0".into());
    }
    let oznaka = oznaka(n.oznaka.as_deref().unwrap_or("Gotovina"))?;
    Ok(format!(
        "{XML_DECL}<RacunZahtjev {XMLNS}><BrojZahtjeva>{}</BrojZahtjeva><VrstaZahtjeva>{vrsta}</VrstaZahtjeva><NoviObjekat><Oznaka>{oznaka}</Oznaka><Iznos>{}</Iznos></NoviObjekat></RacunZahtjev>",
        sljedeci_broj(),
        broj(n.iznos, 2)
    ))
}

/// TFS prima samo Gotovina|Cek|Kartica|Virman, case-sensitive i bez kvačice
/// (vrstaplacanja.xsd). Nepoznata oznaka je greška — tiho spuštanje na
/// Gotovinu bi pogrešno knjižilo račun.
fn oznaka(o: &str) -> Result<&'static str, String> {
    match o.trim().to_lowercase().as_str() {
        "gotovina" => Ok("Gotovina"),
        "kartica" => Ok("Kartica"),
        "virman" => Ok("Virman"),
        "cek" | "ček" => Ok("Cek"),
        _ => Err(format!("oznaka: nepoznata vrsta plaćanja \"{o}\" (Gotovina, Cek, Kartica, Virman)")),
    }
}

fn provjeri_artikal(a: &Artikal) -> Result<(), String> {
    if a.sifra.trim().is_empty() || a.naziv.trim().is_empty() {
        return Err("artikal: sifra i naziv su obavezni".into());
    }
    if !(a.cijena >= 0.0) {
        return Err(format!("artikal {}: cijena ne može biti negativna", a.sifra));
    }
    Ok(())
}

/// Ograničenja polja kupca iz Tring specifikacije (kasa-app tring.ts).
fn provjeri_kupca(k: &Kupac) -> Result<(), String> {
    let id = k.id_broj.trim();
    if id.len() != 13 || !id.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("kupac.idBroj: JIB mora imati tačno 13 cifara (\"{id}\")"));
    }
    if k.naziv.trim().is_empty() {
        return Err("kupac.naziv: naziv kupca je obavezan".into());
    }
    for (polje, vrijednost, max) in [("naziv", &k.naziv, 32), ("adresa", &k.adresa, 32), ("grad", &k.grad, 26)] {
        if vrijednost.chars().count() > max {
            return Err(format!("kupac.{polje}: najviše {max} znakova (poslano {})", vrijednost.chars().count()));
        }
    }
    let pb = k.postanski_broj.trim();
    if !pb.is_empty() && (pb.len() != 5 || !pb.bytes().all(|c| c.is_ascii_digit())) {
        return Err(format!("kupac.postanskiBroj: mora imati 5 cifara (\"{pb}\")"));
    }
    Ok(())
}

fn provjeri_stavke(stavke: &[Stavka]) -> Result<(), String> {
    if stavke.is_empty() {
        return Err("stavke: račun mora imati bar jednu stavku".into());
    }
    for s in stavke {
        provjeri_artikal(&s.artikal)?;
        if !(s.kolicina > 0.0) {
            return Err(format!("artikal {}: kolicina mora biti veća od 0", s.artikal.sifra));
        }
        if !(0.0..=100.0).contains(&s.rabat) {
            return Err(format!("artikal {}: rabat mora biti između 0 i 100", s.artikal.sifra));
        }
    }
    Ok(())
}

/// Broj bez suvišnih decimala, kao JS `${n}` (100 -> "100", 12.5 -> "12.5"),
/// ali zaokružen da ne ode 0.30000000000000004.
fn broj(x: f64, decimala: i32) -> String {
    let f = 10f64.powi(decimala);
    let r = ((x + f64::EPSILON) * f).round() / f;
    if r == 0.0 { "0".into() } else { r.to_string() }
}

/// YYYY-MM-DD -> d.M.yyyy (format koji TFS očekuje).
fn tring_datum(s: &str) -> Option<String> {
    let mut dijelovi = s.trim().split('-');
    let g: u32 = dijelovi.next()?.parse().ok()?;
    let m: u32 = dijelovi.next()?.parse().ok()?;
    let d: u32 = dijelovi.next()?.parse().ok()?;
    if dijelovi.next().is_some() || !(1000..=9999).contains(&g) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(format!("{d}.{m}.{g}"))
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o += "&amp;",
            '<' => o += "&lt;",
            '>' => o += "&gt;",
            '"' => o += "&quot;",
            '\'' => o += "&apos;",
            _ => o.push(c),
        }
    }
    o
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

// ---------------------------------------------------------------------------
// Slanje i odgovor
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TringOdgovor {
    pub success: bool,
    pub vrsta_odgovora: String,
    pub odgovori: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// HTTP status TFS-a; null kad veza nije ni uspostavljena ili odgovor nije stigao.
    pub status_code: Option<u16>,
    /// Komanda je poslana, ali se ne zna da li je izvršena (timeout, prekinuta veza,
    /// nečitljiv odgovor). Web app tad ne smije ponoviti komandu sam — mogao bi
    /// nastati drugi fiskalni račun.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ishod_nepoznat: bool,
    /// Sirovi XML odgovor, samo za log.
    #[serde(skip)]
    pub sirovi: String,
}

/// Šta se zna o komandi kad TFS nije vratio čitljiv odgovor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isporuka {
    /// Veza nije ni uspostavljena — komanda sigurno nije stigla do TFS-a.
    NijePoslana,
    /// Komanda je možda stigla i izvršena (npr. printer bez papira odštampa kasnije).
    Nepoznato,
}

/// Svaka vrsta greške je razvrstana ručno (bez `_ =>`), da nova vrsta u ureq-u
/// ne završi tiho kao "nije poslano". Kad nije dokazivo da se greška desila prije
/// slanja, ishod je nepoznat: pogrešan 504 košta pogled na uređaj, pogrešan 502
/// dupli fiskalni račun.
pub fn isporuka(vrsta: ureq::ErrorKind) -> Isporuka {
    use ureq::ErrorKind as K;
    match vrsta {
        // Sve prije TCP konekcije na TFS (redirekti su isključeni, pa ove vrste
        // ne mogu doći s nekog drugog URL-a nakon što je TFS već odgovorio).
        K::InvalidUrl | K::UnknownScheme | K::InsecureRequestHttpsOnly | K::InvalidProxyUrl | K::Dns => Isporuka::NijePoslana,
        // TCP connect (ili connect_timeout) nije uspio — ništa nije poslano.
        K::ConnectionFailed => Isporuka::NijePoslana,
        // Proxy je odbio CONNECT tunel, prije nego što je zahtjev napisan.
        K::ProxyConnect | K::ProxyUnauthorized => Isporuka::NijePoslana,
        // Zahtjev je napisan (ili se ne zna): timeout čitanja, prekinuta veza,
        // nečitljiv status/header odgovora, redirekt.
        K::Io | K::BadStatus | K::BadHeader | K::TooManyRedirects | K::HTTP => Isporuka::Nepoznato,
    }
}

const PROVJERI_UREDJAJ: &str = "Provjeri na fiskalnom uređaju da li je odštampana prije ponovnog slanja.";

fn neuspjeh(error: String, status_code: Option<u16>, ishod_nepoznat: bool, sirovi: String) -> TringOdgovor {
    TringOdgovor {
        success: false,
        vrsta_odgovora: "Greska".into(),
        odgovori: BTreeMap::new(),
        error: Some(error),
        status_code,
        ishod_nepoznat,
        sirovi,
    }
}

/// TFS je odgovorio — tijelo se mora pročitati do kraja, inače se ne zna šta je uradio.
fn procitaj(status: u16, r: ureq::Response) -> TringOdgovor {
    match r.into_string() {
        Ok(xml) => parsiraj(&xml, Some(status)),
        Err(e) => neuspjeh(
            format!("Komanda je možda izvršena: odgovor Tring servera (HTTP {status}) nije moguće pročitati ({e}). {PROVJERI_UREDJAJ}"),
            Some(status),
            true,
            String::new(),
        ),
    }
}

/// Šalje komandu na TFS. Nikad ne vraća Err — greška veze je neuspješan odgovor.
pub fn posalji(tring_url: &str, timeout: Duration, k: &Komanda) -> TringOdgovor {
    // Bez redirekata: POST bi se pretvorio u GET na drugi URL, a greška tamo bi
    // izgledala kao da TFS nije ni dobio komandu.
    let agent = ureq::AgentBuilder::new().timeout(timeout).redirects(0).build();
    let mut zadnji = None;
    for putanja in k.putanje {
        let url = format!("{}{}", tring_url.trim_end_matches('/'), putanja);
        let rezultat = agent.post(&url).set("Content-Type", "text/xml").send_string(&k.xml);
        let odgovor = match rezultat {
            Ok(r) => procitaj(r.status(), r),
            Err(ureq::Error::Status(status, r)) => procitaj(status, r),
            Err(ureq::Error::Transport(t)) => match isporuka(t.kind()) {
                Isporuka::NijePoslana => neuspjeh(format!("Tring server nije dostupan: {t}"), None, false, String::new()),
                Isporuka::Nepoznato => neuspjeh(
                    format!("Komanda je možda izvršena: Tring server nije odgovorio ({t}). {PROVJERI_UREDJAJ}"),
                    None,
                    true,
                    String::new(),
                ),
            },
        };
        // 404 znači "TFS ne zna ovaj path, probaj rezervni". Nakon nepoznatog
        // ishoda se nikad ne proba druga putanja.
        if odgovor.success || odgovor.ishod_nepoznat || odgovor.status_code != Some(404) {
            return odgovor;
        }
        zadnji = Some(odgovor);
    }
    zadnji.expect("komanda ima bar jednu putanju")
}

/// Kodovi grešaka TFS-a na koje korisnik može smisleno reagovati.
fn poznata_greska(broj: &str) -> Option<&'static str> {
    Some(match broj {
        "516" => "Prekoračenje broja stavki računa ili reklamacije",
        "517" => "Prekoračenje u iznosu reklamacije",
        "518" => "Ne postoji artikal za reklamaciju",
        "521" => "Prekoračenje iznosa plaćanja",
        "522" => "Pogrešna vrsta plaćanja ili nedozvoljen režim",
        "523" => "Plaćanje karticom ili čekom veće od iznosa računa",
        "524" => "Ukupna suma plaćanja veća od sume računa",
        "535" => "Nedovoljno novca u kasi (za tu vrstu plaćanja) — napravi unos novca pa ponovi",
        "573" => "Reklamacija zahtijeva jednu vrstu plaćanja s iznosom 0",
        "412" => "Potrebno je napraviti dnevni izvještaj",
        "537" => "Vrijeme je manje od vremena zadnjeg fiskalnog dokumenta",
        "1707" => "Pogrešan format početnog datuma",
        "1708" => "Nedostaje početni datum",
        "1709" => "Pogrešan format krajnjeg datuma",
        "1710" => "Nedostaje krajnji datum",
        _ => return None,
    })
}

fn tag<'a>(xml: &'a str, ime: &str) -> Option<&'a str> {
    let otvori = format!("<{ime}>");
    let zatvori = format!("</{ime}>");
    let pocetak = xml.find(&otvori)? + otvori.len();
    let kraj = xml[pocetak..].find(&zatvori)? + pocetak;
    Some(&xml[pocetak..kraj])
}

/// Vrijednost može imati xsi:type atribut ili biti samozatvarajuća (prazna).
fn vrijednost(blok: &str) -> Option<String> {
    let pocetak = blok.find("<Vrijednost")?;
    let kraj_taga = blok[pocetak..].find('>')? + pocetak;
    if blok[..kraj_taga].ends_with('/') {
        return Some(String::new());
    }
    let sadrzaj = &blok[kraj_taga + 1..];
    let kraj = sadrzaj.find("</Vrijednost>")?;
    Some(unesc(&sadrzaj[..kraj]))
}

pub fn parsiraj(xml: &str, status_code: Option<u16>) -> TringOdgovor {
    let vrsta_odgovora = tag(xml, "VrstaOdgovora").unwrap_or("Greska").trim().to_string();

    let mut odgovori = BTreeMap::new();
    let mut ostatak = xml;
    while let Some(p) = ostatak.find("<Odgovor>") {
        let od = &ostatak[p + "<Odgovor>".len()..];
        let Some(k) = od.find("</Odgovor>") else { break };
        let blok = &od[..k];
        if let (Some(naziv), Some(v)) = (tag(blok, "Naziv"), vrijednost(blok)) {
            odgovori.insert(unesc(naziv.trim()), v);
        }
        ostatak = &od[k..];
    }

    // Greške stižu kao <Greska><Broj/><Opis/></Greska> (greska.xsd).
    let broj = tag(xml, "Broj").map(str::trim).filter(|b| !b.is_empty() && b.bytes().all(|c| c.is_ascii_digit()));
    let opis = tag(xml, "Opis").map(|o| unesc(o.trim())).filter(|o| !o.is_empty());
    let mut error = broj.map(|b| {
        let mut dijelovi = Vec::new();
        match (poznata_greska(b), &opis) {
            (Some(p), Some(o)) if o != p => {
                dijelovi.push(p.to_string());
                dijelovi.push(format!("({o})"));
            }
            (Some(p), _) => dijelovi.push(p.to_string()),
            (None, Some(o)) => dijelovi.push(o.clone()),
            (None, None) => {}
        }
        dijelovi.push(format!("[{b}]"));
        dijelovi.join(" ")
    });

    let success = vrsta_odgovora == "OK";
    // HTTP 2xx bez VrstaOdgovora i bez broja greške (prazno ili odsječeno tijelo):
    // TFS nije rekao ni da je uspjelo ni da nije.
    let ishod_nepoznat = !success
        && broj.is_none()
        && tag(xml, "VrstaOdgovora").is_none()
        && status_code.is_some_and(|s| (200..300).contains(&s));
    if ishod_nepoznat {
        error = Some(format!(
            "Komanda je možda izvršena: Tring server je vratio HTTP {} bez prepoznatljivog odgovora. {PROVJERI_UREDJAJ}",
            status_code.unwrap_or_default()
        ));
    } else if !success && error.is_none() {
        error = Some(match status_code {
            Some(s) if !(200..300).contains(&s) => format!("Tring server vratio {vrsta_odgovora} (HTTP {s})"),
            _ => format!("Tring server vratio {vrsta_odgovora}"),
        });
    }

    TringOdgovor { success, vrsta_odgovora, odgovori, error, status_code, ishod_nepoznat, sirovi: xml.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn racun(json: &str) -> Racun {
        serde_json::from_str(json).unwrap()
    }

    const STAVKE: &str = r#"[{"artikal":{"sifra":"p1","naziv":"p1","cijena":100,"stopa":"E"},"kolicina":1}]"#;

    #[test]
    fn racun_salje_vrste_placanja_mnozina_i_cek_bez_kvacice() {
        for (ulaz, izlaz) in [("Gotovina", "Gotovina"), ("Kartica", "Kartica"), ("Virman", "Virman"), ("Ček", "Cek")] {
            let r = racun(&format!(r#"{{"stavke":{STAVKE},"placanja":[{{"oznaka":"{ulaz}","iznos":100}}]}}"#));
            let xml = fiskalni_racun(&r).unwrap().xml;
            assert!(xml.contains(&format!(
                "<VrstePlacanja><VrstaPlacanja><Oznaka>{izlaz}</Oznaka><Iznos>100</Iznos></VrstaPlacanja></VrstePlacanja>"
            )), "{xml}");
            assert!(!xml.contains("<VrstaPlacanja><VrstaPlacanja>"));
        }
    }

    #[test]
    fn racun_ima_isti_oblik_kao_kasa_app() {
        let r = racun(&format!(r#"{{"stavke":{STAVKE},"placanja":[{{"oznaka":"Gotovina","iznos":100}}]}}"#));
        let xml = fiskalni_racun(&r).unwrap().xml;
        assert!(xml.contains(
            "<VrstaZahtjeva>0</VrstaZahtjeva><NoviObjekat><StavkeRacuna><RacunStavka><artikal><Sifra>p1</Sifra><Naziv>p1</Naziv><JM>kom</JM><Cijena>100</Cijena><Stopa>E</Stopa><Grupa>0</Grupa><PLU>0</PLU></artikal><Kolicina>1</Kolicina><Rabat>0</Rabat></RacunStavka></StavkeRacuna>"
        ), "{xml}");
        assert!(xml.ends_with("<Napomena></Napomena><BrojRacuna>0</BrojRacuna></NoviObjekat></RacunZahtjev>"));
    }

    #[test]
    fn reklamacija_bez_placanja_salje_gotovina_nula() {
        let r: Reklamacija = serde_json::from_str(&format!(r#"{{"stavke":{STAVKE},"brojRacuna":3}}"#)).unwrap();
        let xml = reklamirani_racun(&r).unwrap().xml;
        assert!(xml.contains("<VrstePlacanja><VrstaPlacanja><Oznaka>Gotovina</Oznaka><Iznos>0</Iznos></VrstaPlacanja></VrstePlacanja>"));
        assert!(xml.contains("<BrojRacuna>3</BrojRacuna>"));
        assert!(xml.contains("<VrstaZahtjeva>2</VrstaZahtjeva>"));
    }

    fn reklamacija(placanja: &str) -> Result<String, String> {
        let r: Reklamacija = serde_json::from_str(&format!(r#"{{"stavke":{STAVKE},"brojRacuna":20,"placanja":{placanja}}}"#)).unwrap();
        reklamirani_racun(&r).map(|k| k.xml)
    }

    #[test]
    fn reklamacija_povrat_na_virman_ide_s_negativnim_iznosom() {
        // Primjer "reklamacija virmana" iz uputstva v3.0.1, 7.4.
        let xml = reklamacija(r#"[{"oznaka":"Virman","iznos":-10.33}]"#).unwrap();
        assert!(xml.contains("<VrstePlacanja><VrstaPlacanja><Oznaka>Virman</Oznaka><Iznos>-10.33</Iznos></VrstaPlacanja></VrstePlacanja>"), "{xml}");
    }

    #[test]
    fn reklamacija_odbija_doplatu_vise_placanja_i_gotovinu_s_iznosom() {
        // Pozitivan iznos = doplata kupca, uređaj bi vratio više novca.
        assert!(reklamacija(r#"[{"oznaka":"Virman","iznos":10.33}]"#).unwrap_err().contains("negativnim"));
        assert!(reklamacija(r#"[{"oznaka":"Gotovina","iznos":10}]"#).unwrap_err().contains("iznosom 0"));
        assert!(reklamacija(r#"[{"oznaka":"Gotovina","iznos":0},{"oznaka":"Kartica","iznos":-5}]"#).unwrap_err().contains("573"));
        assert!(reklamacija(r#"[{"oznaka":"Gotovina","iznos":0}]"#).unwrap().contains("<Oznaka>Gotovina</Oznaka><Iznos>0</Iznos>"));
    }

    #[test]
    fn reklamacija_bez_broja_racuna_je_greska() {
        let r: Reklamacija = serde_json::from_str(&format!(r#"{{"stavke":{STAVKE},"brojRacuna":0}}"#)).unwrap();
        assert!(reklamirani_racun(&r).is_err());
    }

    #[test]
    fn racun_s_vise_vrsta_placanja() {
        // Zakon traži da se račun može platiti s više vrsta (npr. 100 kartica + 10 gotovina).
        let r = racun(&format!(r#"{{"stavke":{STAVKE},"placanja":[{{"oznaka":"Kartica","iznos":90}},{{"oznaka":"Gotovina","iznos":10}}]}}"#));
        let xml = fiskalni_racun(&r).unwrap().xml;
        assert!(xml.contains("<VrstePlacanja><VrstaPlacanja><Oznaka>Kartica</Oznaka><Iznos>90</Iznos></VrstaPlacanja><VrstaPlacanja><Oznaka>Gotovina</Oznaka><Iznos>10</Iznos></VrstaPlacanja></VrstePlacanja>"));
    }

    #[test]
    fn kupac_se_provjerava() {
        let s = |kupac: &str| {
            let r = racun(&format!(r#"{{"stavke":{STAVKE},"placanja":[{{"oznaka":"Virman","iznos":0}}],"kupac":{kupac}}}"#));
            fiskalni_racun(&r).map(|k| k.xml)
        };
        assert!(s(r#"{"idBroj":"4200000000000","naziv":"Firma d.o.o.","postanskiBroj":"71000","grad":"Sarajevo"}"#)
            .unwrap()
            .contains("<Kupac><IDbroj>4200000000000</IDbroj><Naziv>Firma d.o.o.</Naziv><Adresa></Adresa><PostanskiBroj>71000</PostanskiBroj><Grad>Sarajevo</Grad></Kupac>"));
        assert!(s(r#"{"idBroj":"42000","naziv":"Firma"}"#).unwrap_err().contains("13 cifara"));
        assert!(s(r#"{"idBroj":"4200000000000","naziv":"Firma sa jako jako jako dugim nazivom d.o.o."}"#).unwrap_err().contains("32"));
        assert!(s(r#"{"idBroj":"4200000000000","naziv":"Firma","postanskiBroj":"710"}"#).unwrap_err().contains("5 cifara"));
    }

    #[test]
    fn periodicni_obrnut_opseg_je_greska() {
        let p: Period = serde_json::from_str(r#"{"od":"2026-03-31","do":"2026-03-01"}"#).unwrap();
        assert!(periodicni_izvjestaj(&p).is_err());
    }

    #[test]
    fn racun_bez_placanja_i_nepoznata_oznaka_su_greska() {
        let r = racun(&format!(r#"{{"stavke":{STAVKE},"placanja":[]}}"#));
        assert!(fiskalni_racun(&r).is_err());
        let r = racun(&format!(r#"{{"stavke":{STAVKE},"placanja":[{{"oznaka":"Bitcoin","iznos":1}}]}}"#));
        assert!(fiskalni_racun(&r).is_err());
    }

    #[test]
    fn pogresno_ime_polja_se_odbija() {
        // Upravo ovakav tipfeler (VrstaPlacanja umjesto VrstePlacanja) je nekad prošao nezapažen.
        let e = serde_json::from_str::<Racun>(&format!(r#"{{"stavke":{STAVKE},"placanje":[]}}"#)).unwrap_err();
        assert!(e.to_string().contains("placanje"));
    }

    #[test]
    fn escape_i_brojevi() {
        let r = racun(r#"{"stavke":[{"artikal":{"sifra":"a&b","naziv":"<Čokolada>","cijena":0.30000000000000004,"stopa":"K"},"kolicina":0.1235,"rabat":10}],"placanja":[{"oznaka":"kartica","iznos":1.005}]}"#);
        let xml = fiskalni_racun(&r).unwrap().xml;
        assert!(xml.contains("<Sifra>a&amp;b</Sifra><Naziv>&lt;Čokolada&gt;</Naziv>"));
        assert!(xml.contains("<Cijena>0.3</Cijena><Stopa>K</Stopa>"));
        assert!(xml.contains("<Kolicina>0.124</Kolicina><Rabat>10</Rabat>"));
        assert!(xml.contains("<Oznaka>Kartica</Oznaka><Iznos>1.01</Iznos>"));
    }

    #[test]
    fn periodicni_izvjestaj_formatira_datume() {
        let p: Period = serde_json::from_str(r#"{"od":"2026-03-01","do":"2026-03-31"}"#).unwrap();
        let xml = periodicni_izvjestaj(&p).unwrap().xml;
        assert!(xml.contains("<Vrijednost>1.3.2026 00:00:00</Vrijednost>"));
        assert!(xml.contains("<Vrijednost>31.3.2026 23:59:59</Vrijednost>"));
        let los: Period = serde_json::from_str(r#"{"od":"01.03.2026","do":"2026-03-31"}"#).unwrap();
        assert!(periodicni_izvjestaj(&los).is_err());
    }

    #[test]
    fn novac_ima_vrste_7_i_8_i_rezervne_putanje() {
        let n: Novac = serde_json::from_str(r#"{"iznos":50.5}"#).unwrap();
        let k = unos_novca(&n).unwrap();
        assert_eq!(k.putanje, ["/unosnovca", "/un"]);
        assert!(k.xml.contains("<VrstaZahtjeva>7</VrstaZahtjeva><NoviObjekat><Oznaka>Gotovina</Oznaka><Iznos>50.5</Iznos></NoviObjekat>"));
        assert!(povrat_novca(&n).unwrap().xml.contains("<VrstaZahtjeva>8</VrstaZahtjeva>"));
    }

    #[test]
    fn parsira_ok_odgovor_s_xsi_type_i_praznom_vrijednoscu() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<KasaOdgovor xmlns:xsi="x"><VrstaOdgovora>OK</VrstaOdgovora><Odgovori>
  <Odgovor><Naziv>BrojFiskalnogRacuna</Naziv><Vrijednost xsi:type="xsd:int">42</Vrijednost></Odgovor>
  <Odgovor><Naziv>Prazno</Naziv><Vrijednost xsi:type="xsd:string" /></Odgovor>
</Odgovori></KasaOdgovor>"#;
        let o = parsiraj(xml, Some(200));
        assert!(o.success);
        assert_eq!(o.odgovori["BrojFiskalnogRacuna"], "42");
        assert_eq!(o.odgovori["Prazno"], "");
        assert!(o.error.is_none());
    }

    #[test]
    fn parsira_gresku_s_brojem() {
        let xml = "<KasaOdgovor><VrstaOdgovora>Greska</VrstaOdgovora><Greska><Broj>522</Broj><Opis>Wrong payment</Opis></Greska></KasaOdgovor>";
        let o = parsiraj(xml, Some(200));
        assert!(!o.success);
        assert!(!o.ishod_nepoznat);
        assert_eq!(o.error.unwrap(), "Pogrešna vrsta plaćanja ili nedozvoljen režim (Wrong payment) [522]");
    }

    // --- Da li je komanda stigla do TFS-a (502 vs 504) ---

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    #[test]
    fn isporuka_razvrstava_sve_vrste_gresaka() {
        use ureq::ErrorKind as K;
        for k in [K::InvalidUrl, K::UnknownScheme, K::InsecureRequestHttpsOnly, K::InvalidProxyUrl, K::Dns, K::ConnectionFailed, K::ProxyConnect, K::ProxyUnauthorized] {
            assert_eq!(isporuka(k), Isporuka::NijePoslana, "{k:?}");
        }
        for k in [K::Io, K::BadStatus, K::BadHeader, K::TooManyRedirects, K::HTTP] {
            assert_eq!(isporuka(k), Isporuka::Nepoznato, "{k:?}");
        }
    }

    /// Čita jedan HTTP zahtjev do kraja tijela (Content-Length).
    fn procitaj_zahtjev(s: &mut TcpStream) -> String {
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        let (mut buf, mut komad) = (Vec::new(), [0u8; 4096]);
        loop {
            let n = s.read(&mut komad).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&komad[..n]);
            let tekst = String::from_utf8_lossy(&buf);
            if let Some(kraj) = tekst.find("\r\n\r\n") {
                let duzina = tekst[..kraj]
                    .lines()
                    .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                if buf.len() >= kraj + 4 + duzina {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Lažni TFS: na svaku konekciju redom šalje sljedeći sirovi HTTP odgovor;
    /// None = primi zahtjev i nikad ne odgovori. Vraća URL i primljene zahtjeve.
    fn lazni_tfs(odgovori: Vec<Option<&'static str>>) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let primljeno = Arc::new(Mutex::new(Vec::new()));
        let p = primljeno.clone();
        std::thread::spawn(move || {
            for odgovor in odgovori {
                let Ok((mut s, _)) = l.accept() else { return };
                let zahtjev = procitaj_zahtjev(&mut s);
                p.lock().unwrap().push(zahtjev);
                match odgovor {
                    Some(o) => {
                        let _ = s.write_all(o.as_bytes());
                    }
                    // Kao printer bez papira: veza ostaje otvorena dok klijent ne odustane.
                    None => {
                        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
                        let _ = s.read(&mut [0u8; 1]);
                    }
                }
            }
        });
        (url, primljeno)
    }

    fn novac() -> Komanda {
        unos_novca(&serde_json::from_str(r#"{"iznos":10}"#).unwrap()).unwrap()
    }

    #[test]
    fn zatvoren_port_je_nedostupan_502() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let o = posalji(&format!("http://127.0.0.1:{port}"), Duration::from_secs(1), &novac());
        assert!(!o.success && !o.ishod_nepoznat, "{o:?}");
        assert_eq!(o.status_code, None);
        let e = o.error.unwrap();
        assert!(e.starts_with("Tring server nije dostupan:") && e.contains("Connection Failed"), "{e}");
    }

    #[test]
    fn neispravan_tring_url_je_nedostupan_502() {
        for url in ["nije url", "ftp://127.0.0.1:8085"] {
            let o = posalji(url, Duration::from_secs(1), &novac());
            assert!(!o.ishod_nepoznat && o.status_code.is_none(), "{o:?}");
            assert!(o.error.unwrap().starts_with("Tring server nije dostupan:"));
        }
    }

    #[test]
    fn tfs_koji_ne_odgovori_je_ishod_nepoznat_504_bez_rezervne_putanje() {
        let (url, primljeno) = lazni_tfs(vec![None, None]);
        let o = posalji(&url, Duration::from_millis(500), &novac());
        assert!(!o.success && o.ishod_nepoznat, "{o:?}");
        assert_eq!(o.status_code, None);
        assert!(o.error.as_deref().unwrap().starts_with("Komanda je možda izvršena:"), "{o:?}");
        // Komanda je stvarno stigla do "TFS-a", i to samo jednom — /un se ne proba.
        let primljeno = primljeno.lock().unwrap();
        assert_eq!(primljeno.len(), 1, "{primljeno:?}");
        assert!(primljeno[0].starts_with("POST /unosnovca ") && primljeno[0].contains("<VrstaZahtjeva>7</VrstaZahtjeva>"));
        let json = serde_json::to_value(&o).unwrap();
        assert_eq!(json["ishodNepoznat"], true);
        assert!(json["statusCode"].is_null());
    }

    #[test]
    fn odsjecen_odgovor_je_ishod_nepoznat() {
        let (url, _) = lazni_tfs(vec![Some(
            "HTTP/1.1 200 OK\r\nContent-Length: 500\r\nConnection: close\r\n\r\n<KasaOdgovor><VrstaOdgovora>OK",
        )]);
        let o = posalji(&url, Duration::from_secs(5), &novac());
        assert!(!o.success && o.ishod_nepoznat, "{o:?}");
        assert_eq!(o.status_code, Some(200));
        assert!(o.error.unwrap().starts_with("Komanda je možda izvršena:"));
    }

    #[test]
    fn na_404_se_i_dalje_proba_rezervna_putanja() {
        let (url, primljeno) = lazni_tfs(vec![
            Some("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            Some("HTTP/1.1 200 OK\r\nContent-Length: 46\r\nConnection: close\r\n\r\n<KasaOdgovor><VrstaOdgovora>OK</VrstaOdgovora>"),
        ]);
        let o = posalji(&url, Duration::from_secs(5), &novac());
        assert!(o.success && !o.ishod_nepoznat, "{o:?}");
        let putanje: Vec<_> = primljeno.lock().unwrap().iter().map(|z| z.split(' ').nth(1).unwrap_or("").to_string()).collect();
        assert_eq!(putanje, ["/unosnovca", "/un"]);
        assert!(serde_json::to_value(&o).unwrap().get("ishodNepoznat").is_none());
    }

    #[test]
    fn prazan_2xx_odgovor_je_ishod_nepoznat_a_5xx_nije() {
        let o = parsiraj("", Some(200));
        assert!(o.ishod_nepoznat && o.error.unwrap().contains("HTTP 200"));
        let o = parsiraj("", Some(500));
        assert!(!o.ishod_nepoznat);
        assert_eq!(o.error.unwrap(), "Tring server vratio Greska (HTTP 500)");
    }
}
