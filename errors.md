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

## 20. `deposit.rs` — Missing semicolon + missing `size_of` import

**File:** `src/instructions/deposit.rs`

**Error:**
```
error: expected `;`, found `pub`
error[E0425]: cannot find function `size_of` in this scope
```

**Fix:**
```rust
use core::mem::size_of;   // add at top

use crate::{ ... };       // add missing ; at end
```

---

## 21. `deposit.rs` — `user_ata_lp` field missing from struct

**File:** `src/instructions/deposit.rs`

**Error:**
```
error[E0560]: struct `DepositAccounts` has no field named `user_ata_lp`
```

**Cause:** The account was destructured from the slice and used in `Ok(Self{...})` but never declared in the struct itself.

**Fix:** Add the field to `DepositAccounts`:
```rust
pub struct DepositAccounts<'a> {
    ...
    pub user_ata_lp: &'a AccountView,   // add this
    ...
}
```

---

## 22. `deposit.rs` — `TryFrom` missing `<'a>` lifetime on target type

**File:** `src/instructions/deposit.rs`

**Error:**
```
error[E0726]: implicit elided lifetime not allowed here
```

**Cause:** `TryFrom<&'a mut [AccountView]> for DepositAccounts` — missing `<'a>` on `DepositAccounts`.

**Fix:**
```rust
// wrong
impl<'a> TryFrom<&'a mut [AccountView]> for DepositAccounts {

// correct
impl<'a> TryFrom<&'a mut [AccountView]> for DepositAccounts<'a> {
```

---

## 23. `deposit.rs` — Wrong owner address for user ATA checks

**File:** `src/instructions/deposit.rs`

**Cause:** `user_ata_x` and `user_ata_y` are token accounts owned by the **user**, not by the **config** PDA. Using `config.address()` makes the ATA derivation produce the wrong address and the check always fails at runtime.

**Fix:**
```rust
// wrong — vaults are owned by config, user ATAs are not
AssociatedTokenAccount::check(user_ata_x, config.address(), ...)?;
AssociatedTokenAccount::check(user_ata_y, config.address(), ...)?;

// correct
AssociatedTokenAccount::check(user_ata_x, user.address(), ...)?;
AssociatedTokenAccount::check(user_ata_y, user.address(), ...)?;
```

---

## 24. `deposit.rs` — `process(&self)` doesn't match entrypoint dispatch

**File:** `src/instructions/deposit.rs`

**Error:**
```
error[E0061]: this function takes 1 argument but 3 arguments were supplied
```

**Cause:** `entrypoint.rs` calls `Deposit::process(program_id, accounts, rest)` as a static method. The implementation had `pub fn process(&self)` — an instance method with no matching signature.

**Fix:** Mirror the `Initialize` pattern — static `process` creates the struct, private `run` holds the logic:
```rust
pub fn process(
    _program_id: &Address,
    accounts: &'a mut [AccountView],
    data: &'a [u8],
) -> ProgramResult {
    let mut ix = Self::try_from((data, accounts))?;
    ix.run()
}

fn run(&mut self) -> ProgramResult { ... }
```

---

## 25. `deposit.rs` — `TokenAccount` + `from_account_info` don't exist in pinocchio-token 0.6.0

**File:** `src/instructions/deposit.rs`

**Error:**
```
error[E0425]: cannot find value `TokenAccount` in module `pinocchio_token::state`
error[E0599]: no method named `from_account_info` found
```

**Cause:** The type is `Account` (not `TokenAccount`) and the constructor is `from_account_view` (not `from_account_info`) in pinocchio-token 0.6.0.

**Fix:**
```rust
// wrong
pinocchio_token::state::TokenAccount::from_account_info(self.accounts.vault_x)?.amount()

// correct
pinocchio_token::state::Account::from_account_view(self.accounts.vault_x)?.amount()
```

---

## 26. `deposit.rs` — `CURVE_PRECISION` type mismatch (`u8` vs `u32`)

**File:** `src/instructions/deposit.rs`

**Error:**
```
error[E0308]: mismatched types
  expected `u32`, found `u8`
```

**Cause:** `ConstantProduct::xy_deposit_amounts_from_l` takes precision as `u32` but `CURVE_PRECISION` is declared as `u8` in constants.rs.

**Fix:**
```rust
CURVE_PRECISION as u32
```

---

## 27. `deposit.rs` — `TransferChecked`/`MintTo` missing `multisig_signers` field

**File:** `src/instructions/deposit.rs`

**Error:**
```
error[E0063]: missing field `multisig_signers` in initializer of `TransferChecked`
error[E0063]: missing field `multisig_signers` in initializer of `MintTo`
```

**Cause:** pinocchio-token 0.6.0 added a `multisig_signers` field to `TransferChecked` and `MintTo` to support multisig authorities. Must be supplied even when empty.

**Fix:** Declare a typed empty slice and pass it to both structs:
```rust
let no_signers: &[&AccountView] = &[];

TransferChecked {
    ...
    multisig_signers: no_signers,
    ...
}.invoke()?;

MintTo {
    ...
    multisig_signers: no_signers,
    ...
}.invoke_signed(&signer)
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
| 20 | deposit.rs | Missing `;` and `size_of` import | Add both |
| 21 | deposit.rs | `user_ata_lp` not in struct | Add field |
| 22 | deposit.rs | Missing `<'a>` on `TryFrom` target | `for DepositAccounts<'a>` |
| 23 | deposit.rs | User ATA checked against wrong owner | Use `user.address()` not `config.address()` |
| 24 | deposit.rs | `process(&self)` vs static dispatch | Static `process` + private `run` |
| 25 | deposit.rs | `TokenAccount`/`from_account_info` wrong names | `Account::from_account_view` |
| 26 | deposit.rs | `CURVE_PRECISION` is `u8`, needs `u32` | `CURVE_PRECISION as u32` |
| 27 | deposit.rs | `multisig_signers` field missing | `let no_signers: &[&AccountView] = &[]` |
| 28 | swap.rs | `TryFrom` missing `mut` + `<'a>` on target | `&'a mut [AccountView]` + `for SwapAccounts<'a>` |
| 29 | swap.rs | `find_program_address` BPF-only | `derive_program_address(...).ok_or(...)` |
| 30 | swap.rs | Two conflicting `impl Swap` blocks | Removed dead `todo!()` stub, merged into one `impl<'a> Swap<'a>` |
| 31 | swap.rs | `process(&self)` wrong signature | Static `process` + private `run` |
| 32 | swap.rs | `TokenAccount`/`from_account_info` wrong names | `Account::from_account_view` / `Mint::from_account_view` |
| 33 | swap.rs | `TransferChecked` missing `multisig_signers` ×2 | `let no_signers: &[&AccountView] = &[]` |
| 34 | withdraw.rs | `pinocchio::instruction::{Seed, Signer}` wrong path | `pinocchio::cpi::{Seed, Signer}` |
| 35 | withdraw.rs | `TryFrom` missing `mut` + `<'a>` on target | `&'a mut [AccountView]` + `for WithdrawAccounts<'a>` |
| 36 | withdraw.rs | `find_program_address` BPF-only | `derive_program_address(...).ok_or(...)` |
| 37 | withdraw.rs | `process(&self)` wrong signature | Static `process` + private `run` |
| 38 | withdraw.rs | `TokenAccount`/`from_account_info` wrong names | `Account::from_account_view` / `Mint::from_account_view` |
| 39 | withdraw.rs | `CURVE_PRECISION` is `u8`, needs `u32` | `CURVE_PRECISION as u32` |
| 40 | withdraw.rs | `Burn` + `TransferChecked` missing `multisig_signers` ×3 | `let no_signers: &[&AccountView] = &[]` |

---

## Swap-specific errors

### 28–29. `swap.rs` — `TryFrom` not `mut`, `find_program_address` BPF-only

Same pattern as deposit (#22, #6). See those entries.

---

### 30. `swap.rs` — Two conflicting `impl Swap` blocks

**Error:**
```
error[E0201]: duplicate definitions with name `process`
```

**Cause:** The file had both a leftover `todo!()` stub and the real implementation as separate `impl` blocks:
```rust
impl Swap {                    // no lifetime — wrong type
    pub fn process(...) { todo!() }
}
impl<'a> Swap<'a> {
    pub fn process(&self) ...  // instance method — also wrong
}
```

**Fix:** Delete the stub block entirely. Merge everything into a single `impl<'a> Swap<'a>` using the static `process` + private `run` pattern.

---

## Withdraw-specific errors

### 34. `withdraw.rs` — `Seed`/`Signer` still imported from `pinocchio::instruction`

**Error:**
```
error[E0432]: unresolved import `pinocchio::instruction::Seed`
error[E0432]: unresolved import `pinocchio::instruction::Signer`
```

**Cause:** Same as error #5 (initialize.rs) — in pinocchio 0.11.2 these moved to `pinocchio::cpi`. The withdraw file was written against the old API.

**Fix:**
```rust
// wrong
use pinocchio::instruction::{Seed, Signer};

// correct
use pinocchio::cpi::{Seed, Signer};
```

---

### 40. `withdraw.rs` — `Burn` also has `multisig_signers`

**Error:**
```
error[E0063]: missing field `multisig_signers` in initializer of `Burn`
```

**Cause:** Same as error #27. pinocchio-token 0.6.0 added `multisig_signers` to `Burn` as well, not just `TransferChecked` and `MintTo`.

**Fix:** Same `no_signers` pattern:
```rust
let no_signers: &[&AccountView] = &[];

Burn {
    mint: ...,
    account: ...,
    authority: ...,
    multisig_signers: no_signers,
    amount: ...,
}.invoke()?;
```

**Pattern to remember:** Any pinocchio-token 0.6.0 instruction that involves an authority (`Burn`, `MintTo`, `TransferChecked`, `Transfer`) has `multisig_signers`. Always grep the struct definition before constructing it:
```bash
grep -n "pub struct Burn\|pub multisig" \
  ~/.cargo/registry/src/.../pinocchio-token-0.6.0/src/instructions/burn.rs
```

---

## How to Debug These Yourself

### 1. Read the error code, not just the message

Every `cargo check` error has a code like `E0308`, `E0599`, `E0063`. Run:
```bash
rustc --explain E0308
```
This gives a full explanation with examples. The compiler's `help:` and `note:` lines printed inline are also usually the direct answer — read those before anything else.

### 2. Look up the actual crate source in the registry

When a type or method doesn't exist or has the wrong signature, go read the source:
```bash
find ~/.cargo/registry/src -path "*pinocchio-token-0.6.0*" -name "account.rs"
```
Open it and search for the method name. The registry cache at `~/.cargo/registry/src/` has every downloaded version of every crate — always accurate, never outdated docs.

### 3. Diagnose version conflicts with Cargo.lock

When you see `expected &SomeType, found &SomeType` (same name, different crates), grep Cargo.lock:
```bash
grep -A5 'name = "solana-pubkey"' Cargo.lock
```
Multiple entries = multiple versions in the tree = incompatible types even if they look identical. Fix by pinning versions or avoiding the conflicting crate's functions.

### 4. Expand macros when they misbehave

If a macro produces the wrong type (like `declare_id!` producing `[u8;32]` instead of `Address`):
```bash
cargo expand 2>/dev/null | grep -A3 "pub const ID"
```
This shows exactly what the macro expands to, revealing the actual type emitted.

### 5. Read the struct definition when fields are missing

When you get `missing field X` or `wrong number of arguments`, the error always prints:
```
note: defined here --> /path/to/crate/src/file.rs:42
```
Open that file at that line. Don't guess the fields — read the struct definition directly.

### 6. Check which target a function is available on

If a function compiles fine with `cargo build-sbf` but errors on `cargo check` (native), it is likely gated behind:
```rust
#[cfg(any(target_os = "solana", target_arch = "bpf"))]
```
Grep the crate source for the function name to confirm. Use the pure-Rust alternative for code that must compile on both targets (e.g. `derive_program_address` instead of `find_program_address`).

---

## Note on `declare_id!()`

We did use `pinocchio_pubkey::declare_id!("2zmv...")` — and it caused **error #19** because that macro was written for pinocchio 0.9.x where `Pubkey = [u8; 32]`. In 0.11.x the type switched to `Address`, so every use of `crate::ID` produced a type mismatch.

The pinocchio-token and pinocchio-associated-token-account crates declare their IDs correctly using `solana_address::declare_id!`. We could do the same by promoting `solana-address` from `[dev-dependencies]` to `[dependencies]`:

```toml
[dependencies]
solana-address = "2.6.1"
```
```rust
// This would emit: pub const ID: Address = ...; plus check_id() and id() helpers
solana_address::declare_id!("2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE");
```

The current workaround is equivalent and avoids the extra dependency:
```rust
pub const ID: Address =
    Address::new_from_array(pinocchio_pubkey::pubkey!("2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE"));
```
It just doesn't generate the `check_id()` / `id()` helpers, which we don't use anyway.
