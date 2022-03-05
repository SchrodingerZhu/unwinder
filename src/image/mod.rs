mod builder;

use crate::UnwindError;
use findshlibs::SharedLibrary;
use gimli::Dwarf;
use object::{Object, ObjectSection, SymbolMap, SymbolMapEntry, SymbolMapName};
use std::mem::ManuallyDrop;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
use linux::Builder as ImageBuilder;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
use macos::Builder as ImageBuilder;

pub struct Image<'a> {
    pub filename: String,
    pub base_addresses: gimli::BaseAddresses,
    pub bias: usize,
    pub start_address: usize,
    pub length: usize,
    pub symbol_map: SymbolMap<OwnedSymbolMapName>,
    pub dwarf: gimli::Dwarf<Vec<u8>>,
    pub address_context: Option<addr2line::Context<ImageReader<'a>>>,
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
    use builder::Builder;

    let mut vec = Vec::new();
    findshlibs::TargetSharedLibrary::each(|x| unsafe {
        if let Ok((object, mmap, file)) = std::fs::File::open(x.name())
            .map(ManuallyDrop::new)
            .map_err(UnwindError::from)
            .and_then(|f| Ok((ManuallyDrop::new(memmap::Mmap::map(&f)?), f)))
            .and_then(|(m, f)| {
                Ok((
                    object::File::parse(std::slice::from_raw_parts(m.as_ptr(), m.len()))?,
                    m,
                    f,
                ))
            })
        {
            if let Some(base_addresses) = ImageBuilder::build(&object) {
                let symbol_map = SymbolMap::new(
                    object
                        .symbol_map()
                        .symbols()
                        .into_iter()
                        .map(|x| OwnedSymbolMapName::from(x))
                        .collect(),
                );
                let endian = if object.is_little_endian() {
                    gimli::RunTimeEndian::Little
                } else {
                    gimli::RunTimeEndian::Big
                };
                let dwarf = Dwarf::load(|id| -> Result<Vec<u8>, gimli::Error> {
                    Ok(object
                        .section_by_name(id.name())
                        .and_then(|x| x.uncompressed_data().ok())
                        .map(|x| x.to_vec())
                        .unwrap_or_else(Default::default))
                })
                .ok()
                .unwrap_or_else(Default::default);

                let dwarf_slice: gimli::Dwarf<gimli::EndianSlice<'a, gimli::RunTimeEndian>> = {
                    dwarf.borrow(|data| {
                        gimli::EndianSlice::new(
                            std::slice::from_raw_parts(data.as_ptr(), data.len()),
                            endian,
                        )
                    })
                };

                let address_context = addr2line::Context::from_dwarf(dwarf_slice).ok();

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
