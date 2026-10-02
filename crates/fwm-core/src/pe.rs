//! Reads the CodeView (RSDS) record from a PE image: the GUID and age that tie an
//! exe to the exact PDB it was built with. The hook refuses to patch anything
//! unless factorio.pdb carries the same GUID as the running factorio.exe.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeView {
    /// In the byte order RSDS stores it (a Windows GUID).
    pub guid: [u8; 16],
    pub age: u32,
    pub pdb_path: String,
}

const IMAGE_DEBUG_TYPE_CODEVIEW: u32 = 2;
const DEBUG_DIRECTORY_INDEX: usize = 6;

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

struct Headers {
    sections: Vec<Section>,
    size_of_image: u32,
    debug_rva: u32,
    debug_size: u32,
}

struct Section {
    virtual_address: u32,
    virtual_size: u32,
    raw_size: u32,
    raw_pointer: u32,
}

fn parse_headers(b: &[u8]) -> Option<Headers> {
    if b.get(0..2)? != b"MZ" {
        return None;
    }
    let pe = u32_at(b, 0x3c)? as usize;
    if b.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let section_count = u16_at(b, coff + 2)? as usize;
    let optional_size = u16_at(b, coff + 16)? as usize;
    let optional = coff + 20;
    if u16_at(b, optional)? != 0x20b {
        return None; // only PE32+ (64-bit) images
    }
    let size_of_image = u32_at(b, optional + 56)?;
    let directory_count = u32_at(b, optional + 108)? as usize;
    let (debug_rva, debug_size) = if directory_count > DEBUG_DIRECTORY_INDEX {
        let entry = optional + 112 + DEBUG_DIRECTORY_INDEX * 8;
        (u32_at(b, entry)?, u32_at(b, entry + 4)?)
    } else {
        (0, 0)
    };
    let table = optional + optional_size;
    let sections = (0..section_count)
        .map(|i| {
            let s = table + i * 40;
            Some(Section {
                virtual_size: u32_at(b, s + 8)?,
                virtual_address: u32_at(b, s + 12)?,
                raw_size: u32_at(b, s + 16)?,
                raw_pointer: u32_at(b, s + 20)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Headers {
        sections,
        size_of_image,
        debug_rva,
        debug_size,
    })
}

fn parse_rsds(b: &[u8]) -> Option<CodeView> {
    if b.get(0..4)? != b"RSDS" {
        return None;
    }
    let guid: [u8; 16] = b.get(4..20)?.try_into().ok()?;
    let age = u32_at(b, 20)?;
    let path = b.get(24..)?;
    let end = path.iter().position(|&c| c == 0).unwrap_or(path.len());
    Some(CodeView {
        guid,
        age,
        pdb_path: String::from_utf8_lossy(&path[..end]).into_owned(),
    })
}

/// Walk the debug directory, where `locate(entry)` returns the offset of an entry's data in `b`.
fn find_codeview(
    b: &[u8],
    directory: usize,
    size: usize,
    locate: impl Fn(&[u8], usize) -> Option<usize>,
) -> Option<CodeView> {
    (0..size / 28).find_map(|i| {
        let entry = directory + i * 28;
        if u32_at(b, entry + 12)? != IMAGE_DEBUG_TYPE_CODEVIEW {
            return None;
        }
        let len = u32_at(b, entry + 16)? as usize;
        let start = locate(b, entry)?;
        parse_rsds(b.get(start..start + len)?)
    })
}

/// From the bytes of a PE file on disk.
pub fn codeview_from_file(b: &[u8]) -> Option<CodeView> {
    let headers = parse_headers(b)?;
    let rva_to_offset = |rva: u32| {
        headers.sections.iter().find_map(|s| {
            let span = s.virtual_size.max(s.raw_size);
            (rva >= s.virtual_address && rva < s.virtual_address + span)
                .then(|| (rva - s.virtual_address + s.raw_pointer) as usize)
        })
    };
    let directory = rva_to_offset(headers.debug_rva)?;
    // PointerToRawData: the entry's data as a file offset.
    find_codeview(b, directory, headers.debug_size as usize, |b, entry| {
        u32_at(b, entry + 24).map(|o| o as usize)
    })
}

/// `SizeOfImage` of a module mapped at `base`.
///
/// # Safety
/// `base` must be the base address of a loaded PE module.
pub unsafe fn mapped_image_size(base: *const u8) -> Option<usize> {
    let header_page = std::slice::from_raw_parts(base, 4096);
    Some(parse_headers(header_page)?.size_of_image as usize)
}

/// From a module mapped at `base` (e.g. `GetModuleHandleW(NULL)`).
///
/// # Safety
/// `base` must be the base address of a loaded PE module.
pub unsafe fn codeview_from_mapped(base: *const u8) -> Option<CodeView> {
    let size = mapped_image_size(base)?;
    let image = std::slice::from_raw_parts(base, size);
    let headers = parse_headers(image)?;
    // AddressOfRawData: the entry's data as an RVA.
    find_codeview(
        image,
        headers.debug_rva as usize,
        headers.debug_size as usize,
        |b, entry| u32_at(b, entry + 20).map(|o| o as usize),
    )
}

/// `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` for logs.
pub fn format_guid(g: &[u8; 16]) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{}",
        u32::from_le_bytes(g[0..4].try_into().unwrap()),
        u16::from_le_bytes(g[4..6].try_into().unwrap()),
        u16::from_le_bytes(g[6..8].try_into().unwrap()),
        g[8],
        g[9],
        g[10..]
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_pe_data() {
        assert_eq!(codeview_from_file(b"not a PE file at all"), None);
        assert_eq!(codeview_from_file(&[]), None);
    }

    #[test]
    fn reads_the_test_binary_itself() {
        // Every MSVC-target Rust binary has an RSDS record pointing at its PDB.
        let exe = std::env::current_exe().unwrap();
        let cv = codeview_from_file(&std::fs::read(&exe).unwrap())
            .expect("test exe has a CodeView record");
        assert!(
            cv.pdb_path.to_ascii_lowercase().ends_with(".pdb"),
            "{}",
            cv.pdb_path
        );
        assert_ne!(cv.guid, [0; 16]);
    }

    #[test]
    fn mapped_and_file_views_agree() {
        use std::os::raw::c_void;
        extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        }
        let exe = std::env::current_exe().unwrap();
        let from_file = codeview_from_file(&std::fs::read(&exe).unwrap());
        let from_memory =
            unsafe { codeview_from_mapped(GetModuleHandleW(std::ptr::null()) as *const u8) };
        assert!(from_file.is_some());
        assert_eq!(from_file, from_memory);
    }

    #[test]
    fn guid_formats_like_windows() {
        let g = [
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD,
            0xEE, 0xFF,
        ];
        assert_eq!(format_guid(&g), "00112233-4455-6677-8899-AABBCCDDEEFF");
    }
}
