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
    140, 151, 37, 143, 78, 36, 137, 241, 187, 61, 16, 41, 20, 142, 13, 131, 11, 90, 19, 153, 218,
    255, 16, 132, 4, 142, 123, 216, 219, 233, 248, 89,
];

// TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
const TOKEN_PROGRAM_BYTES: [u8; 32] = [
    6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28, 180, 133, 237,
    95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
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
    let msg = Message::new_with_blockhash(&[create_ix, init_ix], Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer, &mint]).unwrap();
    svm.send_transaction(tx).expect("create_mint failed");

    mint.pubkey()
}

fn create_ata(svm: &mut LiteSVM, payer: &Keypair, wallet: Address, mint: Address) -> Address {
    let ata = get_associated_token_address(&wallet, &mint);
    let ata_program = Address::new_from_array(ATA_PROGRAM_BYTES);
    let token_program = Address::new_from_array(TOKEN_PROGRAM_BYTES);

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
    let signers: Vec<&Keypair> = if payer.pubkey() == mint_authority.pubkey() {
        vec![payer]
    } else {
        vec![payer, mint_authority]
    };
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &signers).unwrap();
    svm.send_transaction(tx).expect("mint_tokens failed");
}

fn get_token_balance(svm: &LiteSVM, token_account: &Address) -> u64 {
    let raw = svm
        .get_account(token_account)
        .expect("token account not found");
    spl_token::state::Account::unpack(&raw.data)
        .expect("failed to unpack token account")
        .amount
}

#[test]
fn swap_x_for_y() {
    // step 1: start the VM and load the AMM program
    let mut svm = LiteSVM::new();
    let program_bytes =
        std::fs::read("target/deploy/amm.so").expect("build the program first: cargo build-sbf");
    svm.add_program(program_id(), &program_bytes).unwrap();

    // step 2: create a liquidity provider (admin) and a swapper (user)
    // admin will seed the pool; user will perform the swap.
    let admin = Keypair::new();
    let user = Keypair::new();
    svm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
    svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    // step 3: create two token mints
    let mint_x = create_mint(&mut svm, &admin, 6);
    let mint_y = create_mint(&mut svm, &admin, 6);

    // step 4: derive pool PDAs
    // config PDA : holds pool state (fee, mints, locked flag, bumps)
    // lp_mint PDA: the LP token mint (supply tracks total liquidity in the pool)
    // vault_x/y  : ATAs owned by config PDA — hold the pooled tokens
    let seed: u64 = 1;
    let fee: u16 = 30; // 0.30%
    let seed_bytes = seed.to_le_bytes();

    let (config_pda, _) = Address::derive_program_address(&[b"config", &seed_bytes], &program_id())
        .expect("config PDA");
    let (lp_mint, _) =
        Address::derive_program_address(&[b"lp", config_pda.as_ref()], &program_id())
            .expect("lp_mint PDA");
    let vault_x = get_associated_token_address(&config_pda, &mint_x);
    let vault_y = get_associated_token_address(&config_pda, &mint_y);

    // step 5: initialize the pool
    // Creates config, lp_mint, vault_x, vault_y on-chain.
    // Without this the swap instruction would fail when loading the config account.
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
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&admin]).unwrap();
        svm.send_transaction(tx).expect("initialize failed");
    }

    // step 6: admin provides initial liquidity (seeds the pool)
    // A swap needs a non-empty pool to draw from. Admin deposits 1_000_000 of each
    // token. This is the "first deposit" — the pool takes exactly max_x and max_y.
    //
    // After this step: vault_x = 1_000_000, vault_y = 1_000_000, lp_supply = 1_000.
    let admin_ata_x = create_ata(&mut svm, &admin, admin.pubkey(), mint_x);
    let admin_ata_y = create_ata(&mut svm, &admin, admin.pubkey(), mint_y);
    let admin_ata_lp = get_associated_token_address(&admin.pubkey(), &lp_mint);

    let seed_x: u64 = 1_000_000;
    let seed_y: u64 = 1_000_000;
    let seed_lp: u64 = 1_000;

    mint_tokens(&mut svm, &admin, &admin, mint_x, admin_ata_x, seed_x);
    mint_tokens(&mut svm, &admin, &admin, mint_y, admin_ata_y, seed_y);

    {
        let mut data = Vec::with_capacity(25);
        data.push(1u8); // Deposit::DISCRIMINATOR = 1
        data.extend_from_slice(&seed_lp.to_le_bytes()); // lp amount
        data.extend_from_slice(&seed_x.to_le_bytes()); // max_x
        data.extend_from_slice(&seed_y.to_le_bytes()); // max_y

        let ix = Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new_readonly(mint_x, false),
                AccountMeta::new_readonly(mint_y, false),
                AccountMeta::new_readonly(config_pda, false),
                AccountMeta::new(lp_mint, false),
                AccountMeta::new(vault_x, false),
                AccountMeta::new(vault_y, false),
                AccountMeta::new(admin_ata_x, false),
                AccountMeta::new(admin_ata_y, false),
                AccountMeta::new(admin_ata_lp, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(Address::new_from_array(ATA_PROGRAM_BYTES), false),
            ],
            data,
        };
        let blockhash = svm.latest_blockhash();
        let msg = Message::new_with_blockhash(&[ix], Some(&admin.pubkey()), &blockhash);
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&admin]).unwrap();
        svm.send_transaction(tx).expect("seed deposit failed");
    }

    // step 7: give the user some token X to swap
    // user_ata_x must exist and hold tokens so TransferChecked can pull from it.
    // user_ata_y doesn't need to exist — the swap instruction creates it on-the-fly
    // via CreateIdempotent before it transfers the output tokens there.
    let user_ata_x = create_ata(&mut svm, &admin, user.pubkey(), mint_x);
    let user_ata_y = get_associated_token_address(&user.pubkey(), &mint_y);

    let swap_amount_in: u64 = 100_000; // 0.10 token X (6 decimals)
    let min_amount_out: u64 = 1; // accept any non-zero amount of Y

    mint_tokens(&mut svm, &admin, &admin, mint_x, user_ata_x, swap_amount_in);

    // step 8: build the swap instruction data
    // Layout (after the 1-byte discriminator the entrypoint strips):
    //   [0]    is_x       : u8   — 1 = swapping X for Y, 0 = swapping Y for X
    //   [1..9] amount_in  : u64 LE — tokens the user is sending into the pool
    //   [9..17]min_amount_out : u64 LE — minimum tokens the user accepts from the pool
    //
    // The swap instruction uses the constant-product formula with the pool fee:
    //   effective_in = amount_in * (10000 - fee) / 10000
    //   pool constant k = vault_x * vault_y
    //   amount_out = vault_y - k / (vault_x + effective_in)
    // The fee stays in the pool, growing its reserves over time.
    let mut data = Vec::with_capacity(17);
    data.push(2u8); // Swap::DISCRIMINATOR = 2
    data.push(1u8); // is_x = true → swap X for Y
    data.extend_from_slice(&swap_amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());

    // step 9: build the swap instruction with all 11 accounts
    // Swap has one fewer account than deposit/withdraw — no user_ata_lp is needed.
    // Both user_ata_x and user_ata_y use check_address_only (init_if_needed);
    // the swap instruction creates them via CreateIdempotent before the transfers.
    //
    //  #  account      writable?  why
    //  0  user           yes      pays for CreateIdempotent ATAs; authorizes TransferChecked
    //  1  mint_x         no       read decimals for TransferChecked
    //  2  mint_y         no       read decimals for TransferChecked
    //  3  config         no       read fee, seed, bump for PDA signer
    //  4  mint_lp        no       read LP supply (determines if pool is initialized)
    //  5  vault_x        yes      receives token X (when is_x=true)
    //  6  vault_y        yes      sends token Y  (when is_x=true)
    //  7  user_ata_x     yes      sends token X  (when is_x=true)
    //  8  user_ata_y     yes      receives token Y (when is_x=true)
    //  9  system_program no       CreateIdempotent allocates via system_program
    // 10  token_program  no       TransferChecked CPIs
    // 11  ATA program    no       must be in outer accounts for nested CPI resolution
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),        // 0. user
            AccountMeta::new_readonly(mint_x, false),     // 1. mint_x
            AccountMeta::new_readonly(mint_y, false),     // 2. mint_y
            AccountMeta::new_readonly(config_pda, false), // 3. config
            AccountMeta::new_readonly(lp_mint, false),    // 4. mint_lp (only read for supply)
            AccountMeta::new(vault_x, false),             // 5. vault_x
            AccountMeta::new(vault_y, false),             // 6. vault_y
            AccountMeta::new(user_ata_x, false),          // 7. user_ata_x (X leaves)
            AccountMeta::new(user_ata_y, false),          // 8. user_ata_y (Y arrives)
            AccountMeta::new_readonly(solana_system_interface::program::ID, false), // 9. system_program
            AccountMeta::new_readonly(spl_token::ID, false), // 10. token_program
            AccountMeta::new_readonly(Address::new_from_array(ATA_PROGRAM_BYTES), false), // 11. ATA program
        ],
        data,
    };

    // step 10: send the swap transaction
    // Only the user signs:
    //   • TransferChecked from user_ata_x needs user's signature.
    //   • CreateIdempotent (both ATAs) uses user as the funding account.
    // The config PDA signs the vault→user_ata_y TransferChecked via invoke_signed.
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&user.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&user]).unwrap();
    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "swap failed: {:?}", result.err());

    // step 11: user's X balance should be zero — all sent into the pool
    // The swap deposits the full amount_in from user_ata_x into vault_x.
    assert_eq!(
        get_token_balance(&svm, &user_ata_x),
        0,
        "user should have spent all token X"
    );

    // step 12: user's Y balance should be positive
    // The pool transferred (amount_out) Y from vault_y to user_ata_y.
    // The exact amount depends on the constant-product formula and the 0.30% fee.
    let user_y_received = get_token_balance(&svm, &user_ata_y);
    assert!(
        user_y_received >= min_amount_out,
        "user should have received at least {} token Y, got {}",
        min_amount_out,
        user_y_received,
    );
    assert!(
        user_y_received < swap_amount_in, // Y received < X spent (fee + price impact)
        "user received more Y than X sent in — unexpected"
    );

    // step 13: vault_x grew, vault_y shrank
    // Vault X absorbed the full amount_in. Vault Y released amount_out.
    // Pool reserves moved but the constant-product invariant (k = x*y) holds
    // approximately after the fee is applied.
    let vault_x_after = get_token_balance(&svm, &vault_x);
    let vault_y_after = get_token_balance(&svm, &vault_y);

    assert!(
        vault_x_after > seed_x,
        "vault_x should have grown after receiving X from user"
    );
    assert!(
        vault_y_after < seed_y,
        "vault_y should have shrunk after sending Y to user"
    );

    // Sanity-check: tokens are conserved.
    // X in: user sent swap_amount_in, vault_x should have grown by exactly that.
    assert_eq!(
        vault_x_after,
        seed_x + swap_amount_in,
        "vault_x should equal seed + amount_in (all X goes to vault)"
    );
    // Y out: vault_y + what user received = original vault_y
    assert_eq!(
        vault_y_after + user_y_received,
        seed_y,
        "vault_y + user_y_received should equal original vault_y (tokens conserved)"
    );
}
