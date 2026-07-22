use pinocchio::{AccountView, Address, ProgramResult, error::ProgramError};

use crate::instructions::{deposit, initialize, swap, withdraw};

pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data.split_first() {
        Some((0, rest)) => initialize::process(program_id, accounts, rest),
        Some((1, rest)) => deposit::process(program_id, accounts, rest),
        Some((2, rest)) => swap::process(program_id, accounts, rest),
        Some((3, rest)) => withdraw::process(program_id, accounts, rest),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
