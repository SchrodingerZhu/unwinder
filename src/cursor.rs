use crate::image::ImageReader;
use crate::{GlobalContext, SymbolInfo, UnwindError};
use gimli::{CfaRule, Reader, Register, RegisterRule, StoreOnHeap, UnwindContext, UnwindContextStorage, UnwindSection, UnwindTableRow};
use libc::ucontext_t;
use nix::errno::Errno;
use std::mem::MaybeUninit;

struct InlineStorage;

#[cfg(target_arch = "x86_64")]
mod x86_64 {
    use gimli::{CfaRule, Register, RegisterRule, UnwindContextStorage};
    use crate::UnwindError;

    #[derive(Copy, Clone)]
    pub struct FrameState {
        rip: usize,
        rsp: usize,
    }

    impl FrameState {
        pub fn new(uctx: &libc::ucontext_t) -> Self {
            Self {
                rip: uctx.uc_mcontext.gregs[libc::REG_RIP as usize] as _,
                rsp: uctx.uc_mcontext.gregs[libc::REG_RSP as usize] as _,
            }
        }

        pub fn get_program_counter(&self) -> usize {
            self.rip
        }

        fn get_frame_pointer(&self) -> usize {
            self.rsp
        }

        // only use this after calling recover_frame_pointer
        pub fn step<R, S>(&mut self, row: &gimli::UnwindTableRow<R, S>) -> Result<(), UnwindError>
            where
                R: gimli::Reader,
                S: UnwindContextStorage<R>
        {
            match row.cfa() {
                CfaRule::RegisterAndOffset { register, offset } => {
                    if register.0 == 7 as u16 {
                        self.rsp = (self.rsp as i64 + offset) as usize;
                        match row.register(Register(16)) {
                            RegisterRule::Undefined => {}
                            RegisterRule::SameValue => {}
                            RegisterRule::Offset(offset) => unsafe {
                                self.rip = *((self.rsp as i64 + offset) as usize as *mut usize);
                                return Ok(());
                            }
                            RegisterRule::ValOffset(_) => {}
                            RegisterRule::Register(_) => {}
                            RegisterRule::Expression(_) => {}
                            RegisterRule::ValExpression(_) => {}
                            RegisterRule::Architectural => {}
                        }
                    }
                    Err(UnwindError::UnwindLogicalError("finished".to_string()))
                }
                CfaRule::Expression(_) => {
                    todo!()
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

impl<R: Reader> UnwindContextStorage<R> for InlineStorage {
    type Rules = [(Register, RegisterRule<R>); 192];
    type Stack = [UnwindTableRow<R, Self>; 32];
}

struct UnwindCursor<'a, S: UnwindContextStorage<ImageReader<'a>>> {
    global_ctx: &'a GlobalContext<'a>,
    local_ctx: UnwindContext<ImageReader<'a>, S>,
    state: FrameState,
}

trait Unwinding<'a, S: UnwindContextStorage<ImageReader<'a>>>: Sized {
    fn local_context_mut(&mut self) -> &mut UnwindContext<ImageReader<'a>, S>;
    fn state_mut(&mut self) -> &mut FrameState;
    fn local_context(&self) -> &UnwindContext<ImageReader<'a>, S>;
    fn state(&self) -> &FrameState;
    fn global_context(&self) -> &'a GlobalContext<'a>;

    fn new(g_ctx: &'a GlobalContext<'a>) -> Result<Self, UnwindError> {
        let mut context = MaybeUninit::<libc::ucontext_t>::uninit();
        let res = unsafe { libc::getcontext(context.as_mut_ptr()) };
        Errno::result(res)
            .map(|_| unsafe { Self::from_ucontext(g_ctx, context.assume_init()) })
            .map_err(Into::into)
    }

    fn from_ucontext(g_ctx: &'a GlobalContext<'a>, u_ctx: libc::ucontext_t) -> Self;

    fn get_sym_info(&self) -> SymbolInfo<'a> {
        self.global_context().resolve_symbol(self.state().get_program_counter())
    }

    fn setup_unwind_info(&mut self) -> Result<&UnwindTableRow<ImageReader<'a>, S>, UnwindError> {
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
            let unwind_info = self.setup_unwind_info()?;
            state.step(&unwind_info)?;
        }
        *self.state_mut() = state;
        Ok(())
    }
}

impl<'a, S: UnwindContextStorage<ImageReader<'a>>> Unwinding<'a, S> for UnwindCursor<'a, S> {
    fn local_context_mut(&mut self) -> &mut UnwindContext<ImageReader<'a>, S> {
        &mut self.local_ctx
    }

    fn state_mut(&mut self) -> &mut FrameState {
        &mut self.state
    }

    fn local_context(&self) -> &UnwindContext<ImageReader<'a>, S> {
        &self.local_ctx
    }

    fn state(&self) -> &FrameState {
        &self.state
    }

    fn global_context(&self) -> &'a GlobalContext<'a> {
        self.global_ctx
    }

    fn from_ucontext(g_ctx: &'a GlobalContext<'a>, u_ctx: ucontext_t) -> Self {
        Self {
            global_ctx: g_ctx,
            local_ctx: Default::default(),
            state: FrameState::new(&u_ctx),
        }
    }
}

type DynamicUnwindCursor<'a> = UnwindCursor<'a, StoreOnHeap>;
type StaticUnwindCursor<'a> = UnwindCursor<'a, InlineStorage>;

#[cfg(test)]
mod test {
    extern crate rustc_demangle;
    use crate::cursor::Unwinding;
    use crate::{Frame, GlobalContext};

    #[test]
    fn it_inits_cursor() {
        let g = GlobalContext::new();
        let mut cursor = super::DynamicUnwindCursor::new(&g).unwrap();
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
                    .map(|x|rustc_demangle::demangle(&x).to_string())
                    .collect::<Vec<_>>()
            );
            println!()
        }
    }
}
