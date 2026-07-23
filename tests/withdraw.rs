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
    let msg = Message::new_with_blockhash(&[create_ix, init_ix], Some(&payer.pubkey()), &blockhash);
    let tx =
        VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer, &mint]).unwrap();
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
fn withdraw_returns_liquidity() {
    // step 1: start the VM and load the AMM program
    let mut svm = LiteSVM::new();
    let program_bytes = std::fs::read("target/deploy/amm.so")
        .expect("build the program first: cargo build-sbf");
    svm.add_program(program_id(), &program_bytes).unwrap();

    // step 2: create a user keypair; user will deposit and then withdraw
    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 10_000_000_000).unwrap();

    // step 3: create two token mints (user is the mint authority)
    let mint_x = create_mint(&mut svm, &user, 6);
    let mint_y = create_mint(&mut svm, &user, 6);

    // step 4: derive pool PDAs
    let seed: u64 = 1;
    let fee: u16 = 30; // 0.30%
    let seed_bytes = seed.to_le_bytes();

    let (config_pda, _) =
        Address::derive_program_address(&[b"config", &seed_bytes], &program_id())
            .expect("config PDA");
    let (lp_mint, _) =
        Address::derive_program_address(&[b"lp", config_pda.as_ref()], &program_id())
            .expect("lp_mint PDA");
    let vault_x = get_associated_token_address(&config_pda, &mint_x);
    let vault_y = get_associated_token_address(&config_pda, &mint_y);

    // step 5: initialize the pool
    {
        let mut data = Vec::with_capacity(43);
        data.push(0u8); // Initialize::DISCRIMINATOR = 0
        data.extend_from_slice(&seed.to_le_bytes());
        data.extend_from_slice(&fee.to_le_bytes());
        data.extend_from_slice(&[0u8; 32]); // authority = None

        let ix = Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
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
        let msg = Message::new_with_blockhash(&[ix], Some(&user.pubkey()), &blockhash);
        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&user]).unwrap();
        svm.send_transaction(tx).expect("initialize failed");
    }

    // step 6: create user ATAs for X and Y, mint tokens into them
    // The withdraw instruction requires user_ata_x and user_ata_y to ALREADY EXIST
    // (AssociatedTokenAccount::check — full owner+size validation).
    // They can have zero balance; withdraw will send tokens INTO them, not take from them.
    //
    // user_ata_lp only needs an address-only check and will be created idempotently
    // by the withdraw instruction itself before the Burn CPI.
    let user_ata_x = create_ata(&mut svm, &user, user.pubkey(), mint_x);
    let user_ata_y = create_ata(&mut svm, &user, user.pubkey(), mint_y);
    let user_ata_lp = get_associated_token_address(&user.pubkey(), &lp_mint);

    let deposit_x: u64 = 1_000_000; // 1.000000 token X
    let deposit_y: u64 = 1_000_000; // 1.000000 token Y
    let lp_amount: u64 = 1_000;     // LP tokens to mint

    mint_tokens(&mut svm, &user, &user, mint_x, user_ata_x, deposit_x);
    mint_tokens(&mut svm, &user, &user, mint_y, user_ata_y, deposit_y);

    // step 7: deposit to get LP tokens
    // After deposit:
    //   user_ata_x = 0         (all X transferred to vault_x)
    //   user_ata_y = 0         (all Y transferred to vault_y)
    //   user_ata_lp = 1_000    (LP tokens minted by config PDA)
    //   vault_x = 1_000_000
    //   vault_y = 1_000_000
    //   lp_supply = 1_000
    {
        let mut data = Vec::with_capacity(25);
        data.push(1u8); // Deposit::DISCRIMINATOR = 1
        data.extend_from_slice(&lp_amount.to_le_bytes());
        data.extend_from_slice(&deposit_x.to_le_bytes()); // max_x
        data.extend_from_slice(&deposit_y.to_le_bytes()); // max_y

        let ix = Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(user.pubkey(), true),
                AccountMeta::new_readonly(mint_x, false),
                AccountMeta::new_readonly(mint_y, false),
                AccountMeta::new_readonly(config_pda, false),
                AccountMeta::new(lp_mint, false),
                AccountMeta::new(vault_x, false),
                AccountMeta::new(vault_y, false),
                AccountMeta::new(user_ata_x, false),
                AccountMeta::new(user_ata_y, false),
                AccountMeta::new(user_ata_lp, false),
                AccountMeta::new_readonly(solana_system_interface::program::ID, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new_readonly(Address::new_from_array(ATA_PROGRAM_BYTES), false),
            ],
            data,
        };
        let blockhash = svm.latest_blockhash();
        let msg = Message::new_with_blockhash(&[ix], Some(&user.pubkey()), &blockhash);
        let tx =
            VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&user]).unwrap();
        svm.send_transaction(tx).expect("deposit failed");
    }

    // Verify deposit worked before testing withdraw.
    assert_eq!(get_token_balance(&svm, &user_ata_lp), lp_amount);
    assert_eq!(get_token_balance(&svm, &vault_x), deposit_x);
    assert_eq!(get_token_balance(&svm, &vault_y), deposit_y);

    // step 8: build the withdraw instruction data
    // Layout (after the 1-byte discriminator):
    //   [0..8]  amount : u64 LE — LP tokens to burn
    //   [8..16] min_x  : u64 LE — minimum token X to receive (slippage guard)
    //   [16..24]min_y  : u64 LE — minimum token Y to receive (slippage guard)
    //
    // Pool state after deposit: vault_x = 1_000_000, vault_y = 1_000_000, lp = 1_000.
    // Withdrawing 500 LP (half the supply) from a symmetric pool gives back:
    //   x_out = vault_x * withdraw_amount / lp_supply = 1_000_000 * 500 / 1_000 = 500_000
    //   y_out = vault_y * withdraw_amount / lp_supply = 1_000_000 * 500 / 1_000 = 500_000
    let withdraw_amount: u64 = 500; // burn half the LP supply
    let min_x: u64 = 0;            // accept anything (no slippage check)
    let min_y: u64 = 0;

    let mut data = Vec::with_capacity(25);
    data.push(3u8); // Withdraw::DISCRIMINATOR = 3
    data.extend_from_slice(&withdraw_amount.to_le_bytes());
    data.extend_from_slice(&min_x.to_le_bytes());
    data.extend_from_slice(&min_y.to_le_bytes());

    // step 9: build the withdraw instruction with all 12 accounts
    // Same account count as deposit (12), but the semantics are reversed:
    // tokens flow FROM the vaults TO the user, and LP tokens are burned instead of minted.
    //
    //  #  account      writable?  why
    //  0  user           yes      funds CreateIdempotent; signs Burn authority
    //  1  mint_x         no       read decimals for TransferChecked
    //  2  mint_y         no       read decimals for TransferChecked
    //  3  config         no       read seed, bump for PDA signer; verify mints
    //  4  mint_lp        yes      Burn reduces supply — must be writable
    //  5  vault_x        yes      sends X to user (config PDA signs)
    //  6  vault_y        yes      sends Y to user (config PDA signs)
    //  7  user_ata_x     yes      receives token X
    //  8  user_ata_y     yes      receives token Y
    //  9  user_ata_lp    yes      LP tokens are burned from here
    // 10  system_program no       CreateIdempotent may allocate user_ata_lp
    // 11  token_program  no       Burn + TransferChecked CPIs
    // 12  ATA program    no       must be in outer accounts for nested CPI resolution
    let ix = Instruction {
        program_id: program_id(),
        accounts: vec![
            AccountMeta::new(user.pubkey(), true),              // 0. user
            AccountMeta::new_readonly(mint_x, false),           // 1. mint_x
            AccountMeta::new_readonly(mint_y, false),           // 2. mint_y
            AccountMeta::new_readonly(config_pda, false),       // 3. config
            AccountMeta::new(lp_mint, false),                   // 4. mint_lp (supply shrinks)
            AccountMeta::new(vault_x, false),                   // 5. vault_x (X leaves)
            AccountMeta::new(vault_y, false),                   // 6. vault_y (Y leaves)
            AccountMeta::new(user_ata_x, false),                // 7. user_ata_x (X arrives)
            AccountMeta::new(user_ata_y, false),                // 8. user_ata_y (Y arrives)
            AccountMeta::new(user_ata_lp, false),               // 9. user_ata_lp (LP burned)
            AccountMeta::new_readonly(
                solana_system_interface::program::ID,
                false,
            ),                                                  // 10. system_program
            AccountMeta::new_readonly(spl_token::ID, false),   // 11. token_program
            AccountMeta::new_readonly(
                Address::new_from_array(ATA_PROGRAM_BYTES),
                false,
            ),                                                  // 12. ATA program
        ],
        data,
    };

    // step 10: send the withdraw transaction
    // The user signs:
    //   • Burn authority is the user (they own the LP tokens).
    //   • CreateIdempotent (for user_ata_lp) uses user as the funding account.
    // The config PDA signs the vault→user TransferChecked CPIs via invoke_signed.
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(&[ix], Some(&user.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[&user]).unwrap();
    let result = svm.send_transaction(tx);
    assert!(result.is_ok(), "withdraw failed: {:?}", result.err());

    // step 11: LP tokens decreased by exactly withdraw_amount
    // The Burn CPI burned `withdraw_amount` LP tokens from user_ata_lp.
    // The remaining LP tokens stay in user_ata_lp.
    assert_eq!(
        get_token_balance(&svm, &user_ata_lp),
        lp_amount - withdraw_amount,
        "user_ata_lp should have {} LP remaining",
        lp_amount - withdraw_amount,
    );

    // step 12: user received token X and token Y from the vaults
    // For a symmetric pool (equal amounts of each token), withdrawing half the
    // LP supply returns exactly half of each vault's balance.
    let x_returned = get_token_balance(&svm, &user_ata_x);
    let y_returned = get_token_balance(&svm, &user_ata_y);

    assert!(x_returned > 0, "user should have received some token X");
    assert!(y_returned > 0, "user should have received some token Y");

    // step 13: vault balances decreased proportionally
    // The config PDA transferred x_returned and y_returned out of the vaults.
    let vault_x_after = get_token_balance(&svm, &vault_x);
    let vault_y_after = get_token_balance(&svm, &vault_y);

    assert_eq!(
        vault_x_after + x_returned,
        deposit_x,
        "vault_x + returned_x should equal the original deposit (tokens conserved)"
    );
    assert_eq!(
        vault_y_after + y_returned,
        deposit_y,
        "vault_y + returned_y should equal the original deposit (tokens conserved)"
    );
    assert!(
        vault_x_after < deposit_x,
        "vault_x should have shrunk after withdraw"
    );
    assert!(
        vault_y_after < deposit_y,
        "vault_y should have shrunk after withdraw"
    );
}
