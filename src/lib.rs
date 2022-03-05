#![feature(rustc_private)]

use gimli::{Reader, Register, RegisterRule, UnwindContextStorage, UnwindTableRow};
use std::fmt::{Display, Formatter};

pub mod image;

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

struct GlobalContext<'a> {
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
        let images = image::init_images();
        GlobalContext { images }
    }

    fn find_image(&self, avma: usize) -> Option<&image::Image<'a>> {
        self.images.iter().find(|img| img.has(avma))
    }

    fn resolve_symbol(&'a self, avma: usize) -> SymbolInfo<'a> {
        self.find_image(avma)
            .map(|image| {
                let svma = avma - image.bias;
                let object_name = Some(&image.filename as &str);
                let mut associated_frames = Vec::new();

                if let Some(address_context) = image.address_context.as_ref() {
                    if let Ok(mut frames) = address_context.find_frames(svma as u64) {
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
