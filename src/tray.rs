// Ikona u system trayu: dvoklik otvara prozor, desni klik meni, obavijesti o greškama.
// Radi na glavnoj niti (Win32 petlja poruka); server radi na svojim nitima.

use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIN_BALLOONUSERCLICK, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
const ID_OTVORI: usize = 1;
const ID_FOLDER: usize = 2;
const ID_UGASI: usize = 3;

pub struct Tray {
    pub opis: String,
    pub na_otvori: Box<dyn Fn() + Send + Sync>,
    /// Klik na obavijest; argument je ključ iz `obavijesti`.
    pub na_obavijest: Box<dyn Fn(&str) + Send + Sync>,
    pub na_folder: Box<dyn Fn() + Send + Sync>,
    /// Argument je razlog gašenja, za log.
    pub na_ugasi: Box<dyn Fn(&str) + Send + Sync>,
}

static TRAY: OnceLock<Tray> = OnceLock::new();
static HWND_TRAY: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// Ključ zadnje obavijesti — Windows pokazuje samo jednu, pa klik uvijek znači zadnju.
static ZADNJA_OBAVIJEST: Mutex<String> = Mutex::new(String::new());

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Kopira tekst u fiksno polje strukture (odsiječe ako je predug, uvijek završi nulom).
fn upisi_u(polje: &mut [u16], s: &str) {
    let mut n = 0;
    for c in s.encode_utf16().take(polje.len() - 1) {
        polje[n] = c;
        n += 1;
    }
    polje[n] = 0;
}

/// Ikona iz ugrađenog .ico (PNG slike), najbliža traženoj veličini.
pub fn ucitaj_ikonu(ico: &[u8], velicina: i32) -> HICON {
    let n = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    let mut izbor = None;
    for i in 0..n {
        let e = &ico[6 + 16 * i..22 + 16 * i];
        let sirina = if e[0] == 0 { 256 } else { e[0] as i32 };
        let vel = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let pomak = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        // Najmanja slika koja nije manja od tražene; inače najveća.
        let bolja = match izbor {
            None => true,
            Some((s, _, _)) => (sirina >= velicina && (s < velicina || sirina < s)) || (s < velicina && sirina > s),
        };
        if bolja {
            izbor = Some((sirina, pomak, vel));
        }
    }
    let Some((_, pomak, vel)) = izbor else { return std::ptr::null_mut() };
    unsafe {
        CreateIconFromResourceEx(ico[pomak..].as_ptr(), vel as u32, 1, 0x0003_0000, velicina, velicina, LR_DEFAULTCOLOR)
    }
}

fn podaci_ikone(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut d: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    d.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    d.hWnd = hwnd;
    d.uID = 1;
    d
}

unsafe fn dodaj_ikonu(hwnd: HWND) {
    let t = TRAY.get().unwrap();
    let mut d = podaci_ikone(hwnd);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = ucitaj_ikonu(crate::IKONA, GetSystemMetrics(SM_CXSMICON));
    upisi_u(&mut d.szTip, &t.opis);
    Shell_NotifyIconW(NIM_ADD, &d);
}

/// Windows obavijest (toast) iz ikone u trayu. Klik na nju poziva `na_obavijest(kljuc)`.
/// Može se pozvati s bilo koje niti.
pub fn obavijesti(naslov: &str, tekst: &str, kljuc: &str) {
    let hwnd = HWND_TRAY.load(Ordering::SeqCst) as HWND;
    if hwnd.is_null() {
        return;
    }
    *ZADNJA_OBAVIJEST.lock().unwrap_or_else(|e| e.into_inner()) = kljuc.to_string();
    let mut d = podaci_ikone(hwnd);
    d.uFlags = NIF_INFO;
    d.dwInfoFlags = NIIF_ERROR;
    upisi_u(&mut d.szInfoTitle, naslov);
    upisi_u(&mut d.szInfo, tekst);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
}

/// Skida ikonu iz traya (poziva se prije izlaska iz procesa, s bilo koje niti).
pub fn ukloni() {
    let hwnd = HWND_TRAY.load(Ordering::SeqCst) as HWND;
    if !hwnd.is_null() {
        let d = podaci_ikone(hwnd);
        unsafe { Shell_NotifyIconW(NIM_DELETE, &d) };
    }
}

unsafe fn prikazi_meni(hwnd: HWND) {
    let t = TRAY.get().unwrap();
    let meni = CreatePopupMenu();
    AppendMenuW(meni, MF_STRING | MF_GRAYED, 0, w(&t.opis).as_ptr());
    AppendMenuW(meni, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(meni, MF_STRING, ID_OTVORI, w("Otvori log i podešavanja").as_ptr());
    AppendMenuW(meni, MF_STRING, ID_FOLDER, w("Otvori folder").as_ptr());
    AppendMenuW(meni, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(meni, MF_STRING, ID_UGASI, w("Ugasi forwarder").as_ptr());
    SetMenuDefaultItem(meni, ID_OTVORI as u32, 0);

    let mut p = POINT { x: 0, y: 0 };
    GetCursorPos(&mut p);
    // Bez ovoga meni ne nestaje kad se klikne pored njega.
    SetForegroundWindow(hwnd);
    TrackPopupMenu(meni, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, p.x, p.y, 0, hwnd, std::ptr::null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(meni);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let t = TRAY.get().unwrap();
    match msg {
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONDBLCLK => (t.na_otvori)(),
                NIN_BALLOONUSERCLICK => {
                    let kljuc = ZADNJA_OBAVIJEST.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    (t.na_obavijest)(&kljuc);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => prikazi_meni(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xffff {
                ID_OTVORI => (t.na_otvori)(),
                ID_FOLDER => (t.na_folder)(),
                ID_UGASI => {
                    ukloni();
                    (t.na_ugasi)("ugašen iz traya");
                }
                _ => {}
            }
            0
        }
        // Explorer se restartovao — ikona se mora ponovo dodati.
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) => {
            dodaj_ikonu(hwnd);
            0
        }
        // taskkill bez /F (installer pri upgradeu/deinstalaciji) šalje WM_CLOSE.
        WM_CLOSE => {
            ukloni();
            (t.na_ugasi)("zatvoren (installer ili taskkill)");
            0
        }
        WM_ENDSESSION if wparam != 0 => {
            ukloni();
            (t.na_ugasi)("odjava ili gašenje Windowsa");
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Pokreće tray i vrti petlju poruka do kraja procesa.
pub fn pokreni(tray: Tray) {
    if TRAY.set(tray).is_err() {
        return;
    }
    unsafe {
        let hinst = GetModuleHandleW(std::ptr::null());
        let klasa = w("TornadoKasaTray");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinst;
        wc.lpszClassName = klasa.as_ptr();
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0, klasa.as_ptr(), klasa.as_ptr(), 0, 0, 0, 0, 0,
            std::ptr::null_mut(), std::ptr::null_mut(), hinst, std::ptr::null(),
        );
        HWND_TRAY.store(hwnd as isize, Ordering::SeqCst);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w("TaskbarCreated").as_ptr()), Ordering::SeqCst);
        dodaj_ikonu(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
