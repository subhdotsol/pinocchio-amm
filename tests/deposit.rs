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

// Compute the Associated Token Account (ATA) address for (wallet, mint) without calling
// any on-chain program — pure Rust PDA derivation. Same formula the ATA program uses:
//   seeds = [wallet_bytes, token_program_bytes, mint_bytes]  under the ATA program ID
fn get_associated_token_address(wallet: &Address, mint: &Address) -> Address {
    let ata_program = Address::new_from_array(ATA_PROGRAM_BYTES);
    let (ata, _) = Address::derive_program_address(
        &[wallet.as_ref(), &TOKEN_PROGRAM_BYTES, mint.as_ref()],
        &ata_program,
    )
    .expect("failed to derive ATA");
    ata
}

// Create a new SPL token mint owned by `payer` as the mint authority.
// Returns the mint's public address.
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
    svm.send_transaction(tx).expect("create_mint failed");

    mint.pubkey()
}

// Create an Associated Token Account for `wallet` holding `mint` tokens.
// `payer` funds the rent-exempt balance and the ATA address is returned.
// litesvm includes the ATA program by default, so this transaction executes on-chain normally.
fn create_ata(svm: &mut LiteSVM, payer: &Keypair, wallet: Address, mint: Address) -> Address {
    let ata = get_associated_token_address(&wallet, &mint);
    let ata_program = Address::new_from_array(ATA_PROGRAM_BYTES);
    let token_program = Address::new_from_array(TOKEN_PROGRAM_BYTES);

    // ATA program "Create" instruction — no instruction data needed.
    // Account order: payer(signer), ata(writable), wallet, mint, system_program, token_program
    let ix = Instruction {
        program_id: ata_program,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(wallet, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(solana_system_interface::program::ID, false),
            AccountMeta::new_readonly(token_program, false),
        ],
        data: vec![],
    };

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
    svm.send_transaction(tx).expect("create_ata failed");

    ata
}

// Mint `amount` tokens from `mint` into `dest` token account.
// `mint_authority` must hold the private key corresponding to the mint's authority.
fn mint_tokens(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint_authority: &Keypair,
    mint: Address,
    dest: Address,
    amount: u64,
) {
    let ix = spl_token::instruction::mint_to(
        &spl_token::ID,
        &mint,
        &dest,
        &mint_authority.pubkey(),
        &[],
        amount,
    )
    .unwrap();

    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &blockhash);
    // Avoid passing the same keypair twice when payer == mint_authority.
    // VersionedTransaction::try_new returns TooManySigners if signers > required slots.
    let signers: Vec<&Keypair> = if payer.pubkey() == mint_authority.pubkey() {
        vec![payer]
    } else {
        vec![payer, mint_authority]
    };
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &signers).unwrap();
    svm.send_transaction(tx).expect("mint_tokens failed");
}

// Read the token balance of any SPL token account by unpacking its raw account data.
fn get_token_balance(svm: &LiteSVM, token_account: &Address) -> u64 {
    let raw = svm
        .get_account(token_account)
        .expect("token account not found");
    spl_token::state::Account::unpack(&raw.data)
        .expect("failed to unpack token account")
        .amount
}

#[test]
fn deposit_adds_liquidity() {
    // step 1: start the test VM and load the compiled AMM program
    // LiteSVM runs Solana BPF programs natively — no validator needed.
    // SPL Token and ATA programs are bundled in LiteSVM::new() by default.
    let mut svm = LiteSVM::new();
    let program_bytes = std::fs::read("target/deploy/amm.so")
        .expect("build the program first: cargo build-sbf");
    svm.add_program(program_id(), &program_bytes).unwrap();

    // step 2: create keypairs and give them SOL
    // admin → creates mints and funds ATA rent; admin is also the mint authority
    // user  → will call the deposit instruction
    let admin = Keypair::new();
    let user = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    // step 3: create two SPL token mints
    // 6 decimals each (like USDC). admin is the mint authority, meaning only
    // admin can call MintTo — the pool cannot self-mint these tokens.
    let mint_x = create_mint(&mut svm, &admin, 6);
    let mint_y = create_mint(&mut svm, &admin, 6);

    // step 4: derive all the PDAs the pool will use
    // config PDA : seeds = ["config", seed_as_le_bytes]  → stores pool state
    // lp_mint PDA: seeds = ["lp", config_address]        → config PDA is the mint authority
    // vault_x/y  : ATAs owned by the config PDA           → hold the pooled tokens
    let seed: u64 = 1;
    let fee: u16 = 30; // 0.30%
    let seed_bytes = seed.to_le_bytes();

    let (config_pda, _) =
        Address::derive_program_address(&[b"config", &seed_bytes], &program_id())
            .expect("config PDA derivation failed");

    let (lp_mint, _) =
        Address::derive_program_address(&[b"lp", config_pda.as_ref()], &program_id())
            .expect("lp_mint PDA derivation failed");

    let vault_x = get_associated_token_address(&config_pda, &mint_x);
    let vault_y = get_associated_token_address(&config_pda, &mint_y);

    // step 5: call Initialize to create the pool on-chain
    // This transaction creates config, lp_mint, vault_x, vault_y.
    // Without it, deposit would fail immediately because config doesn't exist yet.
    //
    // Instruction data: [discriminator(0)] + seed(u64 LE) + fee(u16 LE) + authority([u8;32])
    // authority = all zeros → no admin can lock/unlock this pool
    {
        let mut data = Vec::with_capacity(43);
        data.push(0u8); // Initialize::DISCRIMINATOR = 0
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
        let msg = Message::new_with_blockhash(&[ix], Some(&admin.pubkey()), &blockhash);
        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&admin]).unwrap();
        svm.send_transaction(tx).expect("initialize failed");
    }

    // step 6: create user's ATAs for token X and token Y
    // The deposit instruction checks user_ata_x and user_ata_y with
    // AssociatedTokenAccount::check — a full check (owner + data length).
    // They must exist BEFORE the deposit transaction runs.
    //
    // user_ata_lp does NOT need to exist beforehand — the deposit instruction
    // creates it on-the-fly via CreateIdempotent before calling MintTo.
    let user_ata_x = create_ata(&mut svm, &admin, user.pubkey(), mint_x);
    let user_ata_y = create_ata(&mut svm, &admin, user.pubkey(), mint_y);

    // Derive user_ata_lp so we can pass its address as an account.
    // The deposit instruction will create the actual on-chain account.
    let user_ata_lp = get_associated_token_address(&user.pubkey(), &lp_mint);

    // step 7: mint tokens into the user's ATAs
    // admin is the mint authority, so only admin can call MintTo on these mints.
    // The user needs tokens to deposit; we give them exactly 1.0 of each (6 decimals).
    let deposit_x: u64 = 1_000_000; // 1.000000 token X
    let deposit_y: u64 = 1_000_000; // 1.000000 token Y
    let lp_amount: u64 = 1_000;     // LP tokens to receive in return

    mint_tokens(&mut svm, &admin, &admin, mint_x, user_ata_x, deposit_x);
    mint_tokens(&mut svm, &admin, &admin, mint_y, user_ata_y, deposit_y);

    // step 8: build the deposit instruction data
    // The entrypoint strips the 1-byte discriminator and passes the rest to Deposit::run.
    // Layout of the 24 bytes after the discriminator:
    //   [0..8]  amount : u64 LE  — how many LP tokens to mint to the user
    //   [8..16] max_x  : u64 LE  — max token X we allow the pool to take (slippage guard)
    //   [16..24]max_y  : u64 LE  — max token Y we allow the pool to take (slippage guard)
    //
    // First deposit shortcut: if lp_supply == 0 && vault_x == 0 && vault_y == 0,
    // the program skips the constant-product formula and takes exactly (max_x, max_y).
    // The slippage check (x <= max_x, y <= max_y) trivially passes in this case.
    let mut data = Vec::with_capacity(25);
    data.push(1u8); // Deposit::DISCRIMINATOR = 1
    data.extend_from_slice(&lp_amount.to_le_bytes());  // amount (LP tokens to mint)
    data.extend_from_slice(&deposit_x.to_le_bytes());  // max_x
    data.extend_from_slice(&deposit_y.to_le_bytes());  // max_y

    // step 9: build the deposit instruction with all 12 accounts
    // Order must exactly match DepositAccounts::try_from in src/instructions/deposit.rs.
    //
    //  #  account      writable?  signer?  why
    //  0  user           yes       yes     authorizes TransferChecked + funds CreateIdempotent
    //  1  mint_x         no        no      read decimals for TransferChecked
    //  2  mint_y         no        no      read decimals for TransferChecked
    //  3  config         no        no      read seed/bump for PDA signer; verify pool matches
    //  4  mint_lp        yes       no      MintTo increases supply field
    //  5  vault_x        yes       no      receives token X from user
    //  6  vault_y        yes       no      receives token Y from user
    //  7  user_ata_x     yes       no      token X leaves from here
    //  8  user_ata_y     yes       no      token Y leaves from here
    //  9  user_ata_lp    yes       no      LP tokens arrive here (created if needed)
    // 10  system_program no        no      CreateIdempotent needs it to allocate
    // 11  token_program  no        no      TransferChecked, MintTo are SPL Token CPIs
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),              // 0. user
            AccountMeta::new_readonly(mint_x, false),           // 1. mint_x
            AccountMeta::new_readonly(mint_y, false),           // 2. mint_y
            AccountMeta::new_readonly(config_pda, false),       // 3. config
            AccountMeta::new(lp_mint, false),                   // 4. mint_lp
            AccountMeta::new(vault_x, false),                   // 5. vault_x
            AccountMeta::new(vault_y, false),                   // 6. vault_y
            AccountMeta::new(user_ata_x, false),                // 7. user_ata_x
            AccountMeta::new(user_ata_y, false),                // 8. user_ata_y
            AccountMeta::new(user_ata_lp, false),               // 9. user_ata_lp
            AccountMeta::new_readonly(
                solana_system_interface::program::ID,
                false,
            ),                                                  // 10. system_program
            AccountMeta::new_readonly(spl_token::ID, false),   // 11. token_program
            // 12. ATA program — must be present in the outer transaction for the
            // CreateIdempotent CPI (deposit → ATA program) to resolve the callee.
            AccountMeta::new_readonly(
                Address::new_from_array(ATA_PROGRAM_BYTES),
                false,
            ),
        ],
        data,
    };

    // step 10: send the deposit transaction
    // Only the user keypair signs:
    //   • TransferChecked needs user's signature (they own user_ata_x and user_ata_y).
    //   • CreateIdempotent uses user as the funding account.
    // The config PDA signs MintTo via invoke_signed — no keypair for PDAs.
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&user.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&user]).unwrap();
    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "deposit failed: {:?}", result.err());

    // step 11: vault balances should equal what the user deposited
    // On a first deposit the program takes exactly max_x and max_y.
    assert_eq!(
        get_token_balance(&svm, &vault_x),
        deposit_x,
        "vault_x should hold all deposited token X"
    );
    assert_eq!(
        get_token_balance(&svm, &vault_y),
        deposit_y,
        "vault_y should hold all deposited token Y"
    );

    // step 12: user should have received LP tokens
    // The config PDA (mint authority of lp_mint) minted exactly `lp_amount` tokens
    // into user_ata_lp via invoke_signed. These LP tokens represent the user's share.
    assert_eq!(
        get_token_balance(&svm, &user_ata_lp),
        lp_amount,
        "user_ata_lp should hold the requested LP tokens"
    );

    // step 13: user's source ATAs should be empty
    // All tokens were transferred to the vaults so user_ata_x and user_ata_y
    // should now have a zero balance.
    assert_eq!(
        get_token_balance(&svm, &user_ata_x),
        0,
        "user_ata_x should be empty after deposit"
    );
    assert_eq!(
        get_token_balance(&svm, &user_ata_y),
        0,
        "user_ata_y should be empty after deposit"
    );
}
