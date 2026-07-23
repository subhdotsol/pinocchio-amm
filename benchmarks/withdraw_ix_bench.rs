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

const CONFIG_LEN: usize = 109;

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

fn ata(owner: Pubkey, token_program: Pubkey, mint: Pubkey, ata_program: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[owner.as_ref(), token_program.as_ref(), mint.as_ref()],
        &ata_program,
    )
    .0
}

fn main() {
    let program_id: Pubkey = "2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE"
        .parse()
        .unwrap();
    let mut mollusk = Mollusk::new(&program_id, "tests/elfs/amm");

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
    let ata_program_account = program::create_program_account_loader_v3(&ata_program_id);

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

    let (config_pda, config_bump) =
        Pubkey::find_program_address(&[CONFIG_SEED, &seed.to_le_bytes()], &program_id);

    let (lp_pda, lp_bump) =
        Pubkey::find_program_address(&[LP_SEED, config_pda.as_ref()], &program_id);

    // Build config account data
    let mut config_data = vec![0u8; CONFIG_LEN];
    config_data[0..8].copy_from_slice(&seed.to_le_bytes());
    config_data[40..72].copy_from_slice(&mint_x.to_bytes());
    config_data[72..104].copy_from_slice(&mint_y.to_bytes());
    config_data[104..106].copy_from_slice(&fee.to_le_bytes());
    config_data[107] = config_bump;
    config_data[108] = lp_bump;

    let mut config_account = Account::new(
        mollusk.sysvars.rent.minimum_balance(CONFIG_LEN),
        CONFIG_LEN,
        &program_id,
    );
    config_account
        .data_as_mut_slice()
        .copy_from_slice(&config_data);

    // LP mint: supply = 100_000 (pool has existing liquidity)
    let mut lp_mint_account = Account::new(
        mollusk
            .sysvars
            .rent
            .minimum_balance(spl_token::state::Mint::LEN),
        spl_token::state::Mint::LEN,
        &token_program,
    );
    Pack::pack(
        Mint {
            mint_authority: COption::Some(config_pda),
            supply: 100_000,
            decimals: 6,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        lp_mint_account.data_as_mut_slice(),
    )
    .unwrap();

    // Vaults with existing liquidity
    let vault_x = ata(config_pda, token_program, mint_x, ata_program_id);
    let vault_y = ata(config_pda, token_program, mint_y, ata_program_id);
    let vault_x_account = make_token_account(&mollusk, mint_x, config_pda, 100_000, token_program);
    let vault_y_account = make_token_account(&mollusk, mint_y, config_pda, 100_000, token_program);

    // User accounts
    let user = Pubkey::new_unique();
    let user_ata_x = ata(user, token_program, mint_x, ata_program_id);
    let user_ata_y = ata(user, token_program, mint_y, ata_program_id);
    let user_ata_lp = ata(user, token_program, lp_pda, ata_program_id);

    let user_account = Account::new(10_000_000_000, 0, &system_program);
    // User has LP tokens to burn; destination ATAs are pre-initialized
    let user_ata_x_account = make_token_account(&mollusk, mint_x, user, 0, token_program);
    let user_ata_y_account = make_token_account(&mollusk, mint_y, user, 0, token_program);
    let user_ata_lp_account = make_token_account(&mollusk, lp_pda, user, 50_000, token_program);

    // Instruction data: [discriminator(1), amount(8), min_x(8), min_y(8)]
    let lp_amount: u64 = 10_000; // LP tokens to burn
    let min_x: u64 = 0;
    let min_y: u64 = 0;
    let mut data = vec![3u8]; // discriminator = 3 for Withdraw
    data.extend_from_slice(&lp_amount.to_le_bytes());
    data.extend_from_slice(&min_x.to_le_bytes());
    data.extend_from_slice(&min_y.to_le_bytes());

    let instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lp_pda, false),
            AccountMeta::new(vault_x, false),
            AccountMeta::new(vault_y, false),
            AccountMeta::new(user_ata_x, false),
            AccountMeta::new(user_ata_y, false),
            AccountMeta::new(user_ata_lp, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
        ],
        data,
    };

    let accounts = vec![
        (user, user_account),
        (mint_x, mint_x_account),
        (mint_y, mint_y_account),
        (config_pda, config_account),
        (lp_pda, lp_mint_account),
        (vault_x, vault_x_account),
        (vault_y, vault_y_account),
        (user_ata_x, user_ata_x_account),
        (user_ata_y, user_ata_y_account),
        (user_ata_lp, user_ata_lp_account),
        (system_program, system_program_account),
        (token_program, token_program_account),
        (ata_program_id, ata_program_account),
    ];

    MolluskComputeUnitBencher::new(mollusk)
        .bench(("withdraw", &instruction, &accounts))
        .must_pass(true)
        .out_dir("target/benches")
        .execute();
}
