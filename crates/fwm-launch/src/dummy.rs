//! A do-nothing process for the tests.
//!
//! - Injection tests: it has a PDB (as every MSVC Rust binary does) but none of
//!   Factorio's functions, so `fwm_init` should load, verify the PDB, and then
//!   report the symbols as missing.
//! - Steam-switch tests: copied in as a fake `steam.exe`. `-shutdown` exits at
//!   once, like asking Steam to close; otherwise it stays up for the given
//!   milliseconds, `%FWM_DUMMY_MS%`, or 3 s.

fn main() {
    let arg = std::env::args().nth(1);
    if arg.as_deref() == Some("-shutdown") {
        return;
    }
    let millis = arg
        .and_then(|a| a.parse().ok())
        .or_else(|| std::env::var("FWM_DUMMY_MS").ok()?.parse().ok())
        .unwrap_or(3000);
    std::thread::sleep(std::time::Duration::from_millis(millis));
}
