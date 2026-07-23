use mollusk_svm::{program, Mollusk};
use solana_sdk::{
    account::{Account, WritableAccount},
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::Mint;
use std::{fs, path::Path};

// Pinocchio AMM: 1-byte discriminators
const P_INIT: u8 = 0;
const P_DEPOSIT: u8 = 1;
const P_SWAP: u8 = 2;
const P_WITHDRAW: u8 = 3;

// Anchor AMM: sha256("global:<name>")[..8]
const A_INIT_DISC: [u8; 8] = [0xaf, 0xaf, 0x6d, 0x1f, 0x0d, 0x98, 0x9b, 0xed];
const A_DEPOSIT_DISC: [u8; 8] = [0xf2, 0x23, 0xc6, 0x89, 0x52, 0xe1, 0xf2, 0xb6];
const A_SWAP_DISC: [u8; 8] = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x87, 0xc8];
const A_WITHDRAW_DISC: [u8; 8] = [0xb7, 0x12, 0x46, 0x9c, 0x94, 0x6d, 0xa1, 0x22];

// sha256("account:Config")[..8]
const ANCHOR_CONFIG_DISC: [u8; 8] = [0x9b, 0x0c, 0xaa, 0xe0, 0x1e, 0xfa, 0xcc, 0x82];

const PINOCCHIO_AMM_ID: &str = "2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE";
const ANCHOR_AMM_ID: &str = "9a95ZYK3AT5HcivR5X59niNgqdYP9oE5XqomA2kNHWRV";
const ATA_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

// Pinocchio Config: 109 bytes, no discriminator
const P_CONFIG_LEN: usize = 109;
// Anchor Config: 8 disc + 110 InitSpace = 118 bytes
const A_CONFIG_LEN: usize = 118;

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

fn run(mollusk: &Mollusk, name: &str, ix: &Instruction, accounts: &[(Pubkey, Account)]) -> u64 {
    let result = mollusk.process_instruction(ix, accounts);
    assert!(
        result.raw_result.is_ok(),
        "{name} failed: {:?}",
        result.raw_result
    );
    result.compute_units_consumed
}

fn main() {
    let seed: u64 = 12345;
    let fee: u16 = 30;

    let mint_x = Pubkey::new_from_array([0x03; 32]);
    let mint_y = Pubkey::new_from_array([0x02; 32]);
    let ata_program_id: Pubkey = ATA_PROGRAM_ID.parse().unwrap();

    // Pinocchio
    let p_program_id: Pubkey = PINOCCHIO_AMM_ID.parse().unwrap();
    let mut p_mollusk = Mollusk::new(&p_program_id, "tests/elfs/amm");
    p_mollusk.sysvars.rent.lamports_per_byte_year = 6960;
    p_mollusk.sysvars.rent.exemption_threshold = 1.0;
    p_mollusk.add_program(&spl_token::ID, "tests/elfs/spl_token");
    p_mollusk.add_program(&ata_program_id, "tests/elfs/spl_associated_token_account");

    let (system_program, system_program_account) = program::keyed_account_for_system_program();
    let token_program = spl_token::ID;
    let token_program_account = program::create_program_account_loader_v3(&token_program);
    let ata_program_account = program::create_program_account_loader_v3(&ata_program_id);

    let mut mint_x_account = Account::new(
        p_mollusk.sysvars.rent.minimum_balance(Mint::LEN),
        Mint::LEN,
        &token_program,
    );
    Pack::pack(
        Mint { mint_authority: COption::None, supply: 100_000_000, decimals: 6, is_initialized: true, freeze_authority: COption::None },
        mint_x_account.data_as_mut_slice(),
    ).unwrap();

    let mut mint_y_account = Account::new(
        p_mollusk.sysvars.rent.minimum_balance(Mint::LEN),
        Mint::LEN,
        &token_program,
    );
    Pack::pack(
        Mint { mint_authority: COption::None, supply: 100_000_000, decimals: 6, is_initialized: true, freeze_authority: COption::None },
        mint_y_account.data_as_mut_slice(),
    ).unwrap();

    let (p_config_pda, p_config_bump) =
        Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &p_program_id);
    let (p_lp_pda, p_lp_bump) =
        Pubkey::find_program_address(&[b"lp", p_config_pda.as_ref()], &p_program_id);

    let p_vault_x = ata(p_config_pda, token_program, mint_x, ata_program_id);
    let p_vault_y = ata(p_config_pda, token_program, mint_y, ata_program_id);

    // Pinocchio: initialize
    let p_admin = Pubkey::new_unique();
    let p_admin_account = Account::new(100_000_000_000, 0, &system_program);
    let p_init_ix = Instruction {
        program_id: p_program_id,
        accounts: vec![
            AccountMeta::new(p_admin, true),
            AccountMeta::new(mint_x, false),
            AccountMeta::new(mint_y, false),
            AccountMeta::new(p_config_pda, false),
            AccountMeta::new(p_lp_pda, false),
            AccountMeta::new(p_vault_x, false),
            AccountMeta::new(p_vault_y, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
        ],
        data: {
            let mut d = vec![P_INIT];
            d.extend_from_slice(&seed.to_le_bytes());
            d.extend_from_slice(&fee.to_le_bytes());
            d.extend_from_slice(&[0u8; 32]);
            d
        },
    };
    let p_init_accounts = vec![
        (p_admin, p_admin_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (p_config_pda, Account::new(0, 0, &system_program)),
        (p_lp_pda, Account::new(0, 0, &system_program)),
        (p_vault_x, Account::new(0, 0, &system_program)),
        (p_vault_y, Account::new(0, 0, &system_program)),
        (system_program, system_program_account.clone()),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
    ];

    // Pinocchio: shared pool state for deposit/swap/withdraw
    let mut p_config_data = vec![0u8; P_CONFIG_LEN];
    p_config_data[0..8].copy_from_slice(&seed.to_le_bytes());
    p_config_data[40..72].copy_from_slice(&mint_x.to_bytes());
    p_config_data[72..104].copy_from_slice(&mint_y.to_bytes());
    p_config_data[104..106].copy_from_slice(&fee.to_le_bytes());
    p_config_data[107] = p_config_bump;
    p_config_data[108] = p_lp_bump;
    let mut p_config_account = Account::new(
        p_mollusk.sysvars.rent.minimum_balance(P_CONFIG_LEN), P_CONFIG_LEN, &p_program_id,
    );
    p_config_account.data_as_mut_slice().copy_from_slice(&p_config_data);

    let mut p_lp_mint_0 = Account::new(p_mollusk.sysvars.rent.minimum_balance(Mint::LEN), Mint::LEN, &token_program);
    Pack::pack(Mint { mint_authority: COption::Some(p_config_pda), supply: 0, decimals: 6, is_initialized: true, freeze_authority: COption::None }, p_lp_mint_0.data_as_mut_slice()).unwrap();

    let mut p_lp_mint_100k = Account::new(p_mollusk.sysvars.rent.minimum_balance(Mint::LEN), Mint::LEN, &token_program);
    Pack::pack(Mint { mint_authority: COption::Some(p_config_pda), supply: 100_000, decimals: 6, is_initialized: true, freeze_authority: COption::None }, p_lp_mint_100k.data_as_mut_slice()).unwrap();

    let p_vault_x_account = make_token_account(&p_mollusk, mint_x, p_config_pda, 100_000, token_program);
    let p_vault_y_account = make_token_account(&p_mollusk, mint_y, p_config_pda, 100_000, token_program);

    let p_user = Pubkey::new_unique();
    let p_user_account = Account::new(10_000_000_000, 0, &system_program);
    let p_user_ata_x = ata(p_user, token_program, mint_x, ata_program_id);
    let p_user_ata_y = ata(p_user, token_program, mint_y, ata_program_id);
    let p_user_ata_lp = ata(p_user, token_program, p_lp_pda, ata_program_id);

    // Pinocchio: deposit
    let p_deposit_ix = Instruction {
        program_id: p_program_id,
        accounts: vec![
            AccountMeta::new(p_user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(p_config_pda, false),
            AccountMeta::new(p_lp_pda, false),
            AccountMeta::new(p_vault_x, false),
            AccountMeta::new(p_vault_y, false),
            AccountMeta::new(p_user_ata_x, false),
            AccountMeta::new(p_user_ata_y, false),
            AccountMeta::new(p_user_ata_lp, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
        ],
        data: { let mut d = vec![P_DEPOSIT]; d.extend_from_slice(&100_000u64.to_le_bytes()); d.extend_from_slice(&50_000u64.to_le_bytes()); d.extend_from_slice(&50_000u64.to_le_bytes()); d },
    };
    let p_vault_x_empty = make_token_account(&p_mollusk, mint_x, p_config_pda, 0, token_program);
    let p_vault_y_empty = make_token_account(&p_mollusk, mint_y, p_config_pda, 0, token_program);
    let p_deposit_accounts = vec![
        (p_user, p_user_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (p_config_pda, p_config_account.clone()),
        (p_lp_pda, p_lp_mint_0.clone()),
        (p_vault_x, p_vault_x_empty),
        (p_vault_y, p_vault_y_empty),
        (p_user_ata_x, make_token_account(&p_mollusk, mint_x, p_user, 100_000, token_program)),
        (p_user_ata_y, make_token_account(&p_mollusk, mint_y, p_user, 100_000, token_program)),
        (p_user_ata_lp, make_token_account(&p_mollusk, p_lp_pda, p_user, 0, token_program)),
        (system_program, system_program_account.clone()),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
    ];

    // Pinocchio: swap
    let p_swap_ix = Instruction {
        program_id: p_program_id,
        accounts: vec![
            AccountMeta::new(p_user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(p_config_pda, false),
            AccountMeta::new_readonly(p_lp_pda, false),
            AccountMeta::new(p_vault_x, false),
            AccountMeta::new(p_vault_y, false),
            AccountMeta::new(p_user_ata_x, false),
            AccountMeta::new(p_user_ata_y, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
        ],
        data: { let mut d = vec![P_SWAP, 1u8]; d.extend_from_slice(&10_000u64.to_le_bytes()); d.extend_from_slice(&1u64.to_le_bytes()); d },
    };
    let p_swap_accounts = vec![
        (p_user, p_user_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (p_config_pda, p_config_account.clone()),
        (p_lp_pda, p_lp_mint_100k.clone()),
        (p_vault_x, p_vault_x_account.clone()),
        (p_vault_y, p_vault_y_account.clone()),
        (p_user_ata_x, make_token_account(&p_mollusk, mint_x, p_user, 100_000, token_program)),
        (p_user_ata_y, make_token_account(&p_mollusk, mint_y, p_user, 0, token_program)),
        (system_program, system_program_account.clone()),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
    ];

    // Pinocchio: withdraw
    let p_withdraw_ix = Instruction {
        program_id: p_program_id,
        accounts: vec![
            AccountMeta::new(p_user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(p_config_pda, false),
            AccountMeta::new(p_lp_pda, false),
            AccountMeta::new(p_vault_x, false),
            AccountMeta::new(p_vault_y, false),
            AccountMeta::new(p_user_ata_x, false),
            AccountMeta::new(p_user_ata_y, false),
            AccountMeta::new(p_user_ata_lp, false),
            AccountMeta::new_readonly(system_program, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
        ],
        data: { let mut d = vec![P_WITHDRAW]; d.extend_from_slice(&10_000u64.to_le_bytes()); d.extend_from_slice(&0u64.to_le_bytes()); d.extend_from_slice(&0u64.to_le_bytes()); d },
    };
    let p_withdraw_accounts = vec![
        (p_user, p_user_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (p_config_pda, p_config_account.clone()),
        (p_lp_pda, p_lp_mint_100k.clone()),
        (p_vault_x, p_vault_x_account.clone()),
        (p_vault_y, p_vault_y_account.clone()),
        (p_user_ata_x, make_token_account(&p_mollusk, mint_x, p_user, 0, token_program)),
        (p_user_ata_y, make_token_account(&p_mollusk, mint_y, p_user, 0, token_program)),
        (p_user_ata_lp, make_token_account(&p_mollusk, p_lp_pda, p_user, 50_000, token_program)),
        (system_program, system_program_account.clone()),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
    ];

    // Anchor
    let a_program_id: Pubkey = ANCHOR_AMM_ID.parse().unwrap();
    let mut a_mollusk = Mollusk::new(&a_program_id, "tests/elfs/amm_anchor");
    a_mollusk.add_program(&spl_token::ID, "tests/elfs/spl_token");
    a_mollusk.add_program(&ata_program_id, "tests/elfs/spl_associated_token_account");

    let (a_config_pda, a_config_bump) =
        Pubkey::find_program_address(&[b"config", &seed.to_le_bytes()], &a_program_id);
    let (a_lp_pda, a_lp_bump) =
        Pubkey::find_program_address(&[b"lp", a_config_pda.as_ref()], &a_program_id);

    let a_vault_x = ata(a_config_pda, token_program, mint_x, ata_program_id);
    let a_vault_y = ata(a_config_pda, token_program, mint_y, ata_program_id);

    // Anchor: initialize
    let a_admin = Pubkey::new_unique();
    let a_admin_account = Account::new(100_000_000_000, 0, &system_program);
    let a_init_ix = Instruction {
        program_id: a_program_id,
        accounts: vec![
            AccountMeta::new(a_admin, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(a_config_pda, false),
            AccountMeta::new(a_lp_pda, false),
            AccountMeta::new(a_vault_x, false),
            AccountMeta::new(a_vault_y, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: {
            let mut d = A_INIT_DISC.to_vec();
            d.extend_from_slice(&seed.to_le_bytes());
            d.extend_from_slice(&fee.to_le_bytes());
            d.push(0u8); // authority = None
            d
        },
    };
    let a_init_accounts = vec![
        (a_admin, a_admin_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (a_config_pda, Account::new(0, 0, &system_program)),
        (a_lp_pda, Account::new(0, 0, &system_program)),
        (a_vault_x, Account::new(0, 0, &system_program)),
        (a_vault_y, Account::new(0, 0, &system_program)),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
        (system_program, system_program_account.clone()),
    ];

    // Anchor: shared pool state
    let mut a_config_data = vec![0u8; A_CONFIG_LEN];
    a_config_data[0..8].copy_from_slice(&ANCHOR_CONFIG_DISC);
    a_config_data[8..16].copy_from_slice(&seed.to_le_bytes());
    a_config_data[16] = 0; // authority = None
    a_config_data[17..49].copy_from_slice(&mint_x.to_bytes());
    a_config_data[49..81].copy_from_slice(&mint_y.to_bytes());
    a_config_data[81..83].copy_from_slice(&fee.to_le_bytes());
    a_config_data[83] = 0; // locked = false
    a_config_data[84] = a_config_bump;
    a_config_data[85] = a_lp_bump;
    let mut a_config_account = Account::new(
        a_mollusk.sysvars.rent.minimum_balance(A_CONFIG_LEN), A_CONFIG_LEN, &a_program_id,
    );
    a_config_account.data_as_mut_slice().copy_from_slice(&a_config_data);

    let mut a_lp_mint_0 = Account::new(a_mollusk.sysvars.rent.minimum_balance(Mint::LEN), Mint::LEN, &token_program);
    Pack::pack(Mint { mint_authority: COption::Some(a_config_pda), supply: 0, decimals: 6, is_initialized: true, freeze_authority: COption::None }, a_lp_mint_0.data_as_mut_slice()).unwrap();

    let mut a_lp_mint_100k = Account::new(a_mollusk.sysvars.rent.minimum_balance(Mint::LEN), Mint::LEN, &token_program);
    Pack::pack(Mint { mint_authority: COption::Some(a_config_pda), supply: 100_000, decimals: 6, is_initialized: true, freeze_authority: COption::None }, a_lp_mint_100k.data_as_mut_slice()).unwrap();

    let a_vault_x_account = make_token_account(&a_mollusk, mint_x, a_config_pda, 100_000, token_program);
    let a_vault_y_account = make_token_account(&a_mollusk, mint_y, a_config_pda, 100_000, token_program);

    let a_user = Pubkey::new_unique();
    let a_user_account = Account::new(10_000_000_000, 0, &system_program);
    let a_user_ata_x = ata(a_user, token_program, mint_x, ata_program_id);
    let a_user_ata_y = ata(a_user, token_program, mint_y, ata_program_id);
    let a_user_ata_lp = ata(a_user, token_program, a_lp_pda, ata_program_id);

    // Anchor: deposit
    let a_deposit_ix = Instruction {
        program_id: a_program_id,
        accounts: vec![
            AccountMeta::new(a_user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new_readonly(a_config_pda, false),
            AccountMeta::new(a_lp_pda, false),
            AccountMeta::new(a_vault_x, false),
            AccountMeta::new(a_vault_y, false),
            AccountMeta::new(a_user_ata_x, false),
            AccountMeta::new(a_user_ata_y, false),
            AccountMeta::new(a_user_ata_lp, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: { let mut d = A_DEPOSIT_DISC.to_vec(); d.extend_from_slice(&100_000u64.to_le_bytes()); d.extend_from_slice(&50_000u64.to_le_bytes()); d.extend_from_slice(&50_000u64.to_le_bytes()); d },
    };
    let a_vault_x_empty = make_token_account(&a_mollusk, mint_x, a_config_pda, 0, token_program);
    let a_vault_y_empty = make_token_account(&a_mollusk, mint_y, a_config_pda, 0, token_program);
    let a_deposit_accounts = vec![
        (a_user, a_user_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (a_config_pda, a_config_account.clone()),
        (a_lp_pda, a_lp_mint_0.clone()),
        (a_vault_x, a_vault_x_empty),
        (a_vault_y, a_vault_y_empty),
        (a_user_ata_x, make_token_account(&a_mollusk, mint_x, a_user, 100_000, token_program)),
        (a_user_ata_y, make_token_account(&a_mollusk, mint_y, a_user, 100_000, token_program)),
        (a_user_ata_lp, make_token_account(&a_mollusk, a_lp_pda, a_user, 0, token_program)),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
        (system_program, system_program_account.clone()),
    ];

    // Anchor: swap
    let a_swap_ix = Instruction {
        program_id: a_program_id,
        accounts: vec![
            AccountMeta::new(a_user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new_readonly(a_config_pda, false),
            AccountMeta::new_readonly(a_lp_pda, false),
            AccountMeta::new(a_vault_x, false),
            AccountMeta::new(a_vault_y, false),
            AccountMeta::new(a_user_ata_x, false),
            AccountMeta::new(a_user_ata_y, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: { let mut d = A_SWAP_DISC.to_vec(); d.push(1u8); d.extend_from_slice(&10_000u64.to_le_bytes()); d.extend_from_slice(&1u64.to_le_bytes()); d },
    };
    let a_swap_accounts = vec![
        (a_user, a_user_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (a_config_pda, a_config_account.clone()),
        (a_lp_pda, a_lp_mint_100k.clone()),
        (a_vault_x, a_vault_x_account.clone()),
        (a_vault_y, a_vault_y_account.clone()),
        (a_user_ata_x, make_token_account(&a_mollusk, mint_x, a_user, 100_000, token_program)),
        (a_user_ata_y, make_token_account(&a_mollusk, mint_y, a_user, 0, token_program)),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
        (system_program, system_program_account.clone()),
    ];

    // Anchor: withdraw
    let a_withdraw_ix = Instruction {
        program_id: a_program_id,
        accounts: vec![
            AccountMeta::new(a_user, true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new_readonly(a_config_pda, false),
            AccountMeta::new(a_lp_pda, false),
            AccountMeta::new(a_vault_x, false),
            AccountMeta::new(a_vault_y, false),
            AccountMeta::new(a_user_ata_x, false),
            AccountMeta::new(a_user_ata_y, false),
            AccountMeta::new(a_user_ata_lp, false),
            AccountMeta::new_readonly(token_program, false),
            AccountMeta::new_readonly(ata_program_id, false),
            AccountMeta::new_readonly(system_program, false),
        ],
        data: { let mut d = A_WITHDRAW_DISC.to_vec(); d.extend_from_slice(&10_000u64.to_le_bytes()); d.extend_from_slice(&0u64.to_le_bytes()); d.extend_from_slice(&0u64.to_le_bytes()); d },
    };
    let a_withdraw_accounts = vec![
        (a_user, a_user_account.clone()),
        (mint_x, mint_x_account.clone()),
        (mint_y, mint_y_account.clone()),
        (a_config_pda, a_config_account.clone()),
        (a_lp_pda, a_lp_mint_100k.clone()),
        (a_vault_x, a_vault_x_account.clone()),
        (a_vault_y, a_vault_y_account.clone()),
        (a_user_ata_x, make_token_account(&a_mollusk, mint_x, a_user, 0, token_program)),
        (a_user_ata_y, make_token_account(&a_mollusk, mint_y, a_user, 0, token_program)),
        (a_user_ata_lp, make_token_account(&a_mollusk, a_lp_pda, a_user, 50_000, token_program)),
        (token_program, token_program_account.clone()),
        (ata_program_id, ata_program_account.clone()),
        (system_program, system_program_account.clone()),
    ];

    let p_init_cu   = run(&p_mollusk, "pinocchio::initialize", &p_init_ix,    &p_init_accounts);
    let p_dep_cu    = run(&p_mollusk, "pinocchio::deposit",    &p_deposit_ix,  &p_deposit_accounts);
    let p_swap_cu   = run(&p_mollusk, "pinocchio::swap",       &p_swap_ix,     &p_swap_accounts);
    let p_with_cu   = run(&p_mollusk, "pinocchio::withdraw",   &p_withdraw_ix, &p_withdraw_accounts);

    let a_init_cu   = run(&a_mollusk, "anchor::initialize",    &a_init_ix,     &a_init_accounts);
    let a_dep_cu    = run(&a_mollusk, "anchor::deposit",       &a_deposit_ix,  &a_deposit_accounts);
    let a_swap_cu   = run(&a_mollusk, "anchor::swap",          &a_swap_ix,     &a_swap_accounts);
    let a_with_cu   = run(&a_mollusk, "anchor::withdraw",      &a_withdraw_ix, &a_withdraw_accounts);

    fn savings(pinocchio: u64, anchor: u64) -> String {
        let pct = (anchor as f64 - pinocchio as f64) / anchor as f64 * 100.0;
        format!("{:.1}%", pct)
    }

    let md = format!(
        "# AMM Compute Unit Comparison: Pinocchio vs Anchor\n\n\
         | Instruction | Pinocchio (CU) | Anchor (CU) | CU Savings |\n\
         |-------------|---------------:|------------:|-----------:|\n\
         | initialize  | {:>14} | {:>11} | {:>10} |\n\
         | deposit     | {:>14} | {:>11} | {:>10} |\n\
         | swap        | {:>14} | {:>11} | {:>10} |\n\
         | withdraw    | {:>14} | {:>11} | {:>10} |\n",
        p_init_cu, a_init_cu, savings(p_init_cu, a_init_cu),
        p_dep_cu,  a_dep_cu,  savings(p_dep_cu,  a_dep_cu),
        p_swap_cu, a_swap_cu, savings(p_swap_cu, a_swap_cu),
        p_with_cu, a_with_cu, savings(p_with_cu, a_with_cu),
    );

    println!("\n{md}");

    let out_dir = Path::new("target/benches");
    fs::create_dir_all(out_dir).unwrap();
    fs::write(out_dir.join("comparison.md"), &md).unwrap();

    println!("Results written to target/benches/comparison.md");
}
