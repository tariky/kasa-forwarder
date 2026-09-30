// Kasa forwarder: mali lokalni HTTP server koji prima JSON komande od web appa,
// sastavlja XML i šalje ga Tring.Fiscal.Server-u. Svaka komanda ide u log fajl.
//
//   kasa-forwarder.exe              pokreni server + ikonu u trayu (ako već radi: otvori prozor)
//   kasa-forwarder.exe install      autostart pri prijavi na Windows + pokreni odmah
//   kasa-forwarder.exe uninstall    ukloni autostart i zaustavi server

// U release buildu nema konzolnog prozora.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dnevnik;
#[cfg(windows)]
mod tray;
mod tring;
mod ui;
mod win;

use dnevnik::Dnevnik;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, thread};
use tiny_http::{Header, Method, Request, Response, Server};

const NASLOV: &str = "Kasa forwarder";
const MAX_TIJELO: u64 = 1024 * 1024;
const BROJ_RADNIKA: usize = 4;
pub const IKONA: &[u8] = include_bytes!("../assets/ikona.ico");

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct Config {
    /// 127.0.0.1:PORT = samo browser na ovom računaru; 0.0.0.0:PORT = cijela mreža.
    listen: String,
    tring_url: String,
    timeout_sekundi: u64,
    /// Origin-i web appa kojima browser smije slati komande; "*" = svi.
    allowed_origins: Vec<String>,
    /// Ako nije prazan, svaki POST mora imati header X-Kasa-Token s ovom vrijednošću.
    token: String,
    log_max_mb: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8086".into(),
            tring_url: "http://localhost:8085".into(),
            timeout_sekundi: 30,
            allowed_origins: vec!["*".into()],
            token: String::new(),
            log_max_mb: 5,
        }
    }
}

pub struct App {
    pub cfg: Config,
    pub dnevnik: Dnevnik,
    pub exe: PathBuf,
    pub dir: PathBuf,
    /// Port na kojem sluša — prozor forwardera je uvijek na http://127.0.0.1:port/ui.
    pub port: u16,
    /// Printer štampa jedan dokument za drugim — dva klika ne smiju poslati dva računa istovremeno.
    tring: Mutex<()>,
}

fn main() {
    let exe = env::current_exe().expect("putanja do exe fajla");
    let dir = exe.parent().expect("folder exe fajla").to_path_buf();
    let args: Vec<String> = env::args().skip(1).collect();
    let autostart = args.iter().any(|a| a == "--autostart");
    let restart = args.iter().any(|a| a == "--restart");

    match args.first().map(String::as_str) {
        Some("install") => return instaliraj(&exe, &dir),
        Some("uninstall") => return deinstaliraj(&exe),
        _ => {}
    }

    let cfg = match ucitaj_config(&dir) {
        Ok(c) => c,
        Err(e) => {
            Dnevnik::new(dir.join("kasa-forwarder.log"), 5 << 20).upisi(&format!("START GREŠKA {e}"));
            if !autostart {
                win::poruka(NASLOV, &e, true);
            }
            return;
        }
    };
    let dnevnik = Dnevnik::new(dir.join("kasa-forwarder.log"), cfg.log_max_mb.max(1) << 20);

    let port = port_iz(&cfg.listen);

    // Pri restartu stari proces još par stotina ms drži port.
    let mut pokusaja = if restart { 50 } else { 1 };
    let server = loop {
        pokusaja -= 1;
        match Server::http(&cfg.listen) {
            Ok(s) => break Ok(Arc::new(s)),
            Err(e) if pokusaja == 0 => break Err(e),
            Err(_) => thread::sleep(Duration::from_millis(200)),
        }
    };
    let server = match server {
        Ok(s) => s,
        Err(e) => {
            // Dvoklik na exe dok forwarder već radi = otvori njegov prozor.
            if !autostart && vec_radi(port) {
                return otvori_prozor(port);
            }
            dnevnik.upisi(&format!("START GREŠKA ne mogu slušati na {}: {e}", cfg.listen));
            // Pri autostartu je ovo najčešće druga instanca koja već radi — bez popupa.
            if !autostart {
                win::poruka(
                    NASLOV,
                    &format!("Ne mogu pokrenuti server na {}.\n\nForwarder vjerovatno već radi, ili port koristi drugi program.\n\n{e}", cfg.listen),
                    true,
                );
            }
            return;
        }
    };
    dnevnik.upisi(&format!(
        "START v{} sluša na {} -> {}{}",
        env!("CARGO_PKG_VERSION"),
        cfg.listen,
        cfg.tring_url,
        if cfg.token.is_empty() { " (bez tokena)" } else { "" }
    ));

    let app = Arc::new(App { cfg, dnevnik, exe, dir, port, tring: Mutex::new(()) });
    let radnici: Vec<_> = (0..BROJ_RADNIKA)
        .map(|_| {
            let (server, app) = (server.clone(), app.clone());
            thread::spawn(move || {
                while let Ok(rq) = server.recv() {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| obradi(&app, rq)));
                }
            })
        })
        .collect();

    // Ručno pokretanje (Start meni, dvoklik) odmah pokazuje prozor; autostart je tih.
    if !autostart && !restart {
        otvori_prozor(port);
    }

    #[cfg(windows)]
    {
        let (a1, a2, a3) = (app.clone(), app.clone(), app.clone());
        tray::pokreni(tray::Tray {
            opis: format!("Kasa forwarder · {}", app.cfg.listen),
            na_otvori: Box::new(move || otvori_prozor(a1.port)),
            na_folder: Box::new(move || win::otvori(&a2.dir.display().to_string())),
            na_ugasi: Box::new(move |razlog| {
                a3.dnevnik.upisi(&format!("STOP {razlog}"));
                std::process::exit(0);
            }),
        });
    }
    for r in radnici {
        let _ = r.join();
    }
}

fn port_iz(listen: &str) -> u16 {
    listen.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(8086)
}

pub fn autostart_komanda(exe: &Path) -> String {
    format!("\"{}\" --autostart", exe.display())
}

/// Gasi ovaj proces i pokreće novi s novim podešavanjima.
pub fn restartuj(app: &App, razlog: &str) {
    app.dnevnik.upisi(&format!("RESTART {razlog}"));
    if let Err(e) = std::process::Command::new(&app.exe).args(["--autostart", "--restart"]).spawn() {
        app.dnevnik.upisi(&format!("RESTART GREŠKA ne mogu pokrenuti novi proces: {e}"));
        return;
    }
    #[cfg(windows)]
    tray::ukloni();
    std::process::exit(0);
}

/// Da li na portu već radi kasa-forwarder (a ne neki drugi program).
fn vec_radi(port: u16) -> bool {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(2))
        .build()
        .get(&format!("http://127.0.0.1:{port}/api/health"))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .is_some_and(|v| v.get("verzija").is_some())
}

/// Prozor forwardera: Edge u "app" modu (bez adresne trake), inače obični browser.
fn otvori_prozor(port: u16) {
    let url = format!("http://127.0.0.1:{port}/ui");
    let edge = ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(|v| env::var_os(v))
        .map(|p| PathBuf::from(p).join(r"Microsoft\Edge\Application\msedge.exe"))
        .find(|p| p.exists());
    if let Some(edge) = edge {
        let pokrenut = std::process::Command::new(edge)
            .args([format!("--app={url}"), "--window-size=1180,780".into()])
            .spawn();
        if pokrenut.is_ok() {
            return;
        }
    }
    win::otvori(&url);
}

fn ucitaj_config(dir: &Path) -> Result<Config, String> {
    let putanja = dir.join("config.json");
    if !putanja.exists() {
        let c = Config::default();
        let _ = fs::write(&putanja, serde_json::to_string_pretty(&c).unwrap_or_default());
        return Ok(c);
    }
    let tekst = fs::read_to_string(&putanja).map_err(|e| format!("Ne mogu pročitati {}: {e}", putanja.display()))?;
    serde_json::from_str(tekst.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Greška u {}:\n{e}", putanja.display()))
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

fn header(ime: &str, vrijednost: &str) -> Header {
    Header::from_bytes(ime.as_bytes(), vrijednost.as_bytes()).expect("ispravan header")
}

pub fn procitaj_header(rq: &Request, ime: &'static str) -> Option<String> {
    rq.headers().iter().find(|h| h.field.equiv(ime)).map(|h| h.value.as_str().to_string())
}

fn origin_dozvoljen(cfg: &Config, origin: &str) -> bool {
    let origin = origin.trim_end_matches('/');
    cfg.allowed_origins.iter().any(|o| o == "*" || o.trim_end_matches('/').eq_ignore_ascii_case(origin))
}

pub fn odgovori(rq: Request, status: u16, tijelo: &Value, cors: &[Header]) {
    let mut r = Response::from_string(tijelo.to_string())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json; charset=utf-8"));
    for h in cors {
        r.add_header(h.clone());
    }
    let _ = rq.respond(r);
}

fn greska(poruka: impl Into<String>) -> Value {
    json!({ "success": false, "error": poruka.into() })
}

fn obradi(app: &Arc<App>, mut rq: Request) {
    let pocetak = Instant::now();
    let cfg = &app.cfg;
    let metoda = rq.method().clone();
    let putanja = rq.url().split('?').next().unwrap_or("").to_string();
    if ui::je_ui_ruta(&putanja) {
        return ui::obradi(app, rq, &putanja);
    }
    let klijent = rq.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let origin = procitaj_header(&rq, "Origin");

    let mut cors = Vec::new();
    if let Some(o) = &origin {
        if !origin_dozvoljen(cfg, o) {
            app.dnevnik.upisi(&format!("{klijent} {metoda} {putanja} -> ODBIJENO 403 origin {o} nije u allowedOrigins"));
            return odgovori(rq, 403, &greska(format!("Origin {o} nije dozvoljen (allowedOrigins u config.json)")), &[]);
        }
        cors.push(header("Access-Control-Allow-Origin", o));
        cors.push(header("Vary", "Origin"));
    }

    if metoda == Method::Options {
        cors.push(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
        cors.push(header("Access-Control-Allow-Headers", "Content-Type, X-Kasa-Token"));
        // Chrome traži ovo kad stranica s mreže šalje zahtjev na localhost.
        cors.push(header("Access-Control-Allow-Private-Network", "true"));
        cors.push(header("Access-Control-Max-Age", "600"));
        let mut r = Response::empty(204);
        for h in cors {
            r.add_header(h);
        }
        let _ = rq.respond(r);
        return;
    }

    if metoda == Method::Get && putanja == "/api/health" {
        return odgovori(rq, 200, &health(cfg), &cors);
    }
    if metoda != Method::Post {
        return odgovori(rq, 404, &greska(format!("Nepoznata ruta {metoda} {putanja}")), &cors);
    }

    if !cfg.token.is_empty() && procitaj_header(&rq, "X-Kasa-Token").as_deref() != Some(cfg.token.as_str()) {
        app.dnevnik.upisi(&format!("{klijent} POST {putanja} -> ODBIJENO 401 pogrešan ili nedostaje X-Kasa-Token"));
        return odgovori(rq, 401, &greska("Pogrešan ili nedostaje X-Kasa-Token"), &cors);
    }

    let mut tijelo = String::new();
    if rq.as_reader().take(MAX_TIJELO + 1).read_to_string(&mut tijelo).is_err() || tijelo.len() as u64 > MAX_TIJELO {
        return odgovori(rq, 400, &greska("Tijelo zahtjeva nije ispravan UTF-8 ili je veće od 1 MB"), &cors);
    }
    let ulaz: Value = if tijelo.trim().is_empty() {
        json!({})
    } else {
        match serde_json::from_str(&tijelo) {
            Ok(v) => v,
            Err(e) => {
                let poruka = format!("Neispravan JSON: {e}");
                app.dnevnik.upisi(&format!("{klijent} POST {putanja} -> ODBIJENO 400 {poruka} | {tijelo}"));
                return odgovori(rq, 400, &greska(poruka), &cors);
            }
        }
    };
    let ulaz_za_log = za_log(&ulaz);

    let komanda = match sastavi(&putanja, ulaz) {
        None => return odgovori(rq, 404, &greska(format!("Nepoznata komanda {putanja}")), &cors),
        Some(Err(e)) => {
            app.dnevnik.upisi(&format!("{klijent} POST {putanja} -> ODBIJENO 400 {e} | {ulaz_za_log}"));
            return odgovori(rq, 400, &greska(e), &cors);
        }
        Some(Ok(k)) => k,
    };

    let rezultat = {
        let _red = app.tring.lock().unwrap_or_else(|e| e.into_inner());
        tring::posalji(&cfg.tring_url, Duration::from_secs(cfg.timeout_sekundi), &komanda)
    };
    let ms = pocetak.elapsed().as_millis();

    if rezultat.success {
        let odgovori_txt: Vec<String> = rezultat.odgovori.iter().map(|(k, v)| format!("{k}={v}")).collect();
        app.dnevnik.upisi(&format!("{klijent} POST {putanja} -> OK {ms}ms{} | {ulaz_za_log}", odgovori_txt.iter().map(|o| format!(" {o}")).collect::<String>()));
    } else {
        // Kod greške se čuva i XML — bez toga se ne da rekonstruisati šta je uređaj dobio.
        // [ISHOD NEPOZNAT] = možda izvršeno (504), da se u logu razlikuje od sigurne greške.
        app.dnevnik.upisi(&format!(
            "{klijent} POST {putanja} -> GREŠKA {ms}ms {}{} | {ulaz_za_log} | poslano: {} | odgovor: {}",
            if rezultat.ishod_nepoznat { "[ISHOD NEPOZNAT] " } else { "" },
            rezultat.error.as_deref().unwrap_or(""),
            komanda.xml,
            rezultat.sirovi
        ));
    }

    // 200 = TFS je odgovorio (success kaže da li je uspjelo), 502 = komanda sigurno
    // nije stigla do TFS-a, 504 = možda je izvršena — web app je ne smije sam ponoviti.
    let status = if rezultat.ishod_nepoznat {
        504
    } else if rezultat.status_code.is_some() {
        200
    } else {
        502
    };
    let tijelo = serde_json::to_value(&rezultat).unwrap_or_else(|_| greska("interna greška"));
    odgovori(rq, status, &tijelo, &cors);
}

fn sastavi(putanja: &str, ulaz: Value) -> Option<Result<tring::Komanda, String>> {
    fn p<T: DeserializeOwned>(v: Value) -> Result<T, String> {
        serde_json::from_value(v).map_err(|e| format!("Neispravan JSON: {e}"))
    }
    Some(match putanja {
        "/api/inicijalizacija" => p(ulaz).and_then(|z| tring::inicijalizacija(&z)),
        "/api/artikal" => p(ulaz).and_then(|a| tring::upisi_artikal(&a)),
        "/api/racun" => p(ulaz).and_then(|r| tring::fiskalni_racun(&r)),
        "/api/reklamacija" => p(ulaz).and_then(|r| tring::reklamirani_racun(&r)),
        "/api/presjek-stanja" => Ok(tring::presjek_stanja()),
        "/api/dnevni-izvjestaj" => Ok(tring::dnevni_izvjestaj()),
        "/api/periodicni-izvjestaj" => p(ulaz).and_then(|z| tring::periodicni_izvjestaj(&z)),
        "/api/unos-novca" => p(ulaz).and_then(|n| tring::unos_novca(&n)),
        "/api/povrat-novca" => p(ulaz).and_then(|n| tring::povrat_novca(&n)),
        _ => return None,
    })
}

/// JSON u jednoj liniji, bez lozinke operatora.
fn za_log(v: &Value) -> String {
    let mut v = v.clone();
    if let Some(l) = v.get_mut("lozinka") {
        *l = json!("***");
    }
    v.to_string()
}

pub fn health(cfg: &Config) -> Value {
    // Samo TCP konekcija — ne ide kroz red za printer, pa radi i dok se štampa.
    let adresa = cfg.tring_url.trim_start_matches("http://").split('/').next().unwrap_or("");
    let dostupan = adresa
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(1)).is_ok())
        .unwrap_or(false);
    json!({
        "ok": true,
        "verzija": env!("CARGO_PKG_VERSION"),
        "tringUrl": cfg.tring_url,
        "tringDostupan": dostupan,
    })
}

// ---------------------------------------------------------------------------
// install / uninstall
// ---------------------------------------------------------------------------

fn instaliraj(exe: &Path, dir: &PathBuf) {
    let cfg = match ucitaj_config(dir) {
        Ok(c) => c,
        Err(e) => return win::poruka(NASLOV, &e, true),
    };
    if let Err(e) = win::ukljuci_autostart(&autostart_komanda(exe)) {
        return win::poruka(NASLOV, &format!("Instalacija nije uspjela: {e}"), true);
    }
    let _ = std::process::Command::new(exe).arg("--autostart").spawn();
    win::poruka(
        NASLOV,
        &format!(
            "Forwarder je pokrenut i pokretaće se automatski pri svakoj prijavi na Windows.\n\nIkona je u system trayu (pored sata) — dvoklik otvara log i podešavanja.\n\nAdresa: http://{}\nTring: {}\n\nPodešavanja: {}\nLog: {}",
            cfg.listen,
            cfg.tring_url,
            dir.join("config.json").display(),
            dir.join("kasa-forwarder.log").display()
        ),
        false,
    );
}

fn deinstaliraj(exe: &Path) {
    win::iskljuci_autostart();
    let ime = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/F", "/IM", &ime, "/FI", &format!("PID ne {}", std::process::id())]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let _ = cmd.status();
    win::poruka(NASLOV, "Autostart je uklonjen i forwarder je zaustavljen.", false);
}
