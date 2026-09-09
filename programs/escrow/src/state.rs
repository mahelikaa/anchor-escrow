use anchor_lang::prelude::*;

/// Terms of a single swap offer.
///
/// The deposited tokens live in a separate vault token account whose authority
/// is this PDA, so releasing them always requires the program to sign.
#[account]
#[derive(InitSpace)]
pub struct Escrow {
    /// Lets one maker run several escrows at the same time.
    pub seed: u64,
    pub maker: Pubkey,
    /// The mint the maker deposited.
    pub mint_a: Pubkey,
    /// The mint the maker wants in return.
    pub mint_b: Pubkey,
    /// How much of `mint_b` the taker must pay.
    pub receive: u64,
    pub bump: u8,
}
