//! Counter program.
//!
//! Instruction data: byte 0 is the discriminator (`CounterInstruction` in `instructions/mod.rs`);
//! the remaining bytes are that instruction's arguments. Each processor in `instructions/`
//! documents its accounts, arguments, and errors.
//!
//! | Discriminator | Instruction | Processor |
//! |---|---|---|
//! | 0 | `Initialize` | `instructions/initialize.rs` |
//! | 1 | `Increment`  | `instructions/increment.rs` |
//! | 2 | `Deposit`    | `instructions/deposit.rs` |
//! | 3 | `Withdraw`   | `instructions/withdraw.rs` |
//!
//! Counter PDA seeds: `["counter", authority]`. The vault is an SPL Token account owned by the
//! counter PDA (`state/vault.rs`).
#![no_std]

mod error;
mod instructions;
mod state;

pub use {error::CounterError, state::COUNTER_SEED, state::Counter};

use instructions::CounterInstruction;
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
    let (discriminator, data) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;
    match CounterInstruction::try_from(*discriminator)? {
        CounterInstruction::Initialize => instructions::initialize(program_id, accounts, data),
        CounterInstruction::Increment => instructions::increment(program_id, accounts, data),
        CounterInstruction::Deposit => instructions::deposit(program_id, accounts, data),
        CounterInstruction::Withdraw => instructions::withdraw(program_id, accounts, data),
    }
}
