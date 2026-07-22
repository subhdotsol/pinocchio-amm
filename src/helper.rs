use crate::error::AmmError;
use pinocchio::{AccountView, Address, error::ProgramError};

#[inline(always)]
pub fn signer_check(account: &AccountView) -> Result<(), ProgramError> {
    if !account.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    Ok(())
}

#[inline(always)]
pub fn system_account_check(account: &AccountView) -> Result<(), ProgramError> {
    if account.owner() != &pinocchio_system::ID {
        return Err(ProgramError::InvalidAccountOwner);
    }
    Ok(())
}

pub struct Mint;

impl Mint {
    #[inline(always)]
    pub fn check(account: &AccountView) -> Result<(), ProgramError> {
        if account.owner() != &pinocchio_token::ID {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if account.data_len() != pinocchio_token::state::Mint::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}

pub struct TokenAccount;

impl TokenAccount {
    #[inline(always)]
    pub fn check(account: &AccountView) -> Result<(), ProgramError> {
        if account.owner() != &pinocchio_token::ID {
            return Err(ProgramError::InvalidAccountOwner);
        }
        if account.data_len() != pinocchio_token::state::Account::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}

pub struct AssociatedTokenAccount;

impl AssociatedTokenAccount {
    #[inline(always)]
    pub fn derive(owner: &Address, mint: &Address, token_program: &Address) -> Address {
        let (expected, _bump) = Address::derive_program_address(
            &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
            &pinocchio_associated_token_account::ID,
        )
        .expect("ATA PDA derivation failed");
        expected
    }

    /// Correct address AND already a valid, existing token account.
    #[inline(always)]
    pub fn check(
        account: &AccountView,
        owner: &Address,
        mint: &Address,
        token_program: &Address,
    ) -> Result<(), ProgramError> {
        Self::check_address_only(account, owner, mint, token_program)?;
        TokenAccount::check(account)
    }

    /// Address-only — for accounts that may not exist yet (Anchor's `init_if_needed`).
    #[inline(always)]
    pub fn check_address_only(
        account: &AccountView,
        owner: &Address,
        mint: &Address,
        token_program: &Address,
    ) -> Result<(), ProgramError> {
        if account.address() != &Self::derive(owner, mint, token_program) {
            return Err(AmmError::InvalidToken.into());
        }
        Ok(())
    }
}
