use litesvm::LiteSVM;
use solana_address::Address;
use solana_instruction::{Instruction, account_meta::AccountMeta};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use spl_token::solana_program::program_pack::Pack;

use amm::ID;

// ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
const ATA_PROGRAM_BYTES: [u8; 32] = [
    140, 151, 37, 143, 78, 36, 137, 241, 187, 61, 16, 41, 20, 142, 13, 131,
    11, 90, 19, 153, 218, 255, 16, 132, 4, 142, 123, 216, 219, 233, 248, 89,
];

// TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
const TOKEN_PROGRAM_BYTES: [u8; 32] = [
    6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172,
    28, 180, 133, 237, 95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
];

fn program_id() -> Address {
    ID
}

fn get_associated_token_address(wallet: &Address, mint: &Address) -> Address {
    let ata_program = Address::new_from_array(ATA_PROGRAM_BYTES);
    let (ata, _) = Address::derive_program_address(
        &[wallet.as_ref(), &TOKEN_PROGRAM_BYTES, mint.as_ref()],
        &ata_program,
    )
    .expect("failed to derive ATA");
    ata
}

fn create_mint(svm: &mut LiteSVM, payer: &Keypair, decimals: u8) -> Address {
    let mint = Keypair::new();
    let rent = svm.minimum_balance_for_rent_exemption(spl_token::state::Mint::LEN);

    let create_ix = solana_system_interface::instruction::create_account(
        &payer.pubkey(),
        &mint.pubkey(),
        rent,
        spl_token::state::Mint::LEN as u64,
        &spl_token::ID,
    );
    let init_ix = spl_token::instruction::initialize_mint2(
        &spl_token::ID,
        &mint.pubkey(),
        &payer.pubkey(),
        None,
        decimals,
    )
    .unwrap();

    let blockhash = svm.latest_blockhash();
    let message =
        Message::new_with_blockhash(&[create_ix, init_ix], Some(&payer.pubkey()), &blockhash);
    let tx =
        VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[payer, &mint]).unwrap();
    svm.send_transaction(tx).unwrap();

    mint.pubkey()
}

#[test]
fn initialize_creates_pool() {
    let mut svm = LiteSVM::new();

    let program_bytes = std::fs::read("target/deploy/amm.so")
        .expect("build the program first: cargo build-sbf");
    svm.add_program(program_id(), &program_bytes).unwrap();

    let admin = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();

    let mint_x = create_mint(&mut svm, &admin, 6);
    let mint_y = create_mint(&mut svm, &admin, 6);

    let seed: u64 = 1;
    let fee: u16 = 30; // 0.30%

    let seed_bytes = seed.to_le_bytes();
    let (config_pda, _) = Address::derive_program_address(
        &[b"config", &seed_bytes],
        &program_id(),
    )
    .expect("failed to derive config PDA");

    let (lp_mint, _) = Address::derive_program_address(
        &[b"lp", config_pda.as_ref()],
        &program_id(),
    )
    .expect("failed to derive lp PDA");

    let vault_x = get_associated_token_address(&config_pda, &mint_x);
    let vault_y = get_associated_token_address(&config_pda, &mint_y);

    let mut data = Vec::with_capacity(43);
    data.push(0u8); // Initialize::DISCRIMINATOR
    data.extend_from_slice(&seed.to_le_bytes());
    data.extend_from_slice(&fee.to_le_bytes());
    data.extend_from_slice(&[0u8; 32]); // authority = None

    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new_readonly(mint_x, false),
            AccountMeta::new_readonly(mint_y, false),
            AccountMeta::new(config_pda, false),
            AccountMeta::new(lp_mint, false),
            AccountMeta::new(vault_x, false),
            AccountMeta::new(vault_y, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(Address::new_from_array(ATA_PROGRAM_BYTES), false),
        ],
        data,
    };

    let blockhash = svm.latest_blockhash();
    let message = Message::new_with_blockhash(&[ix], Some(&admin.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(message), &[&admin]).unwrap();

    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "initialize failed: {:?}", result.err());

    let config_account = svm
        .get_account(&config_pda)
        .expect("config account should exist");
    assert_eq!(config_account.owner, program_id());
    assert_eq!(
        config_account.data.len(),
        amm::state::Config::LEN
    );
}
