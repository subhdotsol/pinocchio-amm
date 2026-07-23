#![no_std]

use pinocchio::{Address, no_allocator, program_entrypoint};

pub mod constants;
pub mod entrypoint;
pub mod error;
pub mod helper;
pub mod instructions;
pub mod state;

program_entrypoint!(entrypoint::process_instruction);
no_allocator!();

pub const ID: Address = Address::new_from_array(pinocchio_pubkey::pubkey!(
    "2zmvAfAG6t839jmhL9uim6yp9WBrSJyqN9TTeuoEQmkE"
));
