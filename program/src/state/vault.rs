use pinocchio::{AccountView, Address, error::ProgramError};
use pinocchio_token::state::Account as TokenAccount;

use crate::error::CounterError;

/// A counter's vault is any SPL Token account whose owner (token authority) is the counter PDA,
/// for example the counter PDA's associated token account. The client creates it; the program
/// only checks it.
///
/// Returns the vault's token balance. The account data borrow ends when this returns, so the
/// vault can be passed to a CPI right after.
pub fn vault_balance(vault: &AccountView, counter: &Address) -> Result<u64, ProgramError> {
    // Checks the account is owned by the SPL Token program and is exactly 165 bytes.
    let token = TokenAccount::from_account_view(vault).map_err(|_| CounterError::InvalidVault)?;
    if token.owner() != counter {
        return Err(CounterError::InvalidVault.into());
    }
    Ok(token.amount())
}
