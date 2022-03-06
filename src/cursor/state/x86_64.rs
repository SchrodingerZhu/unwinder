use crate::cursor::state::CursorState;
use crate::{GlobalContext, UnwindError};
use gimli::{CfaRule, Reader, Register, RegisterRule, UnwindContextStorage, UnwindTableRow};

#[derive(Copy, Clone)]
pub struct FramePointerBasedState {
    rip: usize,
    rsp: usize,
}

const STACK_POINTER_IDX: u16 = 7;
const RETURN_ADDRESS_IDX: u16 = 16;

impl CursorState for FramePointerBasedState {
    fn new(uctx: &libc::ucontext_t) -> Self {
        Self {
            rip: uctx.uc_mcontext.gregs[libc::REG_RIP as usize] as _,
            rsp: uctx.uc_mcontext.gregs[libc::REG_RSP as usize] as _,
        }
    }

    fn get_program_counter(&self) -> usize {
        self.rip
    }

    fn get_register(&self, reg: Register) -> Result<usize, UnwindError> {
        match reg.0 {
            STACK_POINTER_IDX => Ok(self.rsp),
            _ => Err(UnwindError::NotSupported(
                "only RSP can be retrieved in frame pointer based state",
            )),
        }
    }

    fn get_cfa<R, S>(
        &self,
        row: &UnwindTableRow<R, S>,
        _: &GlobalContext,
    ) -> Result<usize, UnwindError>
    where
        R: Reader,
        S: UnwindContextStorage<R>,
    {
        match row.cfa() {
            CfaRule::RegisterAndOffset { register, offset } => {
                if register.0 == STACK_POINTER_IDX {
                    Ok((self.rsp as i64 + offset) as usize)
                } else {
                    Err(UnwindError::NotSupported(
                        "CFA value is only derivable from RSP in frame pointer based state",
                    ))
                }
            }
            CfaRule::Expression(_) => Err(UnwindError::NotSupported(
                "CFA expression is not supported in frame pointer based state",
            )),
        }
    }

    fn step<R, S>(
        &mut self,
        row: &UnwindTableRow<R, S>,
        g_ctx: &GlobalContext,
    ) -> Result<(), UnwindError>
    where
        R: Reader,
        S: UnwindContextStorage<R>,
    {
        self.rip = self.recover_register(Register(RETURN_ADDRESS_IDX), row, g_ctx)?;
        self.rsp = self.get_cfa(&row, g_ctx)?;
        Ok(())
    }
}
