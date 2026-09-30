// Ugrađuje assets/ikona.ico u exe (ikona u Exploreru i na taskbaru).
//
// GNU toolchain nema windres, pa se resursni COFF objekat (.rsrc sekcija s
// RT_ICON + RT_GROUP_ICON) piše ručno i predaje linkeru kao obični objekat.

use std::{env, fs, path::PathBuf};

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const PODDIREKTORIJ: u32 = 0x8000_0000;
const IMAGE_REL_AMD64_ADDR32NB: u16 = 3;

fn main() {
    println!("cargo:rerun-if-changed=assets/ikona.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("x86_64") {
        println!("cargo:warning=ikona se ugrađuje samo za x86_64");
        return;
    }
    let ico = fs::read("assets/ikona.ico").expect("assets/ikona.ico");
    let izlaz = PathBuf::from(env::var("OUT_DIR").unwrap()).join("ikona-resurs.o");
    fs::write(&izlaz, coff_objekat(&ico)).unwrap();
    println!("cargo:rustc-link-arg-bins={}", izlaz.display());
}

fn u16_le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Direktorij resursa: zaglavlje od 16 bajta + 8 bajta po (id, pomak).
fn direktorij(stavke: &[(u32, u32)]) -> Vec<u8> {
    let mut d = vec![0u8; 12];
    d.extend(0u16.to_le_bytes()); // imenovanih stavki
    d.extend((stavke.len() as u16).to_le_bytes());
    for (id, pomak) in stavke {
        d.extend(id.to_le_bytes());
        d.extend(pomak.to_le_bytes());
    }
    d
}

fn coff_objekat(ico: &[u8]) -> Vec<u8> {
    let n = u16_le(ico, 4) as usize;

    // Slike iz ICO-a postaju RT_ICON 1..n; RT_GROUP_ICON 1 ih navodi.
    let mut grupa = Vec::new();
    grupa.extend([0, 0, 1, 0]);
    grupa.extend((n as u16).to_le_bytes());
    let mut blobovi = Vec::new();
    for i in 0..n {
        let opis = &ico[6 + 16 * i..22 + 16 * i];
        let (velicina, pomak) = (u32_le(opis, 8) as usize, u32_le(opis, 12) as usize);
        blobovi.push(ico[pomak..pomak + velicina].to_vec());
        grupa.extend(&opis[..12]); // širina, visina, boje, rezervisano, ravni, bpp, veličina
        grupa.extend(((i + 1) as u16).to_le_bytes());
    }
    blobovi.push(grupa);
    let broj = blobovi.len();

    // Raspored: korijen | tip RT_ICON | tip RT_GROUP_ICON | jezik × broj | podaci × broj | blobovi
    let korijen = 0u32;
    let tip_ikona = korijen + 16 + 2 * 8;
    let tip_grupa = tip_ikona + 16 + 8 * n as u32;
    let jezik = |i: usize| tip_grupa + 24 + 24 * i as u32;
    let podatak = |i: usize| jezik(broj) + 16 * i as u32;
    let mut pomak_bloba = (podatak(broj) + 7) & !7;

    let mut s = Vec::new();
    s.extend(direktorij(&[(RT_ICON, PODDIREKTORIJ | tip_ikona), (RT_GROUP_ICON, PODDIREKTORIJ | tip_grupa)]));
    let ikone: Vec<_> = (0..n).map(|i| ((i + 1) as u32, PODDIREKTORIJ | jezik(i))).collect();
    s.extend(direktorij(&ikone));
    s.extend(direktorij(&[(1, PODDIREKTORIJ | jezik(n))]));
    for i in 0..broj {
        s.extend(direktorij(&[(0x0409, podatak(i))]));
    }
    let mut relokacije = Vec::new();
    for (i, b) in blobovi.iter().enumerate() {
        relokacije.push(podatak(i));
        s.extend(pomak_bloba.to_le_bytes()); // RVA — linker dodaje adresu sekcije
        s.extend((b.len() as u32).to_le_bytes());
        s.extend([0u8; 8]);
        pomak_bloba = (pomak_bloba + b.len() as u32 + 7) & !7;
    }
    for b in &blobovi {
        s.resize((s.len() + 7) & !7, 0);
        s.extend(b);
    }

    // COFF: zaglavlje (20) + zaglavlje sekcije (40) + sekcija + relokacije + simbol + tabela stringova
    let pocetak_sekcije = 60u32;
    let pocetak_relokacija = pocetak_sekcije + s.len() as u32;
    let pocetak_simbola = pocetak_relokacija + 10 * relokacije.len() as u32;

    let mut o = Vec::new();
    o.extend(0x8664u16.to_le_bytes());
    o.extend(1u16.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend(pocetak_simbola.to_le_bytes());
    o.extend(1u32.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.extend(0u16.to_le_bytes());

    o.extend(b".rsrc\0\0\0");
    o.extend(0u32.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend((s.len() as u32).to_le_bytes());
    o.extend(pocetak_sekcije.to_le_bytes());
    o.extend(pocetak_relokacija.to_le_bytes());
    o.extend(0u32.to_le_bytes());
    o.extend((relokacije.len() as u16).to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.extend(0x4030_0040u32.to_le_bytes()); // inicijalizovani podaci, poravnanje 4, čitanje

    o.extend(&s);
    for r in relokacije {
        o.extend(r.to_le_bytes());
        o.extend(0u32.to_le_bytes()); // simbol 0 = sekcija .rsrc
        o.extend(IMAGE_REL_AMD64_ADDR32NB.to_le_bytes());
    }
    o.extend(b".rsrc\0\0\0");
    o.extend(0u32.to_le_bytes());
    o.extend(1i16.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.push(3); // IMAGE_SYM_CLASS_STATIC
    o.push(0);
    o.extend(4u32.to_le_bytes()); // prazna tabela stringova
    o
}
