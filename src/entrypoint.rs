use pinocchio::{AccountView, Address, ProgramResult, error::ProgramError};

use crate::instructions::{
    deposit::Deposit,
    initialize::Initialize,
    swap::Swap,
    withdraw::Withdraw,
};

pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data.split_first() {
        Some((0, rest)) => Initialize::process(program_id, accounts, rest),
        Some((1, rest)) => Deposit::process(program_id, accounts, rest),
        Some((2, rest)) => Swap::process(program_id, accounts, rest),
        Some((3, rest)) => Withdraw::process(program_id, accounts, rest),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
