use core::mem::size_of;

use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{Sysvar, rent::Rent},
};
use pinocchio_associated_token_account::instructions::Create as CreateAta;
use pinocchio_system::instructions::CreateAccount;
use pinocchio_token::instructions::InitializeMint2;

use crate::{
    constants::{CONFIG_SEED, LP_DECIMALS, LP_SEED},
    error::AmmError,
    helper::{Mint, signer_check, system_account_check},
    state::Config,
};

pub struct InitializeAccounts<'a> {
    pub admin: &'a AccountView,
    pub mint_x: &'a AccountView,
    pub mint_y: &'a AccountView,
    pub config: &'a mut AccountView,
    pub mint_lp: &'a AccountView,
    pub vault_x: &'a AccountView,
    pub vault_y: &'a AccountView,
    pub system_program: &'a AccountView,
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a mut [AccountView]> for InitializeAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a mut [AccountView]) -> Result<Self, Self::Error> {
        let [
            admin,
            mint_x,
            mint_y,
            config,
            mint_lp,
            vault_x,
            vault_y,
            system_program,
            token_program,
            _associated_token_program,
            ..,
        ] = accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        signer_check(admin)?;
        Mint::check(mint_x)?;
        Mint::check(mint_y)?;
        system_account_check(config)?; // must not already exist
        system_account_check(mint_lp)?; // must not already exist

        Ok(Self {
            admin,
            mint_x,
            mint_y,
            config,
            mint_lp,
            vault_x,
            vault_y,
            system_program,
            token_program,
        })
    }
}

pub struct InitializeInstructionData {
    pub seed: u64,
    pub fee: u16,
    pub authority: Option<Address>,
}

impl<'a> TryFrom<&'a [u8]> for InitializeInstructionData {
    type Error = ProgramError;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        const LEN: usize = size_of::<u64>() + size_of::<u16>() + 32;
        if data.len() != LEN {
            return Err(ProgramError::InvalidInstructionData);
        }

        let seed = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let fee = u16::from_le_bytes(data[8..10].try_into().unwrap());

        let mut authority_bytes = [0u8; 32];
        authority_bytes.copy_from_slice(&data[10..42]);
        let authority = if authority_bytes == [0u8; 32] {
            None
        } else {
            Some(Address::new_from_array(authority_bytes))
        };

        if fee >= 10_000 {
            return Err(AmmError::InvalidFee.into());
        }

        Ok(Self {
            seed,
            fee,
            authority,
        })
    }
}

pub struct Initialize<'a> {
    pub accounts: InitializeAccounts<'a>,
    pub data: InitializeInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a mut [AccountView])> for Initialize<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a mut [AccountView])) -> Result<Self, Self::Error> {
        Ok(Self {
            accounts: InitializeAccounts::try_from(accounts)?,
            data: InitializeInstructionData::try_from(data)?,
        })
    }
}

impl<'a> Initialize<'a> {
    pub const DISCRIMINATOR: u8 = 0;

    pub fn process(
        _program_id: &Address,
        accounts: &'a mut [AccountView],
        data: &'a [u8],
    ) -> ProgramResult {
        let mut ix = Self::try_from((data, accounts))?;
        ix.run()
    }

    fn run(&mut self) -> ProgramResult {
        let seed_bytes = self.data.seed.to_le_bytes();

        let (config_pda, config_bump) =
            Address::derive_program_address(&[CONFIG_SEED, &seed_bytes], &crate::ID)
                .ok_or(ProgramError::InvalidSeeds)?;
        if self.accounts.config.address() != &config_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        let (lp_pda, lp_bump) = Address::derive_program_address(
            &[LP_SEED, self.accounts.config.address().as_ref()],
            &crate::ID,
        )
        .ok_or(ProgramError::InvalidSeeds)?;
        if self.accounts.mint_lp.address() != &lp_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        //  create the config account
        let config_bump_seed = [config_bump];
        let config_seeds = [
            Seed::from(CONFIG_SEED),
            Seed::from(&seed_bytes),
            Seed::from(&config_bump_seed),
        ];
        let config_signer = [Signer::from(&config_seeds)];

        CreateAccount {
            from: self.accounts.admin,
            to: self.accounts.config,
            lamports: Rent::get()?.try_minimum_balance(Config::LEN)?,
            space: Config::LEN as u64,
            owner: &crate::ID,
        }
        .invoke_signed(&config_signer)?;

        // create + initialize the LP mint
        let lp_bump_seed = [lp_bump];
        let lp_seeds = [
            Seed::from(LP_SEED),
            Seed::from(self.accounts.config.address().as_ref()),
            Seed::from(&lp_bump_seed),
        ];
        let lp_signer = [Signer::from(&lp_seeds)];

        CreateAccount {
            from: self.accounts.admin,
            to: self.accounts.mint_lp,
            lamports: Rent::get()?.try_minimum_balance(pinocchio_token::state::Mint::LEN)?,
            space: pinocchio_token::state::Mint::LEN as u64,
            owner: &pinocchio_token::ID,
        }
        .invoke_signed(&lp_signer)?;

        InitializeMint2 {
            mint: self.accounts.mint_lp,
            decimals: LP_DECIMALS,
            mint_authority: self.accounts.config.address(),
            freeze_authority: None,
        }
        .invoke()?;

        // create the two pool vault ATAs, owned by the config PDA
        CreateAta {
            funding_account: self.accounts.admin,
            account: self.accounts.vault_x,
            wallet: self.accounts.config,
            mint: self.accounts.mint_x,
            system_program: self.accounts.system_program,
            token_program: self.accounts.token_program,
        }
        .invoke()?;

        CreateAta {
            funding_account: self.accounts.admin,
            account: self.accounts.vault_y,
            wallet: self.accounts.config,
            mint: self.accounts.mint_y,
            system_program: self.accounts.system_program,
            token_program: self.accounts.token_program,
        }
        .invoke()?;

        // write pool state
        let mut config_data = Config::load_mut(self.accounts.config)?;
        config_data.set_inner(
            self.data.seed,
            self.data.authority,
            *self.accounts.mint_x.address(),
            *self.accounts.mint_y.address(),
            self.data.fee,
            config_bump,
            lp_bump,
        )?;

        Ok(())
    }
}
