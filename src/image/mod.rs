use crate::image::debug_info::RawDebugInfo;
use crate::image::symbol_map::OwnedSymbolMap;
use addr2line::Context as LineCtx;
use findshlibs::{SharedLibrary, TargetSharedLibrary};
use gimli::{EndianSlice, ParsedEhFrameHdr, RunTimeEndian};
use object::{Object, ObjectSection};
use std::mem::ManuallyDrop;

mod base_addresses;
mod debug_info;
mod line_info;
mod raw_image;
mod symbol_map;

pub struct Image<'a> {
    pub filename: String,
    pub base_addresses: gimli::BaseAddresses,
    pub bias: usize,
    pub start_address: usize,
    pub length: usize,
    pub symbol_map: OwnedSymbolMap,
    pub dbg_info: RawDebugInfo,
    pub line_context: Option<LineCtx<ImageReader<'a>>>,
    pub eh_frame_section: (Vec<u8>, gimli::EhFrame<ImageReader<'a>>),
    pub eh_frame_hdr_section: Option<(Vec<u8>, ParsedEhFrameHdr<ImageReader<'a>>)>,
    pub endian: RunTimeEndian,
}

impl<'a> Image<'a> {
    pub fn has(&self, avma: usize) -> bool {
        self.start_address <= avma && avma < self.start_address + self.length
    }
}

pub type ImageReader<'a> = EndianSlice<'a, RunTimeEndian>;

pub fn load_all<'a>() -> Vec<Image<'a>> {
    let mut vec = Vec::new();

    TargetSharedLibrary::each(|x| {
        if let Ok((object, mmap, file)) = raw_image::load(x.name()) {
            if let Some(ba) = base_addresses::load(&object) {
                let symbol_map = symbol_map::load(&object);

                let dbg_info = debug_info::load(x.name(), &object);
                let endian = if object.is_little_endian() {
                    RunTimeEndian::Little
                } else {
                    RunTimeEndian::Big
                };

                let line_context = line_info::load(&dbg_info, endian);

                let address_size = std::mem::size_of::<*const ()>() as u8;
                let eh_frame_hdr_section = object
                    .section_by_name(".eh_frame_hdr")
                    .and_then(|x| x.uncompressed_data().ok())
                    .map(|x| x.to_vec())
                    .and_then(|data| unsafe {
                        let slice: &'a [u8] = std::slice::from_raw_parts(data.as_ptr(), data.len());
                        gimli::EhFrameHdr::new(slice, endian)
                            .parse(&ba, address_size)
                            .ok()
                            .map(|hdr| (data, hdr))
                    });

                let eh_frame_data = object
                    .section_by_name(".eh_frame")
                    .and_then(|x| x.uncompressed_data().ok())
                    .map(|x| x.to_vec())
                    .unwrap_or_else(Default::default);

                let eh_frame = unsafe {
                    let slice: &'a [u8] =
                        std::slice::from_raw_parts(eh_frame_data.as_ptr(), eh_frame_data.len());
                    gimli::EhFrame::new(slice, endian)
                };

                vec.push(Image {
                    filename: x.name().to_string_lossy().to_string(),
                    base_addresses: ba,
                    bias: x.virtual_memory_bias().0,
                    start_address: x.actual_load_addr().0,
                    length: x.len(),
                    symbol_map,
                    dbg_info,
                    line_context,
                    eh_frame_section: (eh_frame_data, eh_frame),
                    eh_frame_hdr_section,
                    endian,
                });
            }
            ManuallyDrop::into_inner(mmap);
            ManuallyDrop::into_inner(file);
        }
    });

    vec.sort_by_key(|x| std::cmp::Reverse(x.start_address));
    vec
}
