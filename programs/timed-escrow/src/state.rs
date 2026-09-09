use anchor_lang::prelude::*;

/// A swap offer that is only claimable inside a time window, and only by
/// someone who can prove they know a secret.
///
/// The two conditions are deliberately complementary: before `expires_at` only
/// the taker can act (and only with the preimage), after it only the maker can.
/// There is no window in which both or neither can move, so the deposit is
/// never strandable.
#[account]
#[derive(InitSpace)]
pub struct TimedEscrow {
    pub seed: u64,
    pub maker: Pubkey,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub receive: u64,
    /// Unix timestamp after which the offer can no longer be taken.
    pub expires_at: i64,
    /// SHA-256 of the secret the taker must reveal to claim the deposit.
    ///
    /// The maker hands the secret over off-chain once they are satisfied the
    /// other side of the deal happened — a delivery confirmation, a signed
    /// receipt, whatever the two parties agreed on. Revealing it on-chain is
    /// what releases the funds.
    pub hashlock: [u8; 32],
    pub bump: u8,
}
