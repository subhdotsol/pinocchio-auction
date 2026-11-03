use pinocchio::error::ProgramError;

/// Returned as `ProgramError::Custom(code)`; clients see `custom program error: 0x<code>`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterError {
    /// Signer is not the counter's authority.
    Unauthorized = 0,
    /// `count` would exceed `u64::MAX`.
    Overflow = 1,
}

impl From<CounterError> for ProgramError {
    fn from(e: CounterError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
