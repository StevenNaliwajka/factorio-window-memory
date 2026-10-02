//! Checks the installed game still has everything the hook needs. Skips (passes
//! with a note) when Factorio isn't installed.

use fwm_core::{pe, steam, symbols, targets};

#[test]
fn installed_factorio_has_every_hook_target() {
    let Some(exe) = steam::find_factorio_exe() else {
        eprintln!("skipping: Factorio isn't installed");
        return;
    };
    let pdb = exe.with_extension("pdb");
    let wanted: Vec<&str> = targets::all().collect();
    let table = symbols::load(&pdb, &wanted).unwrap_or_else(|e| panic!("{}: {e}", pdb.display()));

    assert_eq!(table.missing(targets::REQUIRED), Vec::<&str>::new());
    assert_eq!(table.missing(targets::OPTIONAL), Vec::<&str>::new());
    // The hook refuses to patch folded functions. That's fatal for the two it
    // can't work without, and just skips the optional title-bar hooks.
    for name in [targets::WINDOW_CENTER, targets::WINDOW_MOUSE_DRAG] {
        assert!(
            !table.get(name).unwrap().shared,
            "{name} shares its address with another function"
        );
    }
    let skipped: Vec<&str> = targets::HOOKED
        .iter()
        .copied()
        .filter(|n| table.get(n).unwrap().shared)
        .collect();
    eprintln!("optional hooks the DLL will skip as folded: {skipped:?}");
    assert!(
        skipped.len() < targets::HOOKED.len() - 2,
        "every title-bar drag hook is folded"
    );

    let codeview = pe::codeview_from_file(&std::fs::read(&exe).unwrap())
        .expect("factorio.exe has a CodeView record");
    assert_eq!(
        pe::format_guid(&codeview.guid),
        pe::format_guid(&table.identity.guid),
        "PDB GUID"
    );
    assert_eq!(codeview.age, table.identity.age, "PDB age");
}
