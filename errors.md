# AMM Build — Errors & Fixes Log

A running record of every compiler error hit while building this Solana AMM with pinocchio 0.11.2, and exactly how each was resolved.

---

## 1. `core::cell::Ref` vs `solana_account_view::Ref`

**File:** `src/state/pool.rs`

**Error:**
```
error[E0412]: cannot find type `Ref` in this scope
```

**Cause:** pinocchio 0.11.2 replaced `core::cell::Ref/RefMut` with its own wrappers from `solana_account_view`.

**Fix:**
```rust
// wrong
use core::cell::{Ref, RefMut};

// correct
use pinocchio::account::{Ref, RefMut};
```

---

## 2. `Address` is not `[u8; 32]`

**Files:** `src/state/pool.rs`, `src/instructions/initialize.rs`

**Error:**
```
error[E0308]: mismatched types
  expected `Address`, found `[u8; 32]`
```

**Cause:** pinocchio 0.11.2 uses `solana_address::Address` (a newtype), not a raw `[u8; 32]` alias.

**Fix:** Use `Address::new_from_array(...)` everywhere a `[u8; 32]` literal was used, including zero-address checks:
```rust
// wrong
if self.authority == [0u8; 32] { ... }

// correct
if self.authority == Address::new_from_array([0u8; 32]) { ... }
```

---

## 3. `accounts` parameter mutability — `&[AccountView]` vs `&mut [AccountView]`

**Files:** `src/entrypoint.rs`, all instruction handlers

**Error:**
```
error[E0596]: cannot borrow `*accounts` as mutable, as it is behind a `&` reference
```

**Cause:** `program_entrypoint!` passes `&mut [AccountView]`. All dispatch and handler signatures must accept `&mut`.

**Fix:** Change every `accounts: &[AccountView]` to `accounts: &mut [AccountView]` in `process_instruction`, `TryFrom` impls, and instruction `process` methods.

---

## 4. `Config::load_mut` needs `&mut AccountView`

**File:** `src/instructions/initialize.rs`, `src/state/pool.rs`

**Error:**
```
error[E0596]: cannot borrow `*self.accounts.config` as mutable
```

**Cause:** `load_mut` borrows the account mutably, so the `config` field in `InitializeAccounts` must be `&mut AccountView`.

**Fix:**
```rust
pub struct InitializeAccounts<'a> {
    pub config: &'a mut AccountView,  // was &'a AccountView
    ...
}
```
All `TryFrom<&'a mut [AccountView]>` impls had to be updated to match.

---

## 5. `Seeds` / `Signer` wrong module path

**File:** `src/instructions/initialize.rs`

**Error:**
```
error[E0432]: unresolved import `pinocchio::instruction::Seeds`
error[E0432]: unresolved import `pinocchio::instruction::Signer`
```

**Cause:** In pinocchio 0.11.2, `Seeds` was renamed to `Seed` (singular) and `Signer` moved to `pinocchio::cpi`.

**Fix:**
```rust
// wrong
use pinocchio::instruction::{Seeds, Signer};

// correct
use pinocchio::cpi::{Seed, Signer};
```

---

## 6. `find_program_address` not available on native target

**Files:** `src/instructions/initialize.rs`, `tests/initialize.rs`

**Error:**
```
error[E0425]: cannot find function `find_program_address` in module `Address`
```

**Cause:** `find_program_address` is gated behind `#[cfg(any(target_os = "solana", target_arch = "bpf"))]` — it's a syscall, only available on-chain. Tests run natively.

**Fix:** Use `derive_program_address` instead, which is pure Rust and available everywhere:
```rust
// wrong
let (pda, bump) = Address::find_program_address(&[...], &program_id);

// correct
let (pda, bump) = Address::derive_program_address(&[...], &program_id)
    .ok_or(ProgramError::InvalidSeeds)?;
```

---

## 7. `minimum_balance` deprecated

**File:** `src/instructions/initialize.rs`

**Error:**
```
error[E0599]: no method named `minimum_balance` found
```

**Cause:** `Rent::minimum_balance` was replaced by `try_minimum_balance` which returns `Result<u64, ProgramError>`.

**Fix:**
```rust
// wrong
Rent::get()?.minimum_balance(Config::LEN)

// correct
Rent::get()?.try_minimum_balance(Config::LEN)?
```

---

## 8. `pinocchio_token::state::TokenAccount` does not exist

**File:** `src/helper.rs`

**Error:**
```
error[E0412]: cannot find type `TokenAccount` in module `pinocchio_token::state`
```

**Cause:** The type is named `Account`, not `TokenAccount`.

**Fix:**
```rust
// wrong
use pinocchio_token::state::TokenAccount;

// correct
use pinocchio_token::state::Account;
```

---

## 9. `Some(authority_bytes)` type mismatch

**File:** `src/instructions/initialize.rs`

**Error:**
```
error[E0308]: mismatched types
  expected `Option<Address>`, found `Option<[u8; 32]>`
```

**Cause:** The `authority` field is `Option<Address>`, not `Option<[u8; 32]>`.

**Fix:**
```rust
// wrong
Some(authority_bytes)

// correct
Some(Address::new_from_array(authority_bytes))
```

---

## 10. Missing semicolon on `use` statement

**File:** `src/helper.rs`

**Error:**
```
error: expected `;`, found `pub`
```

**Cause:** A `use` statement was missing the trailing semicolon.

**Fix:**
```rust
// wrong
use pinocchio::{AccountView, error::ProgramError}

// correct
use pinocchio::{AccountView, error::ProgramError};
```

---

## 11. `unsafe_op_in_unsafe_fn` — Rust 2024 edition

**File:** `src/state/pool.rs`

**Error:**
```
error[E0133]: call to unsafe function is unsafe and requires unsafe block
```

**Cause:** Rust 2024 edition requires explicit `unsafe {}` blocks inside `unsafe fn`, not just the function signature.

**Fix:**
```rust
pub unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
    unsafe { &*(bytes.as_ptr() as *const Self) }  // explicit block required
}
```

---

## 12. Lifetime error on static `process` wrapper

**File:** `src/instructions/initialize.rs`

**Error:**
```
error[E0597]: `accounts` does not live long enough
```

**Cause:** The `process` function passed `accounts` into a `TryFrom` that tied its lifetime to `'a`, but without explicit lifetime annotations the compiler couldn't prove the borrow lasted long enough.

**Fix:** Annotate explicit lifetimes on `process`:
```rust
pub fn process(
    _program_id: &Address,
    accounts: &'a mut [AccountView],
    data: &'a [u8],
) -> ProgramResult { ... }
```

---

## 13. Integration test — wrong crate name

**File:** `tests/initialize.rs`

**Error:**
```
error[E0463]: can't find crate for `blueshift_native_amm`
```

**Cause:** The test was copied from another project and still referenced the old crate name.

**Fix:**
```rust
// wrong
use blueshift_native_amm::ID;

// correct
use amm::ID;
```

---

## 14. Integration test — `include_bytes!` compile-time failure

**File:** `tests/initialize.rs`

**Error:**
```
error: couldn't read `tests/../target/deploy/amm.so`: No such file or directory
```

**Cause:** `include_bytes!` embeds the file at compile time, so `cargo check` fails if the `.so` hasn't been built yet.

**Fix:** Use `std::fs::read` for a runtime load instead:
```rust
// wrong
let program_bytes = include_bytes!("../target/deploy/amm.so");

// correct
let program_bytes = std::fs::read("target/deploy/amm.so")
    .expect("build first: cargo build-sbf");
svm.add_program(program_id(), &program_bytes).unwrap();
```

---

## 15. Integration test — `Mint::LEN` requires `Pack` trait in scope

**File:** `tests/initialize.rs`

**Error:**
```
error[E0599]: no associated item named `LEN` found for struct `spl_token::state::Mint`
help: trait `Pack` which provides `LEN` is implemented but not in scope
```

**Fix:**
```rust
use spl_token::solana_program::program_pack::Pack;
```

---

## 16. Integration test — `get_associated_token_address` type mismatch

**File:** `tests/initialize.rs`

**Error:**
```
error[E0308]: mismatched types
  expected reference `&solana_pubkey::Pubkey`
             found reference `&Address`
```

**Cause:** `spl-associated-token-account-client 2.0.0` uses `solana-pubkey 2.1.x` where `Pubkey` is a concrete struct — not the same type as `solana_address::Address` used by the rest of the test.

**Fix:** Drop the spl crate's function and compute ATAs manually:
```rust
const TOKEN_PROGRAM_BYTES: [u8; 32] = [
    6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172,
    28, 180, 133, 237, 95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
];
const ATA_PROGRAM_BYTES: [u8; 32] = [
    140, 151, 37, 143, 78, 36, 137, 241, 187, 61, 16, 41, 20, 142, 13, 131,
    11, 90, 19, 153, 218, 255, 16, 132, 4, 142, 123, 216, 219, 233, 248, 89,
];

fn get_associated_token_address(wallet: &Address, mint: &Address) -> Address {
    let (ata, _) = Address::derive_program_address(
        &[wallet.as_ref(), &TOKEN_PROGRAM_BYTES, mint.as_ref()],
        &Address::new_from_array(ATA_PROGRAM_BYTES),
    ).expect("failed to derive ATA");
    ata
}
```

---

## 17. `spl_associated_token_account_client::program::ID` type mismatch

**File:** `tests/initialize.rs`

**Error:**
```
error[E0308]: mismatched types
  expected `Address`, found `Pubkey`
```

**Cause:** Same old-vs-new `solana-pubkey` version split as #16. The program ID constant from the spl crate is the wrong `Pubkey` type for `AccountMeta`.

**Fix:** Replace with the hardcoded `Address` constant:
```rust
// wrong
AccountMeta::new_readonly(spl_associated_token_account_client::program::ID, false)

// correct
AccountMeta::new_readonly(Address::new_from_array(ATA_PROGRAM_BYTES), false)
```

---

## 18. `nostd_panic_handler!()` — duplicate lang item `panic_impl`

**File:** `src/lib.rs`

**Error:**
```
error[E0152]: found duplicate lang item `panic_impl`
  = note: the lang item is first defined in crate `std`
    (which `constant_product_curve` depends on)
```

**Cause:** `constant-product-curve` (a git dependency) links against `std`, which already registers a panic handler. `nostd_panic_handler!()` registers a second one.

**Fix:** Remove `nostd_panic_handler!()` and its import:
```rust
// remove this line
nostd_panic_handler!();
```

---

## 19. `pinocchio_pubkey::declare_id!` produces `[u8; 32]`, not `Address`

**File:** `src/lib.rs`

**Error:**
```
error[E0308]: mismatched types
  expected `&Address`, found `&[u8; 32]`
  (on every use of `&crate::ID`)

error[E0277]: can't compare `Address` with `[u8; 32]`
  (in pool.rs owner checks)
```

**Cause:** `pinocchio_pubkey` 0.3.0 was written for pinocchio 0.9.x where `Pubkey = [u8; 32]`. In 0.11.x the type is `Address`. `declare_id!` expands to `pub const ID: [u8; 32] = ...`, which is incompatible with anything expecting `Address`.

**Fix:** Use `pubkey!` to get the bytes and wrap with `Address::new_from_array` (both are `const`):
```rust
// wrong
pinocchio_pubkey::declare_id!("2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE");

// correct
pub const ID: Address =
    Address::new_from_array(pinocchio_pubkey::pubkey!("2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE"));
```

---

## Summary Table

| # | File | Root Cause | Fix |
|---|------|-----------|-----|
| 1 | pool.rs | Wrong `Ref`/`RefMut` import | `pinocchio::account::{Ref, RefMut}` |
| 2 | pool.rs, initialize.rs | `Address` ≠ `[u8; 32]` | `Address::new_from_array(...)` |
| 3 | entrypoint + all handlers | Accounts not `&mut` | `&mut [AccountView]` everywhere |
| 4 | initialize.rs | `load_mut` needs `&mut AccountView` | `config: &'a mut AccountView` |
| 5 | initialize.rs | `Seeds`/`Signer` moved in 0.11.2 | `pinocchio::cpi::{Seed, Signer}` |
| 6 | initialize.rs, tests | `find_program_address` BPF-only | `derive_program_address` |
| 7 | initialize.rs | `minimum_balance` deprecated | `try_minimum_balance(...)?` |
| 8 | helper.rs | Wrong token account type name | `pinocchio_token::state::Account` |
| 9 | initialize.rs | `Some([u8;32])` vs `Some(Address)` | `Some(Address::new_from_array(...))` |
| 10 | helper.rs | Missing `;` on `use` | Add semicolon |
| 11 | pool.rs | Rust 2024 `unsafe_op_in_unsafe_fn` | Explicit `unsafe {}` block inside `unsafe fn` |
| 12 | initialize.rs | Missing lifetime on `process` | Explicit `'a` on accounts/data params |
| 13 | tests/initialize.rs | Old crate name | `use amm::ID` |
| 14 | tests/initialize.rs | `include_bytes!` at compile time | `std::fs::read(...)` at runtime |
| 15 | tests/initialize.rs | `Pack` trait not in scope | `use spl_token::solana_program::program_pack::Pack` |
| 16 | tests/initialize.rs | Old `solana-pubkey 2.x` Pubkey type in spl-ata-client | Manual ATA derivation with `derive_program_address` |
| 17 | tests/initialize.rs | Old `solana-pubkey 2.x` in spl-ata-client program ID | Hardcoded `ATA_PROGRAM_BYTES` constant |
| 18 | lib.rs | `constant-product-curve` pulls in `std` panic handler | Remove `nostd_panic_handler!()` |
| 19 | lib.rs | `pinocchio_pubkey::declare_id!` targets old `Pubkey=[u8;32]` | `Address::new_from_array(pubkey!(...))` |
