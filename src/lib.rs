#![no_std]

use pinocchio::Address;
#[cfg(target_os = "solana")]
use pinocchio::{no_allocator, program_entrypoint};

pub mod constants;
pub mod entrypoint;
pub mod error;
pub mod helper;
pub mod instructions;
pub mod state;

#[cfg(target_os = "solana")]
program_entrypoint!(entrypoint::process_instruction);
#[cfg(target_os = "solana")]
no_allocator!();

pub const ID: Address = Address::new_from_array(pinocchio_pubkey::pubkey!(
    "2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE"
));
