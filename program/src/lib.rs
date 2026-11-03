//! Counter program.
//!
//! | Discriminator | Instruction | Accounts |
//! |---|---|---|
//! | 0 | `Initialize` | `[signer, writable] authority`, `[writable] counter PDA`, `[] system program` |
//! | 1 | `Increment`  | `[signer] authority`, `[writable] counter PDA` |
//!
//! Counter PDA seeds: `["counter", authority]`.
#![no_std]

mod error;
mod instructions;
mod state;

pub use {error::CounterError, state::COUNTER_SEED, state::Counter};

use pinocchio::{
    AccountView, Address, ProgramResult, error::ProgramError, no_allocator, nostd_panic_handler,
    program_entrypoint,
};

program_entrypoint!(process_instruction);
no_allocator!();
nostd_panic_handler!();

pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data {
        [instructions::INITIALIZE] => instructions::initialize(program_id, accounts),
        [instructions::INCREMENT] => instructions::increment(program_id, accounts),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
