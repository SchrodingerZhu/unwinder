#![feature(rustc_private)]
#![feature(llvm_asm)]

use findshlibs::{Segment, SharedLibrary};
use gimli::{
    Dwarf, Reader, Register, RegisterRule, UnwindContext, UnwindContextStorage, UnwindTableRow,
};
use memmap::Mmap;
use object::{Object, ObjectSection, ReadRef, SymbolMap};
use std::fmt::{write, Display, Formatter};
use std::io::Read;
use std::{borrow, fs, slice};

struct StoreOnStack;

#[derive(thiserror::Error, Debug)]
enum UnwindError {
    IOError(#[from] std::io::Error),
    ObjectParsingError(#[from] object::Error),
}

impl Display for UnwindError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl<R: Reader> UnwindContextStorage<R> for StoreOnStack {
    type Rules = [(Register, RegisterRule<R>); 192];
    type Stack = [UnwindTableRow<R, Self>; 32];
}

type Context<R> = UnwindContext<R, StoreOnStack>;

#[derive(Debug)]
struct Image<'a> {
    filename: String,
    file: std::fs::File,
    mmap: Mmap,
    object: object::File<'a, &'a [u8]>,
    base_addresses: gimli::BaseAddresses,
    bias: usize,
    start_address: usize,
    length: usize,
    symbol_map: SymbolMap<object::SymbolMapName<'a>>,
}

type MmapReader<'a> = gimli::EndianSlice<'a, gimli::RunTimeEndian>;

struct GlobalContext<'a> {
    images: Vec<Image<'a>>,
    dwarfs: Vec<gimli::Dwarf<borrow::Cow<'a, [u8]>>>,
    address_contexts: Vec<addr2line::Context<MmapReader<'a>>>,
}

fn init_images<'a>() -> Vec<Image<'a>> {
    let mut vec = Vec::new();
    findshlibs::TargetSharedLibrary::each(|x| unsafe {
        if let Ok((object, mmap, file)) = fs::File::open(x.name())
            .map_err(UnwindError::from)
            .and_then(|f| Ok((memmap::Mmap::map(&f)?, f)))
            .and_then(|(m, f)| {
                Ok((
                    object::File::parse(slice::from_raw_parts(m.as_ptr(), m.len()))?,
                    m,
                    f,
                ))
            })
        {
            if let Some(base_addresses) = Some(gimli::BaseAddresses::default())
                .and_then(|acc| {
                    object
                        .section_by_name(".text")
                        .map(|x| acc.set_text(x.address()))
                })
                .and_then(|acc| {
                    object
                        .section_by_name(".eh_frame")
                        .map(|x| acc.set_eh_frame(x.address()))
                })
                .and_then(|acc| {
                    object
                        .section_by_name(".eh_frame_hdr")
                        .map(|x| acc.set_eh_frame_hdr(x.address()))
                })
                .and_then(|acc| {
                    object
                        .section_by_name(".got")
                        .map(|x| acc.set_got(x.address()))
                })
            {
                let symbol_map = object.symbol_map();
                vec.push(Image {
                    filename: x.name().to_string_lossy().to_string(),
                    file,
                    mmap,
                    object,
                    base_addresses,
                    bias: x.virtual_memory_bias().0,
                    start_address: x.actual_load_addr().0,
                    length: x.len(),
                    symbol_map,
                });
            }
        }
    });
    vec
}

fn init_global_context<'a>() -> GlobalContext<'a> {
    let images = init_images();
    let mut dwarfs = Vec::new();
    for image in &images {
        if let Ok(dwarf) = Dwarf::load(|id| -> Result<borrow::Cow<[u8]>, gimli::Error> {
            Ok(image
                .object
                .section_by_name(id.name())
                .and_then(|x| x.uncompressed_data().ok())
                .unwrap_or_else(Default::default))
        }) {
            dwarfs.push(dwarf);
        } else {
            dwarfs.push(Default::default())
        };
    }

    let endian = images
        .first()
        .and_then(|img| {
            if img.object.is_little_endian() {
                return Some(gimli::RunTimeEndian::Little);
            }
            None
        })
        .unwrap_or(gimli::RunTimeEndian::Big);

    let dwarf_slices: Vec<gimli::Dwarf<gimli::EndianSlice<'static, gimli::RunTimeEndian>>> = unsafe {
        dwarfs
            .iter()
            .map(|x| {
                x.borrow(|data| {
                    gimli::EndianSlice::new(
                        slice::from_raw_parts(data.as_ptr(), data.len()),
                        endian,
                    )
                })
            })
            .collect()
    };

    let address_contexts = {
        dwarf_slices
            .into_iter()
            .map(|x| addr2line::Context::from_dwarf(x).unwrap())
            .collect()
    };

    GlobalContext {
        images,
        dwarfs,
        address_contexts,
    }
}

enum Frame<'a> {
    Dwarf(addr2line::Frame<'a, MmapReader<'a>>),
    SymbolMap(&'a str),
}

struct SymbolInfo<'a> {
    dynamic_address: usize,
    object_name: Option<&'a str>,
    static_address: Option<usize>,
    associated_frames: Vec<Frame<'a>>,
}

impl<'a> GlobalContext<'a> {
    fn resolve_symbol(&'a self, address: usize) -> SymbolInfo<'a> {
        let mut symbol = SymbolInfo {
            dynamic_address: address,
            object_name: None,
            static_address: None,
            associated_frames: Vec::new(),
        };
        for i in 0..self.images.len() {
            let image = &self.images[i];
            if address >= image.start_address && address < image.start_address + image.length {
                let static_address = address - image.bias;
                symbol.static_address.replace(static_address);
                symbol.object_name.replace(&image.filename);
                if let Ok(mut frames) = self.address_contexts[i].find_frames(static_address as u64)
                {
                    while let Ok(Some(frame)) = frames.next() {
                        symbol.associated_frames.push(Frame::Dwarf(frame));
                    }
                }
                if symbol.associated_frames.len() == 0 {
                    // Find the symbol at the current address
                    let elf_symbol = image.symbol_map.get(static_address as u64);

                    if let Some(elf_symbol) = elf_symbol {
                        symbol
                            .associated_frames
                            .push(Frame::SymbolMap(elf_symbol.name()));
                    }
                }
                break;
            }
        }

        symbol
    }
}

#[cfg(test)]
mod tests {
    use crate::{init_global_context, init_images, Frame};
    use libc::c_void;
    use object::Object;

    #[test]
    fn it_works() {
        init_global_context();
    }

    #[test]
    fn it_resolves() {
        let g = init_global_context();
        let resolved = g.resolve_symbol(it_resolves as usize);
        println!("addr: {:?}", resolved.dynamic_address);
        println!("static addr: {:?}", resolved.static_address);
        println!("object: {:?}", resolved.object_name);
        for i in resolved.associated_frames {
            match i {
                Frame::Dwarf(frame) => {
                    println!(
                        "dwarf name: {:?}",
                        frame
                            .function
                            .as_ref()
                            .and_then(|x| x.name.to_string().ok())
                    );
                    println!(
                        "dwarf file: {:?}",
                        frame.location.as_ref().and_then(|x| x.file)
                    );
                    println!(
                        "dwarf line: {:?}",
                        frame.location.as_ref().and_then(|x| x.line)
                    );
                    println!(
                        "dwarf column: {:?}",
                        frame.location.as_ref().and_then(|x| x.column)
                    );
                }
                Frame::SymbolMap(symbol) => {
                    println!("symbol map name: {}", symbol);
                }
            }
        }
    }
}
