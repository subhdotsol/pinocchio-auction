use pinocchio::{Address, error::ProgramError};

pub const COUNTER_SEED: &[u8] = b"counter";

/// Counter account, 48 bytes:
///
/// | Offset | Size | Field |
/// |---|---|---|
/// | 0  | 1  | discriminator (`1`) |
/// | 1  | 1  | bump |
/// | 2  | 6  | padding |
/// | 8  | 32 | authority |
/// | 40 | 8  | count (u64 little-endian) |
///
/// Every field has alignment 1, so casting account data to `&Counter` is sound at any address.
#[repr(C)]
pub struct Counter {
    pub discriminator: u8,
    pub bump: u8,
    _padding: [u8; 6],
    pub authority: Address,
    count: [u8; 8],
}

const _: () = assert!(size_of::<Counter>() == 48 && align_of::<Counter>() == 1);

impl Counter {
    pub const DISCRIMINATOR: u8 = 1;
    pub const LEN: usize = size_of::<Self>();

    pub fn count(&self) -> u64 {
        u64::from_le_bytes(self.count)
    }

    pub fn set_count(&mut self, count: u64) {
        self.count = count.to_le_bytes();
    }

    /// Checks length and discriminator. The caller must check the account owner first.
    pub fn from_bytes_mut(data: &mut [u8]) -> Result<&mut Self, ProgramError> {
        if data.len() != Self::LEN || data[0] != Self::DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        // SAFETY: length checked; alignment is 1.
        Ok(unsafe { &mut *(data.as_mut_ptr() as *mut Self) })
    }

    /// For a freshly created (zeroed) account. Checks length only.
    pub fn from_bytes_mut_uninit(data: &mut [u8]) -> Result<&mut Self, ProgramError> {
        if data.len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        // SAFETY: length checked; alignment is 1; every bit pattern is valid.
        Ok(unsafe { &mut *(data.as_mut_ptr() as *mut Self) })
    }
}
