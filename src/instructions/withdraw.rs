use core::mem::size_of;

use constant_product_curve::ConstantProduct;
use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};
use pinocchio_associated_token_account::instructions::CreateIdempotent;
use pinocchio_token::instructions::{Burn, TransferChecked};

use crate::{
    constants::{CONFIG_SEED, CURVE_PRECISION, LP_SEED},
    error::AmmError,
    helper::{AssociatedTokenAccount, signer_check},
    state::Config,
};

pub struct WithdrawAccounts<'a> {
    pub user: &'a AccountView,
    pub mint_x: &'a AccountView,
    pub mint_y: &'a AccountView,
    pub config: &'a AccountView,
    pub mint_lp: &'a AccountView,
    pub vault_x: &'a AccountView,
    pub vault_y: &'a AccountView,
    pub user_ata_x: &'a AccountView,
    pub user_ata_y: &'a AccountView,
    pub user_ata_lp: &'a AccountView,
    pub system_program: &'a AccountView,
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a mut [AccountView]> for WithdrawAccounts<'a> {
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
            system_program,
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
        AssociatedTokenAccount::check_address_only(
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
            system_program,
            token_program,
        })
    }
}

pub struct WithdrawInstructionData {
    pub amount: u64,
    pub min_x: u64,
    pub min_y: u64,
}

impl<'a> TryFrom<&'a [u8]> for WithdrawInstructionData {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<u64>() * 3 {
            return Err(ProgramError::InvalidInstructionData);
        }
        let amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let min_x = u64::from_le_bytes(data[8..16].try_into().unwrap());
        let min_y = u64::from_le_bytes(data[16..24].try_into().unwrap());

        if amount == 0 {
            return Err(AmmError::InvalidAmount.into());
        }

        Ok(Self { amount, min_x, min_y })
    }
}

pub struct Withdraw<'a> {
    pub accounts: WithdrawAccounts<'a>,
    pub data: WithdrawInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a mut [AccountView])> for Withdraw<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a mut [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: WithdrawAccounts::try_from(accounts)?,
            data: WithdrawInstructionData::try_from(data)?,
        })
    }
}

impl<'a> Withdraw<'a> {
    pub const DISCRIMINATOR: u8 = 3;

    pub fn process(
        _program_id: &Address,
        accounts: &'a mut [AccountView],
        data: &'a [u8],
    ) -> ProgramResult {
        let mut ix = Self::try_from((data, accounts))?;
        ix.run()
    }

    fn run(&mut self) -> ProgramResult {
        let (seed, config_bump) = {
            let config_data = Config::load(self.accounts.config)?;
            if config_data.locked() {
                return Err(AmmError::PoolLocked.into());
            }
            (config_data.seed(), config_data.config_bump())
        };

        let vault_x_amount =
            pinocchio_token::state::Account::from_account_view(self.accounts.vault_x)?.amount();
        let vault_y_amount =
            pinocchio_token::state::Account::from_account_view(self.accounts.vault_y)?.amount();
        let lp_supply =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_lp)?.supply();

        let (x, y) = if lp_supply == 0 && vault_x_amount == 0 && vault_y_amount == 0 {
            (self.data.min_x, self.data.min_y)
        } else {
            let amounts = ConstantProduct::xy_withdraw_amounts_from_l(
                vault_x_amount,
                vault_y_amount,
                lp_supply,
                self.data.amount,
                CURVE_PRECISION as u32,
            )
            .map_err(AmmError::from)?;
            (amounts.x, amounts.y)
        };

        if x < self.data.min_x || y < self.data.min_y {
            return Err(AmmError::SlippageExceeded.into());
        }

        let no_signers: &[&AccountView] = &[];

        CreateIdempotent {
            funding_account: self.accounts.user,
            account: self.accounts.user_ata_lp,
            wallet: self.accounts.user,
            mint: self.accounts.mint_lp,
            system_program: self.accounts.system_program,
            token_program: self.accounts.token_program,
        }
        .invoke()?;

        // Burn is authorized by the user directly — they own the LP tokens.
        Burn {
            mint: self.accounts.mint_lp,
            account: self.accounts.user_ata_lp,
            authority: self.accounts.user,
            multisig_signers: no_signers,
            amount: self.data.amount,
        }
        .invoke()?;

        let mint_x_decimals =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_x)?.decimals();
        let mint_y_decimals =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_y)?.decimals();

        let seed_bytes = seed.to_le_bytes();
        let bump_seed = [config_bump];
        let seeds = [
            Seed::from(CONFIG_SEED),
            Seed::from(&seed_bytes),
            Seed::from(&bump_seed),
        ];
        let signer = [Signer::from(&seeds)];

        TransferChecked {
            from: self.accounts.vault_x,
            mint: self.accounts.mint_x,
            to: self.accounts.user_ata_x,
            authority: self.accounts.config,
            multisig_signers: no_signers,
            amount: x,
            decimals: mint_x_decimals,
        }
        .invoke_signed(&signer)?;

        TransferChecked {
            from: self.accounts.vault_y,
            mint: self.accounts.mint_y,
            to: self.accounts.user_ata_y,
            authority: self.accounts.config,
            multisig_signers: no_signers,
            amount: y,
            decimals: mint_y_decimals,
        }
        .invoke_signed(&signer)
    }
}
