use mollusk_svm::{program, Mollusk};
use mollusk_svm_bencher::MolluskComputeUnitBencher;
use solana_sdk::{
    account::{Account, WritableAccount},
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::Mint;

const ANCHOR_AMM_ID: &str = "9a95ZYK3AT5HcivR5X59niNgqdYP9oE5XqomA2kNHWRV";
// sha256("global:swap")[..8]
const DISCRIMINATOR: [u8; 8] = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x87, 0xc8];
const CONFIG_DISC: [u8; 8] = [0x9b, 0x0c, 0xaa, 0xe0, 0x1e, 0xfa, 0xcc, 0x82];
const CONFIG_LEN: usize = 118;

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
    let program_id: Pubkey = ANCHOR_AMM_ID.parse().unwrap();
    let mut mollusk = Mollusk::new(&program_id, "tests/elfs/amm_anchor");

    mollusk.add_program(&spl_token::ID, "tests/elfs/spl_token");

    let ata_program_id: Pubkey = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
        .parse()
        .unwrap();
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
        Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &program_id);
    let (lp_pda, lp_bump) =
        Pubkey::find_program_address(&[b"lp", config_pda.as_ref()], &program_id);

    let mut config_data = vec![0u8; CONFIG_LEN];
    config_data[0..8].copy_from_slice(&CONFIG_DISC);
    config_data[8..16].copy_from_slice(&seed.to_le_bytes());
    config_data[16] = 0; // authority = None
    config_data[17..49].copy_from_slice(&mint_x.to_bytes());
    config_data[49..81].copy_from_slice(&mint_y.to_bytes());
    config_data[81..83].copy_from_slice(&fee.to_le_bytes());
    config_data[83] = 0; // locked = false
    config_data[84] = config_bump;
    config_data[85] = lp_bump;

    let mut config_account = Account::new(
        mollusk.sysvars.rent.minimum_balance(CONFIG_LEN),
        CONFIG_LEN,
        &program_id,
    );
    config_account
        .data_as_mut_slice()
        .copy_from_slice(&config_data);

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

    let vault_x = ata(config_pda, token_program, mint_x, ata_program_id);
    let vault_y = ata(config_pda, token_program, mint_y, ata_program_id);
    let vault_x_account = make_token_account(&mollusk, mint_x, config_pda, 100_000, token_program);
    let vault_y_account = make_token_account(&mollusk, mint_y, config_pda, 100_000, token_program);

    let user = Pubkey::new_unique();
    let user_ata_x = ata(user, token_program, mint_x, ata_program_id);
    let user_ata_y = ata(user, token_program, mint_y, ata_program_id);

    let user_account = Account::new(10_000_000_000, 0, &system_program);
    let user_ata_x_account = make_token_account(&mollusk, mint_x, user, 100_000, token_program);
    let user_ata_y_account = make_token_account(&mollusk, mint_y, user, 0, token_program);

    // [discriminator(8), is_x(1), amount_in(8), min_amount_out(8)]
    let is_x: u8 = 1;
    let amount_in: u64 = 10_000;
    let min_amount_out: u64 = 1;
    let mut data = DISCRIMINATOR.to_vec();
    data.push(is_x);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());

    let instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new_readonly(config_pda, false),
            AccountMeta::new_readonly(lp_pda, false),
            AccountMeta::new(vault_x, false),
            AccountMeta::new(vault_y, false),
            AccountMeta::new(user_ata_x, false),
            AccountMeta::new(user_ata_y, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
            AccountMeta::new_readonly(system_program, false),
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
        (token_program, token_program_account),
        (ata_program_id, ata_program_account),
        (system_program, system_program_account),
    ];

    MolluskComputeUnitBencher::new(mollusk)
        .bench(("swap_anchor", &instruction, &accounts))
        .must_pass(true)
        .out_dir("target/benches")
        .execute();
}
