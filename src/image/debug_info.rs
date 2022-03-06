use gimli::Dwarf;
use object::{File, Object, ObjectSection};

pub type RawDebugInfo = Dwarf<Vec<u8>>;

pub fn load(f: &File) -> RawDebugInfo {
    if !f.has_debug_symbols() {
        todo!("Load dwarf symbols from dSYM directory");
    }

    Dwarf::load(|id| -> Result<Vec<u8>, gimli::Error> {
        Ok(f.section_by_name(id.name())
            .and_then(|x| x.uncompressed_data().ok())
            .map(|x| x.to_vec())
            .unwrap_or_else(Default::default))
    })
    .ok()
    .unwrap_or_else(Default::default)
}
