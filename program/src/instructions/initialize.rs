use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};
use pinocchio_system::instructions::CreateAccount;
use solana_program_log::log;

use crate::state::{COUNTER_SEED, Counter};

/// # Initialize (discriminator `0`)
///
/// Creates the authority's counter PDA with `count = 0`.
///
/// ## Business logic
/// 1. The authority signs and pays rent for the new account.
/// 2. The program derives the canonical bump itself; a bump sent by the client is never trusted.
/// 3. The counter is created through a signed System Program CPI, so only this program can
///    create an account at that address.
/// 4. One counter per authority: a second `Initialize` fails.
///
/// ## Accounts
/// 0. `[signer, writable]` authority - pays rent and becomes the counter's authority
/// 1. `[writable]` counter - PDA `["counter", authority]`, must not exist yet
/// 2. `[]` system_program - System Program
///
/// ## Instruction data (after the discriminator)
/// None. Any extra bytes return `InvalidInstructionData`.
///
/// ## Errors
/// - `NotEnoughAccountKeys`: fewer than 3 accounts
/// - `InvalidInstructionData`: extra bytes after the discriminator
/// - `MissingRequiredSignature`: authority did not sign
/// - `IncorrectProgramId`: account 2 is not the System Program
/// - `AccountAlreadyInitialized`: counter is already owned by this program
/// - `InvalidSeeds`: counter is not the canonical PDA for this authority
pub fn initialize(
    program_id: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    if !data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let [authority, counter, system_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.address() != &pinocchio_system::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if counter.owned_by(program_id) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    // Canonical bump only: never trust a bump sent by the client.
    let (expected, bump) =
        Address::find_program_address(&[COUNTER_SEED, authority.address().as_ref()], program_id);
    if counter.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump_seed = [bump];
    let seeds = [
        Seed::from(COUNTER_SEED),
        Seed::from(authority.address().as_ref()),
        Seed::from(&bump_seed),
    ];
    CreateAccount::with_minimum_balance(authority, counter, Counter::LEN as u64, program_id, None)?
        .invoke_signed(&[Signer::from(&seeds)])?;

    let mut data = counter.try_borrow_mut()?;
    let state = Counter::from_bytes_mut_uninit(&mut data)?;
    state.discriminator = Counter::DISCRIMINATOR;
    state.bump = bump;
    state.authority = Address::new_from_array(authority.address().to_bytes());
    state.set_count(0);

    log!("counter initialized");
    Ok(())
}
