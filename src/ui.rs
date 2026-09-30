// Prozor forwardera (otvara se iz traya): pregled loga i podešavanja.
// Sve /ui rute su dostupne samo s ovog računara i samo s vlastite stranice —
// log sadrži podatke o računima, a podešavanja mijenjaju token.

use crate::{odgovori, App, Config};
use serde_json::{json, Value};
use std::io::Read;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;
use std::{fs, thread};
use tiny_http::{Header, Method, Request, Response};

const STRANICA: &str = include_str!("ui.html");
const IKONA_SVG: &str = include_str!("../assets/ikona.svg");
const MAX_STRANICA: usize = 500;

pub fn je_ui_ruta(putanja: &str) -> bool {
    putanja == "/" || putanja == "/ui" || putanja.starts_with("/ui/") || putanja == "/favicon.ico"
}

fn staticno(rq: Request, tip: &str, tijelo: impl Into<Vec<u8>>) {
    let r = Response::from_data(tijelo.into())
        .with_header(Header::from_bytes("Content-Type", tip).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap());
    let _ = rq.respond(r);
}

fn lokalni_zahtjev(app: &App, rq: &Request) -> bool {
    let loopback = rq.remote_addr().map(|a| a.ip().is_loopback()).unwrap_or(false);
    let p = app.port;
    let dozvoljeni = [format!("127.0.0.1:{p}"), format!("localhost:{p}"), format!("[::1]:{p}")];
    // Host štiti od DNS rebindinga, Origin od drugih stranica otvorenih u browseru.
    let host_ok = crate::procitaj_header(rq, "Host").is_some_and(|h| dozvoljeni.iter().any(|d| d.eq_ignore_ascii_case(&h)));
    let origin_ok = match crate::procitaj_header(rq, "Origin") {
        None => true,
        Some(o) => dozvoljeni.iter().any(|d| o.eq_ignore_ascii_case(&format!("http://{d}"))),
    };
    loopback && host_ok && origin_ok
}

pub fn obradi(app: &Arc<App>, mut rq: Request, putanja: &str) {
    if !lokalni_zahtjev(app, &rq) {
        return odgovori(rq, 403, &json!({ "error": "Prozor forwardera je dostupan samo na ovom računaru" }), &[]);
    }
    let upit = rq.url().split_once('?').map(|(_, u)| u.to_string()).unwrap_or_default();

    match (rq.method(), putanja) {
        (Method::Get, "/") => {
            let r = Response::empty(302).with_header(Header::from_bytes("Location", "/ui").unwrap());
            let _ = rq.respond(r);
        }
        (Method::Get, "/ui") => staticno(rq, "text/html; charset=utf-8", STRANICA),
        (Method::Get, "/ui/ikona.svg") => staticno(rq, "image/svg+xml", IKONA_SVG),
        (Method::Get, "/favicon.ico") => staticno(rq, "image/x-icon", crate::IKONA),
        (Method::Get, "/ui/stanje") => odgovori(rq, 200, &stanje(app), &[]),
        (Method::Get, "/ui/log") => odgovori(rq, 200, &log(app, &upit), &[]),
        (Method::Get, "/ui/postavke") => {
            let mut v = serde_json::to_value(&app.cfg).unwrap_or_default();
            v["autostart"] = json!(crate::win::autostart_ukljucen());
            odgovori(rq, 200, &v, &[])
        }
        (Method::Post, "/ui/postavke") => {
            let mut tijelo = String::new();
            let _ = rq.as_reader().take(64 * 1024).read_to_string(&mut tijelo);
            match sacuvaj_postavke(app, &tijelo) {
                Ok((restart, odgovor)) => {
                    odgovori(rq, 200, &odgovor, &[]);
                    if restart {
                        let app = app.clone();
                        thread::spawn(move || {
                            // Da odgovor stigne do stranice prije gašenja.
                            thread::sleep(Duration::from_millis(300));
                            crate::restartuj(&app, "promijenjena podešavanja");
                        });
                    }
                }
                Err(e) => odgovori(rq, 400, &json!({ "error": e }), &[]),
            }
        }
        _ => odgovori(rq, 404, &json!({ "error": "Nepoznata ruta" }), &[]),
    }
}

// ---------------------------------------------------------------------------
// Stanje i log
// ---------------------------------------------------------------------------

fn putanja_loga(app: &App) -> std::path::PathBuf {
    app.dir.join("kasa-forwarder.log")
}

/// Linije oba log fajla, od najstarije do najnovije.
fn linije_loga(app: &App) -> Vec<String> {
    let log = putanja_loga(app);
    let mut sve = Vec::new();
    for p in [log.with_extension("log.1"), log] {
        if let Ok(b) = fs::read(&p) {
            sve.extend(String::from_utf8_lossy(&b).lines().filter(|l| !l.is_empty()).map(str::to_string));
        }
    }
    sve
}

fn stanje(app: &App) -> Value {
    let danas = &crate::win::lokalno_vrijeme()[..10];
    let (mut racuni, mut reklamacije, mut greske, mut zadnja) = (0, 0, 0, Value::Null);
    for l in linije_loga(app).iter().filter(|l| l.starts_with(danas)) {
        if l.contains(" POST /api/racun -> OK") {
            racuni += 1;
        } else if l.contains(" POST /api/reklamacija -> OK") {
            reklamacije += 1;
        } else if l.contains(" -> GREŠKA ") {
            greske += 1;
        }
        if l.contains(" -> ") {
            zadnja = json!(l.get(11..19).unwrap_or(""));
        }
    }
    let health = crate::health(&app.cfg);
    json!({
        "verzija": env!("CARGO_PKG_VERSION"),
        "listen": app.cfg.listen,
        "tringUrl": app.cfg.tring_url,
        "tringDostupan": health["tringDostupan"],
        "folder": app.dir.display().to_string(),
        "danas": { "racuni": racuni, "reklamacije": reklamacije, "greske": greske, "zadnjaKomanda": zadnja },
    })
}

fn log(app: &App, upit: &str) -> Value {
    let param = |ime: &str| {
        upit.split('&')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| *k == ime)
            .map(|(_, v)| dekodiraj(v))
            .unwrap_or_default()
    };
    let trazi = param("q").to_lowercase();
    let vrsta = param("vrsta");
    let od: usize = param("od").parse().unwrap_or(0);
    let limit: usize = param("limit").parse().unwrap_or(100).clamp(1, MAX_STRANICA);

    let mut ukupno = 0;
    let mut stranica = Vec::new();
    for l in linije_loga(app).into_iter().rev() {
        let odgovara_vrsti = match vrsta.as_str() {
            "ok" => l.contains(" -> OK "),
            "greske" => l.contains(" -> GREŠKA "),
            "odbijeno" => l.contains(" -> ODBIJENO "),
            "sistem" => !l.contains(" -> "),
            _ => true,
        };
        if !odgovara_vrsti || (!trazi.is_empty() && !l.to_lowercase().contains(&trazi)) {
            continue;
        }
        if ukupno >= od && stranica.len() < limit {
            stranica.push(l);
        }
        ukupno += 1;
    }
    json!({ "ukupno": ukupno, "od": od, "linije": stranica })
}

/// Percent-decode za query string (UTF-8, + je razmak).
fn dekodiraj(s: &str) -> String {
    let b = s.as_bytes();
    let mut izlaz = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => izlaz.push(b' '),
            b'%' => match (b.get(i + 1).and_then(hex), b.get(i + 2).and_then(hex)) {
                (Some(a), Some(c)) => {
                    izlaz.push(a * 16 + c);
                    i += 2;
                }
                _ => izlaz.push(b'%'),
            },
            c => izlaz.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&izlaz).into_owned()
}

fn hex(c: &u8) -> Option<u8> {
    (*c as char).to_digit(16).map(|d| d as u8)
}

// ---------------------------------------------------------------------------
// Podešavanja
// ---------------------------------------------------------------------------

fn sacuvaj_postavke(app: &App, tijelo: &str) -> Result<(bool, Value), String> {
    let mut v: Value = serde_json::from_str(tijelo).map_err(|e| format!("Neispravan JSON: {e}"))?;
    let autostart = v.as_object_mut().and_then(|o| o.remove("autostart")).and_then(|a| a.as_bool());
    let novi: Config = serde_json::from_value(v).map_err(|e| format!("Neispravna podešavanja: {e}"))?;
    provjeri(&novi, &app.cfg)?;

    if let Some(ukljuci) = autostart {
        if ukljuci != crate::win::autostart_ukljucen() {
            if ukljuci {
                crate::win::ukljuci_autostart(&crate::autostart_komanda(&app.exe))?;
            } else {
                crate::win::iskljuci_autostart();
            }
            app.dnevnik.upisi(&format!("AUTOSTART {}", if ukljuci { "uključen" } else { "isključen" }));
        }
    }

    let promijenjeno = serde_json::to_value(&novi).ok() != serde_json::to_value(&app.cfg).ok();
    if promijenjeno {
        let tekst = serde_json::to_string_pretty(&novi).map_err(|e| e.to_string())?;
        fs::write(app.dir.join("config.json"), tekst).map_err(|e| format!("Ne mogu snimiti config.json: {e}"))?;
        app.dnevnik.upisi("PODEŠAVANJA snimljena iz prozora forwardera");
    }
    let port = novi.listen.rsplit(':').next().unwrap_or("8086").to_string();
    Ok((promijenjeno, json!({ "ok": true, "restart": promijenjeno, "url": format!("http://127.0.0.1:{port}/ui") })))
}

fn provjeri(c: &Config, trenutni: &Config) -> Result<(), String> {
    let adresa: std::net::SocketAddr = c.listen.parse().map_err(|_| format!("Adresa servera \"{}\" nije ispravna (npr. 127.0.0.1:8086)", c.listen))?;
    let stari_port = trenutni.listen.rsplit(':').next().and_then(|p| p.parse::<u16>().ok());
    if Some(adresa.port()) != stari_port && TcpListener::bind(adresa).is_err() {
        return Err(format!("Port {} je zauzet — izaberi drugi", adresa.port()));
    }
    let bez_seme = c.tring_url.strip_prefix("http://").ok_or("Adresa Tring servera mora počinjati s http://")?;
    if bez_seme.split('/').next().unwrap_or("").is_empty() {
        return Err("Adresa Tring servera nije ispravna".into());
    }
    if !(1..=300).contains(&c.timeout_sekundi) {
        return Err("Timeout mora biti između 1 i 300 sekundi".into());
    }
    if !(1..=100).contains(&c.log_max_mb) {
        return Err("Veličina loga mora biti između 1 i 100 MB".into());
    }
    for o in &c.allowed_origins {
        if o != "*" && !(o.starts_with("http://") || o.starts_with("https://")) {
            return Err(format!("Dozvoljena adresa \"{o}\" mora počinjati s http:// ili https:// (ili biti *)"));
        }
    }
    if !adresa.ip().is_loopback() && c.token.is_empty() {
        return Err("Kad je forwarder dostupan s mreže, token je obavezan".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::dekodiraj;

    #[test]
    fn dekodira_query() {
        assert_eq!(dekodiraj("ra%C4%8Dun+br"), "račun br");
        assert_eq!(dekodiraj("100%"), "100%");
        assert_eq!(dekodiraj("%zz"), "%zz");
    }
}
