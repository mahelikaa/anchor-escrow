use anchor_lang::prelude::*;

#[error_code]
pub enum TimedEscrowError {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("An escrow cannot swap a mint for itself")]
    IdenticalMints,
    #[msg("The deadline must be in the future")]
    InvalidDeadline,
    #[msg("The offer has expired")]
    Expired,
    #[msg("The offer has not expired yet")]
    NotExpired,
    #[msg("The secret does not match the hashlock")]
    InvalidPreimage,
}
