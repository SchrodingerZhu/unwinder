use crate::cursor::state::CursorState;
use crate::image::ImageReader;
use crate::{cffi, GlobalContext, SymbolInfo, UnwindError};
use gimli::{
    Reader, Register, RegisterRule, StoreOnHeap, UnwindContext, UnwindContextStorage,
    UnwindSection, UnwindTableRow,
};
use libc::ucontext_t;
use nix::errno::Errno;
use std::borrow::Borrow;
use std::mem::MaybeUninit;

mod state;

struct InlineStorage;

impl<R: Reader> UnwindContextStorage<R> for InlineStorage {
    type Rules = [(Register, RegisterRule<R>); 192];
    type Stack = [UnwindTableRow<R, Self>; 32];
}

struct UnwindCursor<'a, Storage, State>
where
    Storage: UnwindContextStorage<ImageReader<'a>>,
    State: CursorState,
{
    global_ctx: &'a GlobalContext<'a>,
    local_ctx: UnwindContext<ImageReader<'a>, Storage>,
    state: State,
}

trait Unwinding<'a, Storage, State>: Sized
where
    Storage: UnwindContextStorage<ImageReader<'a>>,
    State: CursorState,
{
    fn local_context_mut(&mut self) -> &mut UnwindContext<ImageReader<'a>, Storage>;
    fn state_mut(&mut self) -> &mut State;
    fn local_context(&self) -> &UnwindContext<ImageReader<'a>, Storage>;
    fn state(&self) -> &State;
    fn global_context(&self) -> &'a GlobalContext<'a>;

    fn new(g_ctx: &'a GlobalContext<'a>) -> Result<Self, UnwindError> {
        let mut context = MaybeUninit::<libc::ucontext_t>::uninit();
        let res = unsafe { cffi::getcontext(context.as_mut_ptr()) };
        Errno::result(res)
            .map(|_| unsafe { Self::from_ucontext(g_ctx, context.assume_init()) })
            .map_err(Into::into)
    }

    fn from_ucontext(g_ctx: &'a GlobalContext<'a>, u_ctx: libc::ucontext_t) -> Self;

    fn get_sym_info(&self) -> SymbolInfo<'a> {
        self.global_context()
            .resolve_symbol(self.state().get_program_counter())
    }

    fn setup_unwind_info(
        &mut self,
    ) -> Result<&UnwindTableRow<ImageReader<'a>, Storage>, UnwindError> {
        let pc = self.state().get_program_counter();
        if let Some(img) = self.global_context().find_image(pc) {
            let address = pc as u64 - img.bias as u64;
            if let Some(table) = img.eh_frame_hdr_section.as_ref().and_then(|x| x.1.table()) {
                table
                    .unwind_info_for_address(
                        &img.eh_frame_section.1,
                        &img.base_addresses,
                        self.local_context_mut(),
                        address,
                        gimli::EhFrame::cie_from_offset,
                    )
                    .map_err(Into::into)
            } else {
                img.eh_frame_section
                    .1
                    .unwind_info_for_address(
                        &img.base_addresses,
                        self.local_context_mut(),
                        address,
                        gimli::EhFrame::cie_from_offset,
                    )
                    .map_err(Into::into)
            }
        } else {
            Result::Err(UnwindError::UnknownProgramCounter(pc))
        }
    }

    fn next(&mut self) -> Result<(), UnwindError> {
        let mut state = *self.state();
        {
            let context = self.global_context().borrow();
            let unwind_info = self.setup_unwind_info()?;
            state.step(&unwind_info, context)?;
        }
        *self.state_mut() = state;
        Ok(())
    }
}

impl<'a, Storage, State> Unwinding<'a, Storage, State> for UnwindCursor<'a, Storage, State>
where
    Storage: UnwindContextStorage<ImageReader<'a>>,
    State: CursorState,
{
    fn local_context_mut(&mut self) -> &mut UnwindContext<ImageReader<'a>, Storage> {
        &mut self.local_ctx
    }

    fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    fn local_context(&self) -> &UnwindContext<ImageReader<'a>, Storage> {
        &self.local_ctx
    }

    fn state(&self) -> &State {
        &self.state
    }

    fn global_context(&self) -> &'a GlobalContext<'a> {
        self.global_ctx
    }

    fn from_ucontext(g_ctx: &'a GlobalContext<'a>, u_ctx: ucontext_t) -> Self {
        Self {
            global_ctx: g_ctx,
            local_ctx: Default::default(),
            state: State::new(&u_ctx),
        }
    }
}

type DynamicUnwindCursor<'a, State> = UnwindCursor<'a, StoreOnHeap, State>;
type StaticUnwindCursor<'a, State> = UnwindCursor<'a, InlineStorage, State>;

#[cfg(test)]
mod test {
    extern crate rustc_demangle;

    use crate::cursor::state::FramePointerBasedState;
    use crate::cursor::Unwinding;
    use crate::{Frame, GlobalContext};

    #[test]
    fn it_inits_cursor() {
        let g = GlobalContext::new();
        let mut cursor = super::DynamicUnwindCursor::<FramePointerBasedState>::new(&g).unwrap();
        while let Ok(_) = cursor.next() {
            let sym = cursor.get_sym_info();
            println!("AVMA: {:?}", sym.avma);
            println!("SVMA: {:?}", sym.svma);
            println!("object: {:?}", sym.object_name);
            println!(
                "name: {:?}",
                sym.associated_frames
                    .iter()
                    .filter_map(|x| match x {
                        Frame::Dwarf(d) => {
                            d.function
                                .as_ref()
                                .and_then(|x| x.name.to_string().map(|x| x.to_string()).ok())
                        }
                        Frame::SymbolMap(map) => {
                            Some(map.to_string())
                        }
                    })
                    .map(|x| rustc_demangle::demangle(&x).to_string())
                    .collect::<Vec<_>>()
            );
            println!()
        }
    }
}
