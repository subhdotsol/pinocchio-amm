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
// sha256("global:initialize")[..8]
const DISCRIMINATOR: [u8; 8] = [0xaf, 0xaf, 0x6d, 0x1f, 0x0d, 0x98, 0x9b, 0xed];

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

    let (config_pda, _config_bump) =
        Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &program_id);
    let (lp_pda, _lp_bump) =
        Pubkey::find_program_address(&[b"lp", config_pda.as_ref()], &program_id);

    let (vault_x, _) = Pubkey::find_program_address(
        &[
            config_pda.as_ref(),
            spl_token::ID.as_ref(),
            mint_x.as_ref(),
        ],
        &ata_program_id,
    );
    let (vault_y, _) = Pubkey::find_program_address(
        &[
            config_pda.as_ref(),
            spl_token::ID.as_ref(),
            mint_y.as_ref(),
        ],
        &ata_program_id,
    );

    let admin = Pubkey::new_unique();
    let admin_account = Account::new(100_000_000_000, 0, &system_program);
    let config_account = Account::new(0, 0, &system_program);
    let lp_mint_account = Account::new(0, 0, &system_program);
    let vault_x_account = Account::new(0, 0, &system_program);
    let vault_y_account = Account::new(0, 0, &system_program);

    // [discriminator(8), seed(8), fee(2), authority Option<Pubkey> = None(1)]
    let mut data = DISCRIMINATOR.to_vec();
    data.extend_from_slice(&seed.to_le_bytes());
    data.extend_from_slice(&fee.to_le_bytes());
    data.push(0u8); // authority = None

    let instruction = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(admin, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lp_pda, false),
            AccountMeta::new(vault_x, false),
            AccountMeta::new(vault_y, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
            AccountMeta::new_readonly(system_program, false),
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
        (token_program, token_program_account),
        (ata_program_id, ata_program_account),
        (system_program, system_program_account),
    ];

    MolluskComputeUnitBencher::new(mollusk)
        .bench(("initialize_anchor", &instruction, &accounts))
        .must_pass(true)
        .out_dir("target/benches")
        .execute();
}
