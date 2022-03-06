#![cfg_attr(test, feature(rustc_private))]

use std::fmt::{Debug, Display, Formatter};

mod cffi;
pub mod cursor;
pub mod image;

#[derive(thiserror::Error, Debug)]
pub enum UnwindError {
    IOError(#[from] std::io::Error),
    ObjectParsingError(#[from] object::Error),
    GimliError(#[from] gimli::Error),
    ErrnoError(#[from] nix::errno::Errno),
    UnknownProgramCounter(usize),
    UnwindLogicalError(&'static str),
    NotSupported(&'static str),
    UnwindEnded,
}

impl Display for UnwindError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            UnwindError::IOError(e) => Display::fmt(e, f),
            UnwindError::ObjectParsingError(e) => Display::fmt(e, f),
            UnwindError::ErrnoError(e) => Display::fmt(e, f),
            UnwindError::GimliError(e) => Display::fmt(e, f),
            UnwindError::UnknownProgramCounter(pc) => {
                write!(f, "unknown program counter: {:#x}", pc)
            }
            UnwindError::UnwindLogicalError(s) => {
                write!(f, "{}", s)
            }
            UnwindError::NotSupported(s) => {
                write!(f, "{}", s)
            }
            UnwindError::UnwindEnded => {
                write!(f, "cursor cannot step any further")
            }
        }
    }
}

pub struct GlobalContext<'a> {
    images: Vec<image::Image<'a>>,
}

enum Frame<'a> {
    Dwarf(addr2line::Frame<'a, image::ImageReader<'a>>),
    SymbolMap(&'a str),
}

struct SymbolInfo<'a> {
    object_name: Option<&'a str>,
    avma: usize,
    svma: Option<usize>,
    associated_frames: Vec<Frame<'a>>,
}

impl<'a> SymbolInfo<'a> {
    fn new_unresolved(avma: usize) -> Self {
        Self {
            object_name: None,
            avma,
            svma: None,
            associated_frames: Vec::new(),
        }
    }
}

impl<'a> GlobalContext<'a> {
    fn new() -> Self {
        let images = image::load_all();
        GlobalContext { images }
    }

    fn find_image(&self, avma: usize) -> Option<&image::Image<'a>> {
        match self
            .images
            .binary_search_by_key(&std::cmp::Reverse(avma), |x| {
                std::cmp::Reverse(x.start_address)
            }) {
            Ok(i) => Some(&self.images[i]),
            Err(i) if i >= self.images.len() => None,
            Err(i) => {
                if self.images[i].has(avma) {
                    Some(&self.images[i])
                } else {
                    None
                }
            }
        }
    }

    fn resolve_symbol(&'a self, avma: usize) -> SymbolInfo<'a> {
        self.find_image(avma)
            .map(|image| {
                let svma = avma - image.bias;
                let object_name = Some(&image.filename as &str);
                let mut associated_frames = Vec::new();

                if let Some(line_ctx) = image.line_context.as_ref() {
                    if let Ok(mut frames) = line_ctx.find_frames(svma as u64) {
                        while let Ok(Some(frame)) = frames.next() {
                            associated_frames.push(Frame::Dwarf(frame));
                        }
                    }
                }

                if associated_frames.is_empty() {
                    // Find the symbol at the current address.
                    if let Some(elf_symbol) = image.symbol_map.get(svma as u64) {
                        associated_frames.push(Frame::SymbolMap(elf_symbol.name()));
                    }
                }

                SymbolInfo {
                    object_name,
                    avma,
                    svma: Some(svma),
                    associated_frames,
                }
            })
            .unwrap_or(SymbolInfo::new_unresolved(avma))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Frame, GlobalContext};

    #[test]
    fn it_works() {
        GlobalContext::new();
    }

    #[test]
    fn it_resolves() {
        let g = GlobalContext::new();
        let resolved = g.resolve_symbol(it_resolves as usize);
        println!("AVMA: {:?}", resolved.avma);
        println!("SVMA: {:?}", resolved.svma);
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
