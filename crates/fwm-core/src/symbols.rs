//! Looks up function addresses (as RVAs) in factorio.pdb by mangled name.

use pdb::FallibleIterator;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    pub rva: u32,
    /// Another public symbol has the same address: the linker folded identical
    /// functions together, so patching this one would patch the others too.
    pub shared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdbIdentity {
    /// In RSDS byte order, comparable with [`crate::pe::CodeView::guid`].
    pub guid: [u8; 16],
    pub age: u32,
}

#[derive(Debug)]
pub struct SymbolTable {
    pub identity: PdbIdentity,
    symbols: HashMap<String, Symbol>,
}

impl SymbolTable {
    pub fn get(&self, name: &str) -> Option<Symbol> {
        self.symbols.get(name).copied()
    }

    pub fn missing<'a>(&self, names: &[&'a str]) -> Vec<&'a str> {
        names
            .iter()
            .copied()
            .filter(|n| !self.symbols.contains_key(*n))
            .collect()
    }
}

#[derive(Debug)]
pub enum SymbolError {
    Io(std::io::Error),
    Pdb(pdb::Error),
}

impl fmt::Display for SymbolError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SymbolError::Io(e) => write!(f, "{e}"),
            SymbolError::Pdb(e) => write!(f, "bad PDB: {e}"),
        }
    }
}

impl From<std::io::Error> for SymbolError {
    fn from(e: std::io::Error) -> Self {
        SymbolError::Io(e)
    }
}

impl From<pdb::Error> for SymbolError {
    fn from(e: pdb::Error) -> Self {
        SymbolError::Pdb(e)
    }
}

/// Scan the public symbols of `path` for the `wanted` mangled names.
pub fn load(path: &Path, wanted: &[&str]) -> Result<SymbolTable, SymbolError> {
    let file = std::fs::File::open(path)?;
    let mut pdb = pdb::PDB::open(std::io::BufReader::with_capacity(1 << 20, file))?;

    let info = pdb.pdb_information()?;
    let (d1, d2, d3, d4) = info.guid.as_fields();
    let mut guid = [0u8; 16];
    guid[0..4].copy_from_slice(&d1.to_le_bytes());
    guid[4..6].copy_from_slice(&d2.to_le_bytes());
    guid[6..8].copy_from_slice(&d3.to_le_bytes());
    guid[8..16].copy_from_slice(d4);
    // The exe's RSDS age matches the DBI stream's age, not the info stream's.
    let age = pdb
        .debug_information()
        .ok()
        .and_then(|d| d.age())
        .unwrap_or(info.age);

    let wanted: HashSet<&str> = wanted.iter().copied().collect();
    let address_map = pdb.address_map()?;
    let globals = pdb.global_symbols()?;
    let mut iter = globals.iter();
    let mut found: HashMap<String, u32> = HashMap::new();
    let mut per_address: HashMap<u32, u32> = HashMap::new();
    while let Some(symbol) = iter.next()? {
        let Ok(pdb::SymbolData::Public(public)) = symbol.parse() else {
            continue;
        };
        let Some(rva) = public.offset.to_rva(&address_map) else {
            continue;
        };
        *per_address.entry(rva.0).or_default() += 1;
        let name = public.name.to_string();
        if wanted.contains(name.as_ref()) {
            found.insert(name.into_owned(), rva.0);
        }
    }

    let symbols = found
        .into_iter()
        .map(|(name, rva)| {
            (
                name,
                Symbol {
                    rva,
                    shared: per_address.get(&rva).copied().unwrap_or(0) > 1,
                },
            )
        })
        .collect();
    Ok(SymbolTable {
        identity: PdbIdentity { guid, age },
        symbols,
    })
}
