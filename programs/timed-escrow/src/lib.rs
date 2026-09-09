pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("7fDt53vo9t1ddnXfTeCsCtvWocae3n7GPihxDeWcbica");

/// An escrow whose settlement is gated on two things at once: a deadline read
/// from the `Clock` sysvar, and a secret the taker has to reveal.
#[program]
pub mod timed_escrow {
    use super::*;

    /// Opens a hash-locked offer that stops being takeable at `expires_at`.
    pub fn make(
        ctx: Context<Make>,
        seed: u64,
        deposit: u64,
        receive: u64,
        expires_at: i64,
        hashlock: [u8; 32],
    ) -> Result<()> {
        instructions::make::handle_make(ctx, seed, deposit, receive, expires_at, hashlock)
    }

    /// Settles the swap by revealing the secret before the deadline.
    pub fn take(ctx: Context<Take>, preimage: [u8; 32]) -> Result<()> {
        instructions::take::handle_take(ctx, preimage)
    }

    /// Reclaims the deposit once the offer has expired unfilled.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        instructions::refund::handle_refund(ctx)
    }

    /// Re-prices a live offer or extends its deadline.
    pub fn update(ctx: Context<Update>, receive: u64, expires_at: i64) -> Result<()> {
        instructions::update::handle_update(ctx, receive, expires_at)
    }
}
