use pinocchio::{AccountView, Address, ProgramResult};

pub struct Deposit;

impl Deposit {
    pub fn process(
        _program_id: &Address,
        _accounts: &mut [AccountView],
        _data: &[u8],
    ) -> ProgramResult {
        todo!()
    }
}
