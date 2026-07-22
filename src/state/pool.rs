use core::mem::size_of;

use pinocchio::{
    AccountView, Address,
    account::{Ref, RefMut},
    error::ProgramError,
};

#[repr(C)]
pub struct Config {
    seed: [u8; 8],
    authority: Address, // Address::default() == no authority (immutable pool)
    mint_x: Address,
    mint_y: Address,
    fee: [u8; 2],
    locked: u8, // 0 = false, 1 = true
    config_bump: u8,
    lp_bump: u8,
}

impl Config {
    pub const LEN: usize = size_of::<Self>();

    #[inline(always)]
    pub fn load(account: &AccountView) -> Result<Ref<Self>, ProgramError> {
        if account.data_len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if account.owner() != &crate::ID {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(Ref::map(account.try_borrow()?, |data| unsafe {
            Self::from_bytes_unchecked(data)
        }))
    }

    #[inline(always)]
    pub fn load_mut(account: &mut AccountView) -> Result<RefMut<Self>, ProgramError> {
        if account.data_len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        if account.owner() != &crate::ID {
            return Err(ProgramError::InvalidAccountOwner);
        }
        Ok(RefMut::map(account.try_borrow_mut()?, |data| unsafe {
            Self::from_bytes_unchecked_mut(data)
        }))
    }

    /// # Safety
    /// Caller must ensure `bytes.len() == Config::LEN`. Every field is a byte
    /// array or `u8`, so there's no alignment requirement to uphold.
    #[inline(always)]
    pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    #[inline(always)]
    pub unsafe fn from_bytes_unchecked_mut(bytes: &mut [u8]) -> &mut Self {
        unsafe { &mut *(bytes.as_mut_ptr() as *mut Self) }
    }

    // ---- getters ----

    #[inline(always)]
    pub fn seed(&self) -> u64 {
        u64::from_le_bytes(self.seed)
    }

    #[inline(always)]
    pub fn mint_x(&self) -> &Address {
        &self.mint_x
    }

    #[inline(always)]
    pub fn mint_y(&self) -> &Address {
        &self.mint_y
    }

    #[inline(always)]
    pub fn fee(&self) -> u16 {
        u16::from_le_bytes(self.fee)
    }

    #[inline(always)]
    pub fn locked(&self) -> bool {
        self.locked != 0
    }

    #[inline(always)]
    pub fn config_bump(&self) -> u8 {
        self.config_bump
    }

    #[inline(always)]
    pub fn lp_bump(&self) -> u8 {
        self.lp_bump
    }

    #[inline(always)]
    pub fn authority(&self) -> Option<Address> {
        if self.authority == Address::new_from_array([0u8; 32]) {
            None
        } else {
            Some(self.authority)
        }
    }

    // ---- setters ----

    #[inline(always)]
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed.to_le_bytes();
    }

    #[inline(always)]
    pub fn set_authority(&mut self, authority: Option<Address>) {
        self.authority = authority.unwrap_or(Address::new_from_array([0u8; 32]));
    }

    #[inline(always)]
    pub fn set_mint_x(&mut self, mint_x: Address) {
        self.mint_x = mint_x;
    }

    #[inline(always)]
    pub fn set_mint_y(&mut self, mint_y: Address) {
        self.mint_y = mint_y;
    }

    #[inline(always)]
    pub fn set_fee(&mut self, fee: u16) -> Result<(), ProgramError> {
        if fee >= 10_000 {
            return Err(crate::error::AmmError::InvalidFee.into());
        }
        self.fee = fee.to_le_bytes();
        Ok(())
    }

    #[inline(always)]
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked as u8;
    }

    #[inline(always)]
    pub fn set_config_bump(&mut self, bump: u8) {
        self.config_bump = bump;
    }

    #[inline(always)]
    pub fn set_lp_bump(&mut self, bump: u8) {
        self.lp_bump = bump;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_inner(
        &mut self,
        seed: u64,
        authority: Option<Address>,
        mint_x: Address,
        mint_y: Address,
        fee: u16,
        config_bump: u8,
        lp_bump: u8,
    ) -> Result<(), ProgramError> {
        self.set_seed(seed);
        self.set_authority(authority);
        self.set_mint_x(mint_x);
        self.set_mint_y(mint_y);
        self.set_fee(fee)?;
        self.set_locked(false);
        self.set_config_bump(config_bump);
        self.set_lp_bump(lp_bump);
        Ok(())
    }
}
