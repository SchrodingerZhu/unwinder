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
    dynamic_address: usize,
    object_name: Option<&'a str>,
    static_address: Option<usize>,
    associated_frames: Vec<Frame<'a>>,
}

impl<'a> GlobalContext<'a> {
    fn new() -> Self {
        let images = image::init_images();
        GlobalContext { images }
    }
    fn resolve_symbol(&'a self, address: usize) -> SymbolInfo<'a> {
        let mut symbol = SymbolInfo {
            dynamic_address: address,
            object_name: None,
            static_address: None,
            associated_frames: Vec::new(),
        };
        for image in &self.images {
            if address >= image.start_address && address < image.start_address + image.length {
                let static_address = address - image.bias;
                symbol.static_address.replace(static_address);
                symbol.object_name.replace(&image.filename);
                if let Some(address_context) = image.address_context.as_ref() {
                    if let Ok(mut frames) = address_context.find_frames(static_address as u64) {
                        while let Ok(Some(frame)) = frames.next() {
                            symbol.associated_frames.push(Frame::Dwarf(frame));
                        }
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
    use crate::{Frame, GlobalContext};

    #[test]
    fn it_works() {
        GlobalContext::new();
    }

    #[test]
    fn it_resolves() {
        let g = GlobalContext::new();
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
