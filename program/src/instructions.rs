use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};
use pinocchio_system::instructions::CreateAccount;
use solana_program_log::log;

use crate::{error::CounterError, state::COUNTER_SEED, state::Counter};

pub const INITIALIZE: u8 = 0;
pub const INCREMENT: u8 = 1;

pub fn initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
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

pub fn increment(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [authority, counter, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !counter.owned_by(program_id) {
        return Err(ProgramError::InvalidAccountOwner);
    }

    let mut data = counter.try_borrow_mut()?;
    let state = Counter::from_bytes_mut(&mut data)?;
    if &state.authority != authority.address() {
        return Err(CounterError::Unauthorized.into());
    }
    let count = state.count().checked_add(1).ok_or(CounterError::Overflow)?;
    state.set_count(count);

    log!("count = {}", count);
    Ok(())
}
