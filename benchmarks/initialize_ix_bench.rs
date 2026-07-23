use amm::constants::{CONFIG_SEED, LP_SEED};
use mollusk_svm::{Mollusk, program};
use mollusk_svm_bencher::MolluskComputeUnitBencher;
use solana_sdk::{
    account::{Account, WritableAccount},
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::Mint;

#[allow(deprecated)]
fn set_pinocchio_rent(mollusk: &mut Mollusk) {
    // pinocchio reads only lamports_per_byte from sysvar (no exemption_threshold);
    // set both so SPL token and pinocchio agree on minimum_balance.
    mollusk.sysvars.rent.lamports_per_byte_year = 6960;
    mollusk.sysvars.rent.exemption_threshold = 1.0;
}

fn make_token_account(
    mollusk: &Mollusk,
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    token_program: Pubkey,
) -> Account {
    let mut account = Account::new(
        mollusk
            .sysvars
            .rent
            .minimum_balance(spl_token::state::Account::LEN),
        spl_token::state::Account::LEN,
        &token_program,
    );
    Pack::pack(
        spl_token::state::Account {
            mint,
            owner,
            amount,
            delegate: COption::None,
            state: spl_token::state::AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        account.data_as_mut_slice(),
    )
    .unwrap();
    account
}

fn main() {
    let program_id: Pubkey = "2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE"
        .parse()
        .unwrap();
    let mut mollusk = Mollusk::new(&program_id, "tests/elfs/amm");
    set_pinocchio_rent(&mut mollusk);

    mollusk.add_program(&spl_token::ID, "tests/elfs/spl_token");

    let ata_program_id: Pubkey = Pubkey::new_from_array(
        pinocchio_associated_token_account::ID
            .as_ref()
            .try_into()
            .unwrap(),
    );
    mollusk.add_program(&ata_program_id, "tests/elfs/spl_associated_token_account");

    let (system_program, system_program_account) = program::keyed_account_for_system_program();
    let token_program = spl_token::ID;
    let token_program_account = program::create_program_account_loader_v3(&token_program);

    let seed: u64 = 12345;
    let fee: u16 = 30;

    let mint_x = Pubkey::new_from_array([0x03; 32]);
    let mut mint_x_account = Account::new(
        mollusk
            .sysvars
            .rent
            .minimum_balance(spl_token::state::Mint::LEN),
        spl_token::state::Mint::LEN,
        &token_program,
    );
    Pack::pack(
        Mint {
            mint_authority: COption::None,
            supply: 100_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        mint_x_account.data_as_mut_slice(),
    )
    .unwrap();

    let mint_y = Pubkey::new_from_array([0x02; 32]);
    let mut mint_y_account = Account::new(
        mollusk
            .sysvars
            .rent
            .minimum_balance(spl_token::state::Mint::LEN),
        spl_token::state::Mint::LEN,
        &token_program,
    );
    Pack::pack(
        Mint {
            mint_authority: COption::None,
            supply: 100_000_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        mint_y_account.data_as_mut_slice(),
    )
    .unwrap();

    let (config_pda, _config_bump) =
        Pubkey::find_program_address(&[CONFIG_SEED, &seed.to_le_bytes()], &program_id);

    let (lp_pda, _lp_bump) =
        Pubkey::find_program_address(&[LP_SEED, config_pda.as_ref()], &program_id);

    let (vault_x, _) = Pubkey::find_program_address(
        &[config_pda.as_ref(), spl_token::ID.as_ref(), mint_x.as_ref()],
        &ata_program_id,
    );
    let (vault_y, _) = Pubkey::find_program_address(
        &[config_pda.as_ref(), spl_token::ID.as_ref(), mint_y.as_ref()],
        &ata_program_id,
    );

    let admin = Pubkey::new_unique();
    let admin_account = Account::new(100_000_000_000, 0, &system_program);

    // config and lp_mint start empty — created by the instruction.
    let config_account = Account::new(0, 0, &system_program);
    let lp_mint_account = Account::new(0, 0, &system_program);

    // Vaults are pre-created by the client before calling initialize.
    let vault_x_account = make_token_account(&mollusk, mint_x, config_pda, 0, token_program);
    let vault_y_account = make_token_account(&mollusk, mint_y, config_pda, 0, token_program);

    let mut data = vec![0u8]; // discriminator = 0 for Initialize
    data.extend_from_slice(&seed.to_le_bytes());
    data.extend_from_slice(&fee.to_le_bytes());
    data.extend_from_slice(&[0u8; 32]); // no authority

    let instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lp_pda, false),
            AccountMeta::new_readonly(vault_x, false),
            AccountMeta::new_readonly(vault_y, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(token_program, false),
        ],
        data,
    };

    let accounts = vec![
        (admin, admin_account),
        (mint_x, mint_x_account),
        (mint_y, mint_y_account),
        (config_pda, config_account),
        (lp_pda, lp_mint_account),
        (vault_x, vault_x_account),
        (vault_y, vault_y_account),
        (system_program, system_program_account),
        (token_program, token_program_account),
    ];

    MolluskComputeUnitBencher::new(mollusk)
        .bench(("initialize", &instruction, &accounts))
        .must_pass(true)
        .out_dir("target/benches")
        .execute();
}
