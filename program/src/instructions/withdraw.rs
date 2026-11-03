use core::mem::MaybeUninit;

use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};
use pinocchio_token::instructions::{Batch, CloseAccount, IntoBatch, Transfer};
use solana_program_log::log;

use crate::{
    error::CounterError,
    state::{COUNTER_SEED, Counter, vault_balance},
};

/// Batch buffers for exactly Transfer + CloseAccount, single-owner authority (3 accounts each).
const BATCH_DATA_LEN: usize =
    Batch::header_data_len(2) + Transfer::DATA_LEN + CloseAccount::DATA_LEN;
const BATCH_ACCOUNTS_LEN: usize = 3 + 3;

/// # Withdraw (discriminator `3`)
///
/// Sends the vault's whole balance to `destination`, then closes the vault and returns its rent
/// to the authority. Same shape as refunding an outbid bidder from an auction vault.
///
/// ## Business logic
/// 1. Only the counter's authority can withdraw.
/// 2. The counter PDA owns the vault, so the program signs as the counter with seeds
///    `["counter", authority, bump]`. The bump is the canonical one stored at `Initialize`, so
///    no `find_program_address` is needed here.
/// 3. Transfer and CloseAccount go to the Token program in one p-token `Batch` CPI
///    (discriminator 255), so the CPI base cost is paid once and one `signers` array signs both.
///    `Batch` exists only in p-token (SIMD-0266), which runs at the SPL Token address on
///    devnet, mainnet, LiteSVM, and surfpool. The legacy SPL Token program rejects it.
/// 4. All account data borrows end before the batch is built; the batch is invoked without a
///    second borrow check, so nothing may borrow these accounts in between.
/// 5. Transfer is first in the batch because the Token program only closes an account with a
///    zero balance.
///
/// ## Accounts
/// 0. `[signer, writable]` authority - must equal `Counter::authority`; receives the vault rent
/// 1. `[]` counter - counter PDA owned by this program
/// 2. `[writable]` vault - token account owned by the counter PDA
/// 3. `[writable]` destination - token account with the vault's mint
/// 4. `[]` token_program - SPL Token program
///
/// ## Instruction data (after the discriminator)
/// None. Any extra bytes return `InvalidInstructionData`.
///
/// ## Errors
/// - `InvalidInstructionData`: extra bytes after the discriminator
/// - `NotEnoughAccountKeys`: fewer than 5 accounts
/// - `MissingRequiredSignature`: authority did not sign
/// - `IncorrectProgramId`: account 4 is not the SPL Token program
/// - `InvalidAccountOwner`: counter is not owned by this program
/// - `InvalidAccountData`: counter has the wrong size or discriminator
/// - `CounterError::Unauthorized` (`0x0`): signer is not the counter's authority
/// - `CounterError::InvalidVault` (`0x2`): vault is not a token account owned by the counter
pub fn withdraw(program_id: &Address, accounts: &mut [AccountView], data: &[u8]) -> ProgramResult {
    if !data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    let [authority, counter, vault, destination, token_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if token_program.address() != &pinocchio_token::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !counter.owned_by(program_id) {
        return Err(ProgramError::InvalidAccountOwner);
    }

    // Read what the CPIs need, then drop the borrow at the end of the block.
    let bump = {
        let data = counter.try_borrow()?;
        let state = Counter::from_bytes(&data)?;
        if &state.authority != authority.address() {
            return Err(CounterError::Unauthorized.into());
        }
        state.bump
    };
    let amount = vault_balance(vault, counter.address())?;

    let bump_seed = [bump];
    let seeds = [
        Seed::from(COUNTER_SEED),
        Seed::from(authority.address().as_ref()),
        Seed::from(&bump_seed),
    ];
    let signers = [Signer::from(&seeds)];

    let mut batch_data = [const { MaybeUninit::uninit() }; BATCH_DATA_LEN];
    let mut instruction_accounts = [const { MaybeUninit::uninit() }; BATCH_ACCOUNTS_LEN];
    let mut cpi_accounts = [const { MaybeUninit::uninit() }; BATCH_ACCOUNTS_LEN];
    let mut batch = Batch::new(
        &mut batch_data,
        &mut instruction_accounts,
        &mut cpi_accounts,
    )?;
    // `into_batch` checks that the writable accounts are not borrowed.
    if amount > 0 {
        Transfer::new(vault, destination, counter, amount).into_batch(&mut batch)?;
    }
    CloseAccount::new(vault, authority, counter).into_batch(&mut batch)?;
    // SAFETY: no account borrow is alive. The counter and vault borrows ended above, and no
    // account is borrowed between `into_batch` and this call.
    unsafe { batch.invoke_signed_unchecked(&signers)? };

    log!("withdrew {} and closed the vault", amount);
    Ok(())
}
