use pinocchio::{AccountView, Address, ProgramResult};

pub struct Swap;

impl Swap {
    pub fn process(
        _program_id: &Address,
        _accounts: &mut [AccountView],
        _data: &[u8],
    ) -> ProgramResult {
        todo!()
    }
}
