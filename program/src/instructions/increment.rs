use pinocchio::{AccountView, Address, ProgramResult, error::ProgramError};
use solana_program_log::log;

use crate::{error::CounterError, state::Counter};

/// # Increment (discriminator `1`)
///
/// Adds 1 to the counter.
///
/// ## Business logic
/// 1. Only the authority stored in the counter may increment it.
/// 2. The owner check runs before the account data is read, so a look-alike account owned by
///    another program is rejected.
/// 3. The addition is checked: at `u64::MAX` it fails instead of wrapping to 0.
///
/// ## Accounts
/// 0. `[signer]` authority - must equal `Counter::authority`
/// 1. `[writable]` counter - counter PDA owned by this program
///
/// ## Instruction data (after the discriminator)
/// None. Any extra bytes return `InvalidInstructionData`.
///
/// ## Errors
/// - `NotEnoughAccountKeys`: fewer than 2 accounts
/// - `InvalidInstructionData`: extra bytes after the discriminator
/// - `MissingRequiredSignature`: authority did not sign
/// - `InvalidAccountOwner`: counter is not owned by this program
/// - `InvalidAccountData`: wrong size or discriminator
/// - `CounterError::Unauthorized` (`0x0`): signer is not the counter's authority
/// - `CounterError::Overflow` (`0x1`): count is already `u64::MAX`
pub fn increment(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if !data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
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
