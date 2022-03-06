use crate::{GlobalContext, UnwindError};
use gimli::{
    EndianSlice, Endianity, EvaluationResult, Expression, Location, Reader, Register, RegisterRule,
    Section, UnwindContextStorage,
};
use std::{ptr, slice};

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
                RegisterRule::Expression(expr) => {
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

    fn eval<R, S>(
        &self,
        cfa: usize,
        mut expr: Expression<R>,
        row: &gimli::UnwindTableRow<R, S>,
        g_ctx: &GlobalContext,
    ) -> Result<usize, UnwindError>
    where
        R: gimli::Reader,
        S: UnwindContextStorage<R>,
    {
        let image = g_ctx
            .find_image(cfa)
            .ok_or(UnwindError::UnwindLogicalError(
                "failed to locate image for given CFA",
            ))?;
        let dbg_info = image
            .dwarf
            .debug_info
            .borrow(|x| EndianSlice::new(x, image.endian));
        let header = dbg_info
            .units()
            .next()?
            .ok_or(UnwindError::UnwindLogicalError(
                "cannot fetch debug info header for evaluation",
            ))?;
        let mut evaluation = expr.evaluation(header.encoding());
        let mut status = evaluation.evaluate()?;
        loop {
            match status {
                EvaluationResult::Complete => unsafe {
                    let value = 0usize;
                    let result = evaluation.result();
                    if result.len() != 1 {
                        return Err(UnwindError::UnwindLogicalError(
                            "evaluation returns unexpected result",
                        ));
                    }
                    let res = &result[0];
                    let check = || {
                        if res.bit_offset.is_some() {
                            return Err(UnwindError::NotSupported(
                                "bit offset in evaluation result is not supported",
                            ));
                        } else if let Some(s) = res.size_in_bits {
                            if s != header.address_size() as u64 * 8 {
                                return Err(UnwindError::UnwindLogicalError(
                                    "unexpected bit size in evaluation result",
                                ));
                            }
                        }
                        Ok(())
                    };
                    return match &res.location {
                        Location::Empty => Err(UnwindError::UnwindLogicalError(
                            "evaluation returns empty result",
                        )),
                        Location::Value { value } => {
                            value.to_u64(u64::MAX).map(|x| x as usize).map_err(|_| {
                                UnwindError::UnwindLogicalError("evaluation returns wrongly result")
                            })
                        }
                        Location::Register { register } => {
                            check().and_then(|_| self.get_register(*register))
                        }
                        Location::Address { address } => {
                            let mut address = *address as usize;
                            if let Some(x) = res.bit_offset {
                                address += x as usize / 8;
                            }
                            Ok(*(address as *mut usize))
                        }
                        Location::Bytes { value } => {
                            let mut data = 0usize;
                            value.endian().read_u64(slice::from_raw_parts_mut(
                                &mut data as *mut usize as *mut u8,
                                header.address_size() as usize,
                            ));
                            Ok(data)
                        }
                        Location::ImplicitPointer { value, byte_offset } => {
                            let reader = dbg_info.reader();
                            let total_offset = match std::mem::size_of_val(&value.0) {
                                8 => {
                                    let mut offset: i64 = 0;
                                    ptr::copy(
                                        &value.0 as *const _ as *const u8,
                                        &mut offset as *mut _ as *mut u8,
                                        8,
                                    );
                                    (offset + *byte_offset) as usize
                                }
                                4 => {
                                    let mut offset: i32 = 0;
                                    ptr::copy(
                                        &value.0 as *const _ as *const u8,
                                        &mut offset as *mut _ as *mut u8,
                                        4,
                                    );
                                    (offset as i64 + *byte_offset) as usize
                                }
                                2 => {
                                    let mut offset: i16 = 0;
                                    ptr::copy(
                                        &value.0 as *const _ as *const u8,
                                        &mut offset as *mut _ as *mut u8,
                                        2,
                                    );
                                    (offset as i64 + *byte_offset) as usize
                                }
                                1 => {
                                    let mut offset: i8 = 0;
                                    ptr::copy(
                                        &value.0 as *const _ as *const u8,
                                        &mut offset as *mut _ as *mut u8,
                                        1,
                                    );
                                    (offset as i64 + *byte_offset) as usize
                                }
                                _ => usize::MAX,
                            };
                            if total_offset == usize::MAX {
                                return Err(UnwindError::NotSupported(
                                    "unknown implicit value offset type",
                                ));
                            }

                            if reader.len() <= total_offset {
                                return Err(UnwindError::UnwindLogicalError(
                                    "implicit pointer out of range in evaluation result",
                                ));
                            }
                            let slice = EndianSlice::new(&reader[total_offset..], image.endian);
                            let mut data = 0usize;
                            slice.endian().read_u64(slice::from_raw_parts_mut(
                                &mut data as *mut usize as *mut u8,
                                header.address_size() as usize,
                            ));
                            Ok(data)
                        }
                    };
                },
                EvaluationResult::RequiresMemory { .. } => {}
                EvaluationResult::RequiresRegister { .. } => {}
                EvaluationResult::RequiresFrameBase => {}
                EvaluationResult::RequiresTls(_) => {}
                EvaluationResult::RequiresCallFrameCfa => {}
                EvaluationResult::RequiresAtLocation(_) => {}
                EvaluationResult::RequiresEntryValue(_) => {}
                EvaluationResult::RequiresParameterRef(_) => {}
                EvaluationResult::RequiresRelocatedAddress(_) => {}
                EvaluationResult::RequiresIndexedAddress { .. } => {}
                EvaluationResult::RequiresBaseType(_) => {}
            }
        }
    }
}
