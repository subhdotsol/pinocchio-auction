use pinocchio::{AccountView, Address, ProgramResult, error::ProgramError};
use pinocchio_token::instructions::Transfer;
use solana_program_log::log;

use crate::{
    error::CounterError,
    state::{Counter, vault_balance},
};

/// # Deposit (discriminator `2`)
///
/// Moves SPL tokens from the depositor's token account into the counter's vault.
///
/// ## Business logic
/// 1. Anyone can deposit into any counter's vault; the depositor signs for their own tokens.
/// 2. The vault must be an SPL Token account whose owner is the counter PDA, so only this
///    program (signing as the counter) can move tokens out of it again.
/// 3. The Transfer CPI is signed by the depositor, so it uses `invoke`, not `invoke_signed`.
///    The Token program checks that `source` and `vault` have the same mint.
///
/// ## Accounts
/// 0. `[signer]` depositor - owner of `source`
/// 1. `[writable]` source - depositor's token account
/// 2. `[]` counter - counter PDA owned by this program
/// 3. `[writable]` vault - token account owned by the counter PDA
/// 4. `[]` token_program - SPL Token program
///
/// ## Instruction data (after the discriminator)
/// | Offset | Size | Field |
/// |---|---|---|
/// | 0 | 8 | amount (`u64` little-endian), must be > 0 |
///
/// ## Errors
/// - `InvalidInstructionData`: data is not exactly 8 bytes
/// - `CounterError::ZeroAmount` (`0x3`): amount is 0
/// - `NotEnoughAccountKeys`: fewer than 5 accounts
/// - `MissingRequiredSignature`: depositor did not sign
/// - `IncorrectProgramId`: account 4 is not the SPL Token program
/// - `InvalidAccountOwner`: counter is not owned by this program
/// - `InvalidAccountData`: counter has the wrong size or discriminator
/// - `CounterError::InvalidVault` (`0x2`): vault is not a token account owned by the counter
pub fn deposit(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    // `try_into` copies the 8 bytes into a `[u8; 8]`, so the read never depends on the
    // alignment of `data` (casting `data.as_ptr()` to `*const u64` would be undefined behavior).
    let amount = u64::from_le_bytes(
        data.try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    if amount == 0 {
        return Err(CounterError::ZeroAmount.into());
    }

    let [depositor, source, counter, vault, token_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !depositor.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if token_program.address() != &pinocchio_token::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !counter.owned_by(program_id) {
        return Err(ProgramError::InvalidAccountOwner);
    }
    Counter::from_bytes(&counter.try_borrow()?)?;
    vault_balance(vault, counter.address())?;

    Transfer::new(source, vault, depositor, amount).invoke()?;

    log!("deposited {}", amount);
    Ok(())
}
