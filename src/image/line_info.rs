use crate::image::debug_info::RawDebugInfo;
use crate::image::ImageReader;
use addr2line::Context as LineCtx;
use gimli::{EndianSlice, RunTimeEndian};

pub fn load<'a>(
    dbg_info: &RawDebugInfo,
    endian: RunTimeEndian,
) -> Option<LineCtx<ImageReader<'a>>> {
    LineCtx::from_dwarf({
        dbg_info.borrow(|data| unsafe {
            EndianSlice::new(
                std::slice::from_raw_parts(data.as_ptr(), data.len()),
                endian,
            )
        })
    })
    .ok()
}
