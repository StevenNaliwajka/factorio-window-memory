//! Startup: find factorio.pdb, check it matches the running exe, resolve the
//! functions, and install the hooks. Any failure leaves the game untouched.

use crate::game::{find_vtable_slot, Game};
use crate::hooks::{self, HookSpec, Kind};
use fwm_core::protocol::{code, default_data_dir, InitConfig};
use fwm_core::store::{LoadOutcome, Store};
use fwm_core::{pe, symbols, targets};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

pub unsafe fn run(param: *const u16) -> u32 {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return code::ALREADY_LOADED;
    }

    let config = if param.is_null() {
        InitConfig::default()
    } else {
        InitConfig::decode(&read_wide(param))
    };
    let verbose = config.verbose || std::env::var_os("FWM_VERBOSE").is_some_and(|v| v == "1");
    let Some(data_dir) = config.data_dir.clone().or_else(default_data_dir) else {
        return code::DATA_DIR;
    };
    if std::fs::create_dir_all(&data_dir).is_err() {
        return code::DATA_DIR;
    }
    crate::log::open(&data_dir.join("fwm.log"));
    log!(
        "factorio-window-memory {} in pid {} ({}, verbose={verbose})",
        env!("CARGO_PKG_VERSION"),
        GetCurrentProcessId(),
        if config.attach {
            "attached to a running game"
        } else {
            "loaded at game start"
        }
    );

    let base = GetModuleHandleW(std::ptr::null()) as usize;
    let exe = module_path();
    let Some(image_size) = pe::mapped_image_size(base as *const u8) else {
        log!("can't read the headers of {}", exe.display());
        return code::PDB_READ;
    };
    let codeview = pe::codeview_from_mapped(base as *const u8);
    let Some(pdb_path) = find_pdb(&exe, codeview.as_ref()) else {
        log!("no PDB found for {}", exe.display());
        return code::PDB_READ;
    };

    let started = Instant::now();
    let wanted: Vec<&str> = targets::all().collect();
    let table = match symbols::load(&pdb_path, &wanted) {
        Ok(table) => table,
        Err(e) => {
            log!("couldn't read {}: {e}", pdb_path.display());
            return code::PDB_READ;
        }
    };
    log!(
        "read {} in {:.2}s",
        pdb_path.display(),
        started.elapsed().as_secs_f64()
    );

    let Some(codeview) = codeview else {
        log!(
            "{} has no CodeView record, so its PDB can't be verified",
            exe.display()
        );
        return code::PDB_MISMATCH;
    };
    if codeview.guid != table.identity.guid {
        log!(
            "PDB doesn't match the exe: exe expects {} age {}, PDB is {} age {}",
            pe::format_guid(&codeview.guid),
            codeview.age,
            pe::format_guid(&table.identity.guid),
            table.identity.age
        );
        return code::PDB_MISMATCH;
    }
    if codeview.age != table.identity.age {
        log!(
            "note: PDB age {} differs from the exe's {}",
            table.identity.age,
            codeview.age
        );
    }

    let missing = table.missing(targets::REQUIRED);
    if !missing.is_empty() {
        log!("missing symbols: {}", missing.join(", "));
        return code::SYMBOLS_MISSING;
    }

    let address = |name: &str| table.get(name).map(|s| base + s.rva as usize);
    let hookable = |name: &str| match table.get(name) {
        Some(s) if s.shared => {
            log!("not hooking {name}: its code is shared with other functions");
            None
        }
        Some(s) => Some(base + s.rva as usize),
        None => None,
    };

    let drag_target_slot = match (
        address(targets::FRAME_VFTABLE),
        address(targets::FRAME_GET_DRAG_TARGET),
    ) {
        (Some(vtable), Some(function)) => {
            find_vtable_slot(vtable, function, 512, base, base + image_size)
        }
        _ => None,
    };
    log!(
        "getDragTarget vtable slot: {}",
        drag_target_slot.map_or("not found".to_owned(), |s| s.to_string())
    );

    let game = Game::new(
        base,
        image_size,
        address(targets::WIDGET_GET_LOCATION).unwrap(),
        address(targets::WIDGET_SET_LOCATION).unwrap(),
        address(targets::WIDGET_GET_ABSOLUTE_RECTANGLE).unwrap(),
        drag_target_slot,
    );

    let (store, outcome) = Store::load(data_dir.join("positions.json"));
    match outcome {
        LoadOutcome::Missing => log!("no saved positions yet ({})", store.path().display()),
        LoadOutcome::Loaded { windows } => log!(
            "loaded {windows} saved positions from {}",
            store.path().display()
        ),
        LoadOutcome::Corrupt { backup, error } => {
            log!(
                "positions file didn't parse ({error}); moved it to {} and started fresh",
                backup.display()
            )
        }
        LoadOutcome::Unreadable { error } => {
            log!("couldn't read {}: {error}", store.path().display())
        }
    }

    let mut specs = Vec::new();
    for (kind, name) in [
        (Kind::Center, targets::WINDOW_CENTER),
        (Kind::WindowDrag, targets::WINDOW_MOUSE_DRAG),
    ] {
        let Some(target) = hookable(name) else {
            return code::HOOK_FAILED;
        };
        specs.push(HookSpec {
            kind,
            target,
            required: true,
        });
    }
    if drag_target_slot.is_some() {
        for (kind, name) in [
            (Kind::WidgetDrag, targets::WIDGET_MOUSE_DRAG),
            (Kind::LabelDrag, targets::LABEL_MOUSE_DRAG),
            (Kind::LayoutDrag, targets::LAYOUT_MOUSE_DRAG),
            (Kind::EmptyWidgetDrag, targets::EMPTY_WIDGET_MOUSE_DRAG),
        ] {
            if let Some(target) = hookable(name) {
                specs.push(HookSpec {
                    kind,
                    target,
                    required: false,
                });
            }
        }
    }

    match hooks::install(game, store, verbose, &specs) {
        Ok(installed) => {
            let names: Vec<&str> = installed.iter().map(|k| k.name()).collect();
            log!("hooks installed: {}", names.join(", "));
            code::OK
        }
        Err(e) => {
            log!("hooking failed: {e}");
            code::HOOK_FAILED
        }
    }
}

unsafe fn read_wide(p: *const u16) -> String {
    let mut len = 0;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

unsafe fn module_path() -> PathBuf {
    let mut buffer = vec![0u16; 32768];
    let len = GetModuleFileNameW(
        std::ptr::null_mut(),
        buffer.as_mut_ptr(),
        buffer.len() as u32,
    ) as usize;
    PathBuf::from(OsString::from_wide(&buffer[..len]))
}

/// `%FWM_PDB%`, else factorio.pdb beside factorio.exe, else wherever the exe says its PDB is.
fn find_pdb(exe: &Path, codeview: Option<&pe::CodeView>) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FWM_PDB") {
        return Some(p.into());
    }
    let mut candidates = vec![exe.with_extension("pdb")];
    if let Some(cv) = codeview {
        let recorded = PathBuf::from(&cv.pdb_path);
        if let (Some(dir), Some(name)) = (exe.parent(), recorded.file_name()) {
            candidates.push(dir.join(name));
        }
        candidates.push(recorded);
    }
    candidates.into_iter().find(|p| p.is_file())
}
