// Windows sitnice: autostart u registru, MessageBox, lokalno vrijeme.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, SYSTEMTIME};
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, SW_SHOWNORMAL};

    const RUN_KLJUC: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VRIJEDNOST: &str = "TornadoKasa";
    /// Ime do 0.1.3 (Kasa Forwarder); briše se da stari unos ne pokreće obrisani exe.
    const STARA_RUN_VRIJEDNOST: &str = "KasaForwarder";

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn poruka(naslov: &str, tekst: &str, greska: bool) {
        let ikona = if greska { MB_ICONERROR } else { MB_ICONINFORMATION };
        unsafe {
            MessageBoxW(std::ptr::null_mut(), w(tekst).as_ptr(), w(naslov).as_ptr(), MB_OK | ikona);
        }
    }

    pub fn ukljuci_autostart(komanda: &str) -> Result<(), String> {
        let podaci = w(komanda);
        let rez = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                w(RUN_KLJUC).as_ptr(),
                w(RUN_VRIJEDNOST).as_ptr(),
                REG_SZ,
                podaci.as_ptr().cast(),
                (podaci.len() * 2) as u32,
            )
        };
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, w(RUN_KLJUC).as_ptr(), w(STARA_RUN_VRIJEDNOST).as_ptr());
        }
        if rez == ERROR_SUCCESS { Ok(()) } else { Err(format!("upis u registar nije uspio (kod {rez})")) }
    }

    pub fn iskljuci_autostart() {
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, w(RUN_KLJUC).as_ptr(), w(RUN_VRIJEDNOST).as_ptr());
            RegDeleteKeyValueW(HKEY_CURRENT_USER, w(RUN_KLJUC).as_ptr(), w(STARA_RUN_VRIJEDNOST).as_ptr());
        }
    }

    pub fn autostart_ukljucen() -> bool {
        let mut velicina = 0u32;
        let rez = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w(RUN_KLJUC).as_ptr(),
                w(RUN_VRIJEDNOST).as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut velicina,
            )
        };
        rez == ERROR_SUCCESS
    }

    /// Otvara URL ili folder kao dvoklik u Exploreru.
    pub fn otvori(sta: &str) {
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                w("open").as_ptr(),
                w(sta).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }

    pub fn lokalno_vrijeme() -> String {
        let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut t) };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
        )
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn poruka(naslov: &str, tekst: &str, _greska: bool) {
        eprintln!("{naslov}: {tekst}");
    }
    pub fn ukljuci_autostart(_komanda: &str) -> Result<(), String> {
        Err("autostart postoji samo na Windowsu".into())
    }
    pub fn iskljuci_autostart() {}
    pub fn autostart_ukljucen() -> bool {
        false
    }
    pub fn otvori(sta: &str) {
        let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(program).arg(sta).spawn();
    }

    // struct tm iz libc (isti raspored na macOS-u i glibc-u).
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }

    extern "C" {
        fn localtime_r(t: *const i64, tm: *mut Tm) -> *mut Tm;
    }

    /// Isti format kao na Windowsu — prozor iz njega čita vrijeme i dnevnu statistiku.
    pub fn lokalno_vrijeme() -> String {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let s = d.as_secs() as i64;
        let mut t: Tm = unsafe { std::mem::zeroed() };
        unsafe { localtime_r(&s, &mut t) };
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.year + 1900, t.mon + 1, t.mday, t.hour, t.min, t.sec, d.subsec_millis()
        )
    }
}

pub use imp::*;
