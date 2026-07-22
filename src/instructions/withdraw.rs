use pinocchio::{AccountView, Address, ProgramResult};

pub struct Withdraw;

impl Withdraw {
    pub fn process(
        _program_id: &Address,
        _accounts: &mut [AccountView],
        _data: &[u8],
    ) -> ProgramResult {
        todo!()
    }
}
