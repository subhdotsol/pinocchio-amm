use core::mem::size_of;

use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};

use constant_product_curve::ConstantProduct;
use pinocchio_token::instructions::{MintTo, Transfer};

use crate::{
    constants::{CONFIG_SEED, CURVE_PRECISION, LP_SEED},
    error::AmmError,
    helper::{AssociatedTokenAccount, signer_check},
    state::Config,
};

pub struct DepositAccounts<'a> {
    pub user: &'a AccountView,
    pub mint_x: &'a AccountView,
    pub mint_y: &'a AccountView,
    pub config: &'a mut AccountView,
    pub mint_lp: &'a AccountView,
    pub vault_x: &'a AccountView,
    pub vault_y: &'a AccountView,
    pub user_ata_x: &'a AccountView,
    pub user_ata_y: &'a AccountView,
    pub user_ata_lp: &'a AccountView,
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a mut [AccountView]> for DepositAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a mut [AccountView]) -> Result<Self, Self::Error> {
        let [
            user,
            mint_x,
            mint_y,
            config,
            mint_lp,
            vault_x,
            vault_y,
            user_ata_x,
            user_ata_y,
            user_ata_lp,
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
            let (expected_config, _) = Address::derive_program_address(
                &[CONFIG_SEED, &config_data.seed().to_le_bytes()],
                &crate::ID,
            )
            .ok_or(ProgramError::InvalidSeeds)?;
            if config.address() != &expected_config {
                return Err(ProgramError::InvalidSeeds);
            }
            let (expected_lp, _) =
                Address::derive_program_address(&[LP_SEED, config.address().as_ref()], &crate::ID)
                    .ok_or(ProgramError::InvalidSeeds)?;
            if mint_lp.address() != &expected_lp {
                return Err(ProgramError::InvalidSeeds);
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
        AssociatedTokenAccount::check(
            user_ata_lp,
            user.address(),
            mint_lp.address(),
            token_program.address(),
        )?;

        Ok(Self {
            user,
            mint_x,
            mint_y,
            config,
            mint_lp,
            vault_x,
            vault_y,
            user_ata_x,
            user_ata_y,
            user_ata_lp,
            token_program,
        })
    }
}

pub struct DepositInstructionData {
    pub amount: u64,
    pub max_x: u64,
    pub max_y: u64,
}

impl<'a> TryFrom<&'a [u8]> for DepositInstructionData {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<u64>() * 3 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let max_x = u64::from_le_bytes(data[8..16].try_into().unwrap());
        let max_y = u64::from_le_bytes(data[16..24].try_into().unwrap());

        if amount == 0 {
            return Err(AmmError::InvalidAmount.into());
        }

        Ok(Self {
            amount,
            max_x,
            max_y,
        })
    }
}

pub struct Deposit<'a> {
    pub accounts: DepositAccounts<'a>,
    pub data: DepositInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a mut [AccountView])> for Deposit<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a mut [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: DepositAccounts::try_from(accounts)?,
            data: DepositInstructionData::try_from(data)?,
        })
    }
}

impl<'a> Deposit<'a> {
    pub const DISCRIMINATOR: u8 = 1;

    pub fn process(
        _program_id: &Address,
        accounts: &'a mut [AccountView],
        data: &'a [u8],
    ) -> ProgramResult {
        let mut ix = Self::try_from((data, accounts))?;
        ix.run()
    }

    fn run(&mut self) -> ProgramResult {
        // Phase 1: read everything from config in a single borrow.
        let (seed, config_bump, reserve_x, reserve_y) = {
            let config_data = Config::load(self.accounts.config)?;
            if config_data.locked() {
                return Err(AmmError::PoolLocked.into());
            }
            (
                config_data.seed(),
                config_data.config_bump(),
                config_data.reserve_x(),
                config_data.reserve_y(),
            )
        };

        let lp_supply =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_lp)?.supply();

        let (x, y) = if lp_supply == 0 && reserve_x == 0 && reserve_y == 0 {
            (self.data.max_x, self.data.max_y)
        } else {
            let amounts = ConstantProduct::xy_deposit_amounts_from_l(
                reserve_x,
                reserve_y,
                lp_supply,
                self.data.amount,
                CURVE_PRECISION as u32,
            )
            .map_err(AmmError::from)?;
            (amounts.x, amounts.y)
        };

        if x > self.data.max_x || y > self.data.max_y {
            return Err(AmmError::SlippageExceeded.into());
        }

        // Phase 2: CPIs — all config borrows must be released before this.
        let no_signers: &[&AccountView] = &[];

        Transfer {
            from: self.accounts.user_ata_x,
            to: self.accounts.vault_x,
            authority: self.accounts.user,
            multisig_signers: no_signers,
            amount: x,
        }
        .invoke()?;

        Transfer {
            from: self.accounts.user_ata_y,
            to: self.accounts.vault_y,
            authority: self.accounts.user,
            multisig_signers: no_signers,
            amount: y,
        }
        .invoke()?;

        let seed_bytes = seed.to_le_bytes();
        let bump_seed = [config_bump];
        let seeds = [
            Seed::from(CONFIG_SEED),
            Seed::from(&seed_bytes),
            Seed::from(&bump_seed),
        ];
        let signer = [Signer::from(&seeds)];

        let no_signers: &[&AccountView] = &[];
        MintTo {
            mint: self.accounts.mint_lp,
            account: self.accounts.user_ata_lp,
            mint_authority: self.accounts.config,
            multisig_signers: no_signers,
            amount: self.data.amount,
        }
        .invoke_signed(&signer)?;

        // Phase 3: update cached reserves.
        let mut config_data = Config::load_mut(self.accounts.config)?;
        config_data.set_reserve_x(
            reserve_x
                .checked_add(x)
                .ok_or(ProgramError::ArithmeticOverflow)?,
        );
        config_data.set_reserve_y(
            reserve_y
                .checked_add(y)
                .ok_or(ProgramError::ArithmeticOverflow)?,
        );

        Ok(())
    }
}
