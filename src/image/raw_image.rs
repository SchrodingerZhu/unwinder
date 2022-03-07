use crate::UnwindError;
use memmap::Mmap;
use object::File as ObjFile;
use std::fs::File;
use std::mem::ManuallyDrop;
use std::path::Path;

type RawImage<'a> = (object::File<'a>, ManuallyDrop<Mmap>, ManuallyDrop<File>);

pub fn load<'a, T: AsRef<Path>>(x: T) -> Result<RawImage<'a>, UnwindError> {
    File::open(x)
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
}
