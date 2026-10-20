// Log fajl: jedna linija po komandi. Kad pređe max veličinu, stari se preimenuje
// u .log.1 (prethodni .log.1 se briše) — ukupno nikad više od 2 × max.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Dnevnik {
    putanja: PathBuf,
    max_bajta: u64,
    fajl: Mutex<Option<File>>,
}

impl Dnevnik {
    pub fn new(putanja: PathBuf, max_bajta: u64) -> Self {
        Self { putanja, max_bajta, fajl: Mutex::new(None) }
    }

    /// Vraća vrijeme upisa — ono je i ključ linije (prozor po njemu nalazi zapis).
    pub fn upisi(&self, poruka: &str) -> String {
        let mut fajl = self.fajl.lock().unwrap_or_else(|e| e.into_inner());

        let prevelik = fs::metadata(&self.putanja).map(|m| m.len() >= self.max_bajta).unwrap_or(false);
        if prevelik {
            *fajl = None;
            let stari = self.putanja.with_extension("log.1");
            let _ = fs::remove_file(&stari);
            let _ = fs::rename(&self.putanja, &stari);
        }
        if fajl.is_none() {
            *fajl = OpenOptions::new().create(true).append(true).open(&self.putanja).ok();
        }
        let vrijeme = crate::win::lokalno_vrijeme();
        if let Some(f) = fajl.as_mut() {
            // Jedna komanda = jedna linija, bez obzira šta je u JSON-u/XML-u.
            let poruka = poruka.replace(['\r', '\n'], " ");
            let _ = writeln!(f, "{vrijeme} {poruka}");
        }
        vrijeme
    }
}
