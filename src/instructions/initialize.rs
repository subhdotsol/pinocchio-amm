use core::mem::size_of;

use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{Sysvar, rent::Rent},
};

pub struct Initialize;

impl Initialize {
    pub fn process(
        _program_id: &Address,
        _accounts: &mut [AccountView],
        _data: &[u8],
    ) -> ProgramResult {
        todo!()
    }
}
