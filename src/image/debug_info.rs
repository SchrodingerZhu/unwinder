use crate::image::raw_image;
use gimli::Dwarf;
use object::{File, Object, ObjectSection};
use std::mem::ManuallyDrop;
use std::path::Path;

pub type RawDebugInfo = Dwarf<Vec<u8>>;

pub fn load<T: AsRef<Path>>(p: T, f: &File) -> RawDebugInfo {
    if f.has_debug_symbols() {
        return load_dwarf(f);
    }

    if let Ok(Some(uuid)) = f.mach_uuid() {
        if let Ok(f) = locate_dwarf::locate_dsym(p, uuid) {
            if let Ok((obj, m, f)) = raw_image::load(f) {
                let info = load_dwarf(&obj);
                ManuallyDrop::into_inner(m);
                ManuallyDrop::into_inner(f);
                return info;
            }
        }
    }

    Default::default()
}

fn load_dwarf(f: &File) -> RawDebugInfo {
    Dwarf::load(|id| -> Result<Vec<u8>, gimli::Error> {
        Ok(f.section_by_name(id.name())
            .and_then(|x| x.uncompressed_data().ok())
            .map(|x| x.to_vec())
            .unwrap_or_else(Default::default))
    })
    .ok()
    .unwrap_or_else(Default::default)
}
