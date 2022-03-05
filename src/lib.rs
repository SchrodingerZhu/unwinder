use std::fmt::{Display, Formatter, write};
use std::{borrow, fs, slice};
use std::io::Read;
use gimli::{Dwarf, Reader, Register, RegisterRule, UnwindContext, UnwindContextStorage, UnwindTableRow};
use findshlibs::{Segment, SharedLibrary};
use memmap::Mmap;
use object::{Object, ObjectSection, ReadRef};
use object::SectionKind::Debug;

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


fn from_ucontext(ucontext: &libc::ucontext_t) {}

#[derive(Debug)]
struct Image {
    file: std::fs::File,
    mmap: Mmap,
    object: object::File<'static, &'static [u8]>,
    base_addresses: gimli::BaseAddresses,
    bias: usize,
    start_address: usize,
    length: usize
}

struct GlobalContext {
    images: Vec<Image>,
    dwarfs: Vec<gimli::Dwarf<borrow::Cow<'static, [u8]>>>,
    address_contexts: Vec<addr2line::Context<gimli::EndianSlice<'static, gimli::RunTimeEndian>>>,
}


fn init_images() -> Vec<Image> {
    let mut vec = Vec::new();
    findshlibs::TargetSharedLibrary::each(|x| unsafe {
        if let Ok((object, mmap, file)) = fs::File::open(x.name())
            .map_err(UnwindError::from)
            .and_then(|f| Ok((memmap::Mmap::map(&f)?, f)))
            .and_then(|(m, f)| Ok((object::File::parse(slice::from_raw_parts(m.as_ptr(), m.len()))?, m, f))) {
            if let Some(base_addresses) = Some(gimli::BaseAddresses::default())
                .and_then(|acc| object.section_by_name(".text").map(|x| acc.set_text(x.address())))
                .and_then(|acc| object.section_by_name(".eh_frame").map(|x| acc.set_eh_frame(x.address())))
                .and_then(|acc| object.section_by_name(".eh_frame_hdr").map(|x| acc.set_eh_frame_hdr(x.address())))
                .and_then(|acc| object.section_by_name(".got").map(|x| acc.set_got(x.address()))) {
                vec.push(Image {
                    file,
                    mmap,
                    object,
                    base_addresses,
                    bias: x.virtual_memory_bias().0,
                    start_address: x.actual_load_addr().0,
                    length: x.len(),
                });
            }
        }
    });
    vec
}

fn init_global_context() -> GlobalContext {
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
        } else { dwarfs.push(Default::default()) };
    }

    let endian = if images[0].object.is_little_endian() {
        gimli::RunTimeEndian::Little
    } else {
        gimli::RunTimeEndian::Big
    };

    let dwarf_slices: Vec<gimli::Dwarf<gimli::EndianSlice<'static, gimli::RunTimeEndian>>> = unsafe {
        dwarfs
            .iter()
            .map(|x|
                x.borrow(|data|
                    gimli::EndianSlice::new(slice::from_raw_parts(data.as_ptr(), data.len()), endian)))
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

#[cfg(test)]
mod tests {
    use libc::c_void;
    use object::Object;
    use crate::{init_global_context, init_images};

    #[test]
    fn it_works() {
        let g = init_global_context();
        for i in g.images.iter() {
            println!("{:#?}", i.file);
            println!("{:#?}", i.mmap);
            println!("{:#?}", i.base_addresses);
            println!("{:#?}", i.bias);
            println!("{:#?}", i.start_address);
            println!("{:#?}", i.length);
        }
    }
}
