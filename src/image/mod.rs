mod builder;

use crate::UnwindError;
use addr2line::Context as LineCtx;
use findshlibs::{SharedLibrary, TargetSharedLibrary};
use gimli::{Dwarf, EndianSlice, RunTimeEndian};
use memmap::Mmap;
use object::{File as ObjFile, Object, ObjectSection, SymbolMap, SymbolMapEntry, SymbolMapName};
use std::fs::File;
use std::mem::ManuallyDrop;

pub struct Image<'a> {
    pub filename: String,
    pub base_addresses: gimli::BaseAddresses,
    pub bias: usize,
    pub start_address: usize,
    pub length: usize,
    pub symbol_map: SymbolMap<OwnedSymbolMapName>,
    pub dwarf: Dwarf<Vec<u8>>,
    pub address_context: Option<LineCtx<ImageReader<'a>>>,
}

impl<'a> Image<'a> {
    pub fn has(&self, avma: usize) -> bool {
        self.start_address <= avma && avma < self.start_address + self.length
    }
}

#[derive(Debug)]
pub struct OwnedSymbolMapName {
    address: u64,
    name: String,
}

impl OwnedSymbolMapName {
    pub fn new<S: AsRef<str>>(address: u64, name: S) -> Self {
        OwnedSymbolMapName {
            address,
            name: name.as_ref().to_string(),
        }
    }

    /// The symbol address.
    #[inline]
    pub fn address(&self) -> u64 {
        self.address
    }

    /// The symbol name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn from(origin: &SymbolMapName) -> Self {
        Self::new(origin.address(), origin.name())
    }
}

impl SymbolMapEntry for OwnedSymbolMapName {
    #[inline]
    fn address(&self) -> u64 {
        self.address
    }
}

pub type ImageReader<'a> = gimli::EndianSlice<'a, gimli::RunTimeEndian>;

pub fn init_images<'a>() -> Vec<Image<'a>> {
    let mut vec = Vec::new();
    TargetSharedLibrary::each(|x| {
        if let Ok((object, mmap, file)) = File::open(x.name())
            .map(ManuallyDrop::new)
            .map_err(UnwindError::from)
            .and_then(|f| unsafe { Ok((ManuallyDrop::new(Mmap::map(&f)?), f)) })
            .and_then(|(m, f)| unsafe {
                Ok((
                    ObjFile::parse(std::slice::from_raw_parts(m.as_ptr(), m.len()))?,
                    m,
                    f,
                ))
            })
        {
            if let Some(base_addresses) = builder::build(&object) {
                let symbol_map = SymbolMap::new(
                    object
                        .symbol_map()
                        .symbols()
                        .into_iter()
                        .map(OwnedSymbolMapName::from)
                        .collect(),
                );

                let dwarf = Dwarf::load(|id| -> Result<Vec<u8>, gimli::Error> {
                    Ok(object
                        .section_by_name(id.name())
                        .and_then(|x| x.uncompressed_data().ok())
                        .map(|x| x.to_vec())
                        .unwrap_or_else(Default::default))
                })
                .ok()
                .unwrap_or_else(Default::default);

                let dwarf_slice: Dwarf<EndianSlice<'a, RunTimeEndian>> = {
                    dwarf.borrow(|data| unsafe {
                        EndianSlice::new(
                            std::slice::from_raw_parts(data.as_ptr(), data.len()),
                            if object.is_little_endian() {
                                RunTimeEndian::Little
                            } else {
                                RunTimeEndian::Big
                            },
                        )
                    })
                };

                let address_context = LineCtx::from_dwarf(dwarf_slice).ok();

                vec.push(Image {
                    filename: x.name().to_string_lossy().to_string(),
                    base_addresses,
                    bias: x.virtual_memory_bias().0,
                    start_address: x.actual_load_addr().0,
                    length: x.len(),
                    symbol_map,
                    dwarf,
                    address_context,
                });
            }
            ManuallyDrop::into_inner(mmap);
            ManuallyDrop::into_inner(file);
        }
    });
    vec
}
