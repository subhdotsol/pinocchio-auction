mod deposit;
mod increment;
mod initialize;
mod withdraw;

pub use {deposit::deposit, increment::increment, initialize::initialize, withdraw::withdraw};

use pinocchio::error::ProgramError;

/// First byte of instruction data. The remaining bytes are the instruction's arguments,
/// documented on each processor.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterInstruction {
    Initialize = 0,
    Increment = 1,
    Deposit = 2,
    Withdraw = 3,
}

impl TryFrom<u8> for CounterInstruction {
    type Error = ProgramError;

    fn try_from(discriminator: u8) -> Result<Self, Self::Error> {
        match discriminator {
            0 => Ok(Self::Initialize),
            1 => Ok(Self::Increment),
            2 => Ok(Self::Deposit),
            3 => Ok(Self::Withdraw),
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}
