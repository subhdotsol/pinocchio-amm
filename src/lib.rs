#![no_std]

use pinocchio::{Address, no_allocator, nostd_panic_handler, program_entrypoint};

pub mod constants;
pub mod entrypoint;
pub mod error;
pub mod helper;
pub mod instructions;
pub mod state;

program_entrypoint!(entrypoint::process_instruction);
no_allocator!();
nostd_panic_handler!();

// Placeholder — replace after `cargo build-sbf` +
// `solana-keygen pubkey ./target/deploy/amm-keypair.json`.
pub const ID: Address = Address::new_from_array([1u8; 32]);

#[cfg(test)]
mod tests;
