use core::mem::size_of;

use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};

use pinocchio_token::instructions::Transfer;

use crate::{
    constants::CONFIG_SEED,
    error::AmmError,
    helper::{AssociatedTokenAccount, signer_check},
    state::Config,
};

pub struct SwapAccounts<'a> {
    pub user: &'a AccountView,
    pub mint_x: &'a AccountView,
    pub mint_y: &'a AccountView,
    pub config: &'a mut AccountView,
    pub vault_x: &'a AccountView,
    pub vault_y: &'a AccountView,
    pub user_ata_x: &'a AccountView,
    pub user_ata_y: &'a AccountView,
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a mut [AccountView]> for SwapAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a mut [AccountView]) -> Result<Self, Self::Error> {
        let [
            user,
            mint_x,
            mint_y,
            config,
            vault_x,
            vault_y,
            user_ata_x,
            user_ata_y,
            token_program,
            ..,
        ] = accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        signer_check(user)?;

        {
            let config_data = Config::load(config)?;
            if config_data.mint_x() != mint_x.address() || config_data.mint_y() != mint_y.address()
            {
                return Err(ProgramError::InvalidAccountData);
            }
        }

        AssociatedTokenAccount::check(
            vault_x,
            config.address(),
            mint_x.address(),
            token_program.address(),
        )?;
        AssociatedTokenAccount::check(
            vault_y,
            config.address(),
            mint_y.address(),
            token_program.address(),
        )?;
        AssociatedTokenAccount::check(
            user_ata_x,
            user.address(),
            mint_x.address(),
            token_program.address(),
        )?;
        AssociatedTokenAccount::check(
            user_ata_y,
            user.address(),
            mint_y.address(),
            token_program.address(),
        )?;

        Ok(Self {
            user,
            mint_x,
            mint_y,
            config,
            vault_x,
            vault_y,
            user_ata_x,
            user_ata_y,
            token_program,
        })
    }
}

pub struct SwapInstructionData {
    pub is_x: bool,
    pub amount_in: u64,
    pub min_amount_out: u64,
}

impl<'a> TryFrom<&'a [u8]> for SwapInstructionData {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() != 1 + size_of::<u64>() * 2 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let is_x = match data[0] {
            0 => false,
            1 => true,
            _ => return Err(ProgramError::InvalidInstructionData),
        };
        let amount_in = u64::from_le_bytes(data[1..9].try_into().unwrap());
        let min_amount_out = u64::from_le_bytes(data[9..17].try_into().unwrap());

        if amount_in == 0 {
            return Err(AmmError::InvalidAmount.into());
        }

        Ok(Self {
            is_x,
            amount_in,
            min_amount_out,
        })
    }
}

pub struct Swap<'a> {
    pub accounts: SwapAccounts<'a>,
    pub data: SwapInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a mut [AccountView])> for Swap<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a mut [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: SwapAccounts::try_from(accounts)?,
            data: SwapInstructionData::try_from(data)?,
        })
    }
}

impl<'a> Swap<'a> {
    pub const DISCRIMINATOR: u8 = 2;

    pub fn process(
        _program_id: &Address,
        accounts: &'a mut [AccountView],
        data: &'a [u8],
    ) -> ProgramResult {
        let mut ix = Self::try_from((data, accounts))?;
        ix.run()
    }

    fn run(&mut self) -> ProgramResult {
        // Phase 1: read config in a single borrow.
        let (fee, reserve_x, reserve_y, seed, config_bump) = {
            let config_data = Config::load(self.accounts.config)?;
            if config_data.locked() {
                return Err(AmmError::PoolLocked.into());
            }
            (
                config_data.fee(),
                config_data.reserve_x(),
                config_data.reserve_y(),
                config_data.seed(),
                config_data.config_bump(),
            )
        };

        // Phase 2: inline constant-product AMM formula (no library, no LP supply read).
        let (reserve_in, reserve_out) = if self.data.is_x {
            (reserve_x, reserve_y)
        } else {
            (reserve_y, reserve_x)
        };

        let amount_in_with_fee = self
            .data
            .amount_in
            .checked_mul(
                10_000u64
                    .checked_sub(fee as u64)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            )
            .ok_or(ProgramError::ArithmeticOverflow)?
            .checked_div(10_000)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        let amount_out = reserve_out
            .checked_mul(amount_in_with_fee)
            .ok_or(ProgramError::ArithmeticOverflow)?
            .checked_div(
                reserve_in
                    .checked_add(amount_in_with_fee)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            )
            .ok_or(ProgramError::ArithmeticOverflow)?;

        if amount_out == 0 {
            return Err(AmmError::InvalidAmount.into());
        }
        if amount_out < self.data.min_amount_out {
            return Err(AmmError::SlippageExceeded.into());
        }

        // Phase 3: CPIs — config borrows already released.
        let seed_bytes = seed.to_le_bytes();
        let bump_seed = [config_bump];
        let seeds = [
            Seed::from(CONFIG_SEED),
            Seed::from(&seed_bytes),
            Seed::from(&bump_seed),
        ];
        let signer = [Signer::from(&seeds)];

        let (from_user, to_vault, from_vault, to_user) = if self.data.is_x {
            (
                self.accounts.user_ata_x,
                self.accounts.vault_x,
                self.accounts.vault_y,
                self.accounts.user_ata_y,
            )
        } else {
            (
                self.accounts.user_ata_y,
                self.accounts.vault_y,
                self.accounts.vault_x,
                self.accounts.user_ata_x,
            )
        };

        let no_signers: &[&AccountView] = &[];

        Transfer {
            from: from_user,
            to: to_vault,
            authority: self.accounts.user,
            multisig_signers: no_signers,
            amount: self.data.amount_in,
        }
        .invoke()?;

        Transfer {
            from: from_vault,
            to: to_user,
            authority: self.accounts.config,
            multisig_signers: no_signers,
            amount: amount_out,
        }
        .invoke_signed(&signer)?;

        // Phase 4: update cached reserves.
        let mut config_data = Config::load_mut(self.accounts.config)?;
        if self.data.is_x {
            config_data.set_reserve_x(
                reserve_x
                    .checked_add(self.data.amount_in)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            );
            config_data.set_reserve_y(
                reserve_y
                    .checked_sub(amount_out)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            );
        } else {
            config_data.set_reserve_y(
                reserve_y
                    .checked_add(self.data.amount_in)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            );
            config_data.set_reserve_x(
                reserve_x
                    .checked_sub(amount_out)
                    .ok_or(ProgramError::ArithmeticOverflow)?,
            );
        }

        Ok(())
    }
}
