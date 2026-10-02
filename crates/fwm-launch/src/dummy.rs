//! A do-nothing process for the injection tests: it has a PDB (as every MSVC Rust
//! binary does) but none of Factorio's functions, so `fwm_init` should load,
//! verify the PDB, and then report the symbols as missing.

fn main() {
    let millis = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3000);
    std::thread::sleep(std::time::Duration::from_millis(millis));
}
