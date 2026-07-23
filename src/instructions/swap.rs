use core::mem::size_of;

use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};

use constant_product_curve::{ConstantProduct, LiquidityPair};
use pinocchio_associated_token_account::instructions::CreateIdempotent;
use pinocchio_token::instructions::TransferChecked;

use crate::{
    constants::{CONFIG_SEED, LP_SEED},
    error::AmmError,
    helper::{AssociatedTokenAccount, signer_check},
    state::Config,
};

pub struct SwapAccounts<'a> {
    pub user: &'a AccountView,
    pub mint_x: &'a AccountView,
    pub mint_y: &'a AccountView,
    pub config: &'a AccountView,
    pub mint_lp: &'a AccountView,
    pub vault_x: &'a AccountView,
    pub vault_y: &'a AccountView,
    pub user_ata_x: &'a AccountView,
    pub user_ata_y: &'a AccountView,
    pub system_program: &'a AccountView,
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
            mint_lp,
            vault_x,
            vault_y,
            user_ata_x,
            user_ata_y,
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
        // Both user ATAs are `init_if_needed` — address-only check here,
        // created idempotently in run() before the swap.
        AssociatedTokenAccount::check_address_only(
            user_ata_x,
            user.address(),
            mint_x.address(),
            token_program.address(),
        )?;
        AssociatedTokenAccount::check_address_only(
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
            mint_lp,
            vault_x,
            vault_y,
            user_ata_x,
            user_ata_y,
            system_program,
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
        let (fee, seed, config_bump) = {
            let config_data = Config::load(self.accounts.config)?;
            if config_data.locked() {
                return Err(AmmError::PoolLocked.into());
            }
            (
                config_data.fee(),
                config_data.seed(),
                config_data.config_bump(),
            )
        };

        // Create either user ATA idempotently before we need them.
        CreateIdempotent {
            funding_account: self.accounts.user,
            account: self.accounts.user_ata_x,
            wallet: self.accounts.user,
            mint: self.accounts.mint_x,
            system_program: self.accounts.system_program,
            token_program: self.accounts.token_program,
        }
        .invoke()?;
        CreateIdempotent {
            funding_account: self.accounts.user,
            account: self.accounts.user_ata_y,
            wallet: self.accounts.user,
            mint: self.accounts.mint_y,
            system_program: self.accounts.system_program,
            token_program: self.accounts.token_program,
        }
        .invoke()?;

        let vault_x_amount =
            pinocchio_token::state::Account::from_account_view(self.accounts.vault_x)?.amount();
        let vault_y_amount =
            pinocchio_token::state::Account::from_account_view(self.accounts.vault_y)?.amount();
        let lp_supply =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_lp)?.supply();

        let mut curve = ConstantProduct::init(vault_x_amount, vault_y_amount, lp_supply, fee, None)
            .map_err(AmmError::from)?;

        let pair = if self.data.is_x {
            LiquidityPair::X
        } else {
            LiquidityPair::Y
        };
        let swap_result = curve
            .swap(pair, self.data.amount_in, self.data.min_amount_out)
            .map_err(AmmError::from)?;

        if swap_result.deposit == 0 || swap_result.withdraw == 0 {
            return Err(AmmError::InvalidAmount.into());
        }

        let mint_x_decimals =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_x)?.decimals();
        let mint_y_decimals =
            pinocchio_token::state::Mint::from_account_view(self.accounts.mint_y)?.decimals();

        let no_signers: &[&AccountView] = &[];

        // deposit leg: user → vault, user signs directly
        let (from_ata, to_vault, in_mint, in_decimals) = if self.data.is_x {
            (
                self.accounts.user_ata_x,
                self.accounts.vault_x,
                self.accounts.mint_x,
                mint_x_decimals,
            )
        } else {
            (
                self.accounts.user_ata_y,
                self.accounts.vault_y,
                self.accounts.mint_y,
                mint_y_decimals,
            )
        };

        TransferChecked {
            from: from_ata,
            mint: in_mint,
            to: to_vault,
            authority: self.accounts.user,
            multisig_signers: no_signers,
            amount: swap_result.deposit,
            decimals: in_decimals,
        }
        .invoke()?;

        // withdraw leg: vault → user, config PDA signs
        let (from_vault, to_ata, out_mint, out_decimals) = if self.data.is_x {
            (
                self.accounts.vault_y,
                self.accounts.user_ata_y,
                self.accounts.mint_y,
                mint_y_decimals,
            )
        } else {
            (
                self.accounts.vault_x,
                self.accounts.user_ata_x,
                self.accounts.mint_x,
                mint_x_decimals,
            )
        };

        let seed_bytes = seed.to_le_bytes();
        let bump_seed = [config_bump];
        let seeds = [
            Seed::from(CONFIG_SEED),
            Seed::from(&seed_bytes),
            Seed::from(&bump_seed),
        ];
        let signer = [Signer::from(&seeds)];

        TransferChecked {
            from: from_vault,
            mint: out_mint,
            to: to_ata,
            authority: self.accounts.config,
            multisig_signers: no_signers,
            amount: swap_result.withdraw,
            decimals: out_decimals,
        }
        .invoke_signed(&signer)
    }
}
