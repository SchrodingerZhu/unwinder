use crate::{GlobalContext, UnwindError};
use gimli::{Register, RegisterRule, UnwindContextStorage};

#[cfg(target_arch = "x86_64")]
mod x86_64;

#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

pub trait CursorState: Sized + Copy + Clone {
    fn new(u_ctx: &libc::ucontext_t) -> Self;
    fn get_program_counter(&self) -> usize;
    fn get_register(&self, reg: Register) -> Result<usize, UnwindError>;

    fn get_cfa<R, S>(
        &self,
        row: &gimli::UnwindTableRow<R, S>,
        g_ctx: &GlobalContext,
    ) -> Result<usize, UnwindError>
    where
        R: gimli::Reader,
        S: UnwindContextStorage<R>;

    fn step<R, S>(
        &mut self,
        row: &gimli::UnwindTableRow<R, S>,
        g_ctx: &GlobalContext,
    ) -> Result<(), UnwindError>
    where
        R: gimli::Reader,
        S: UnwindContextStorage<R>;

    fn recover_register<R, S>(
        &self,
        reg: Register,
        row: &gimli::UnwindTableRow<R, S>,
        g_ctx: &GlobalContext,
    ) -> Result<usize, UnwindError>
    where
        R: gimli::Reader,
        S: UnwindContextStorage<R>,
    {
        self.get_cfa(row, g_ctx)
            .and_then(|cfa| match row.register(reg) {
                RegisterRule::Undefined => Err(UnwindError::UnwindEnded),
                RegisterRule::SameValue => self.get_register(reg),
                RegisterRule::Offset(offset) => unsafe {
                    Ok(*((cfa as i64 + offset) as usize as *mut usize))
                },
                RegisterRule::ValOffset(offset) => Ok((cfa as i64 + offset) as usize),
                RegisterRule::Register(target) => self.get_register(target),
                RegisterRule::Expression(_) => {
                    todo!()
                }
                RegisterRule::ValExpression(_) => {
                    todo!()
                }
                RegisterRule::Architectural => Err(UnwindError::NotSupported(
                    "target register recovery is architectural",
                )),
            })
    }
}
