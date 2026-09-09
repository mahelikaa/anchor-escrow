use anchor_lang::prelude::*;

use crate::{constants::*, error::TimedEscrowError, state::TimedEscrow};

#[derive(Accounts)]
pub struct Update<'info> {
    pub maker: Signer<'info>,

    #[account(
        mut,
        has_one = maker,
        seeds = [TIMED_ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, TimedEscrow>,
}

/// Re-prices an offer and/or moves its deadline.
///
/// Only allowed while the offer is still live: once it has expired the maker's
/// only route is `refund`, otherwise they could revive a dead offer and
/// surprise a taker who had already written it off.
pub fn handle_update(ctx: Context<Update>, receive: u64, expires_at: i64) -> Result<()> {
    require!(receive > 0, TimedEscrowError::InvalidAmount);

    let now = Clock::get()?.unix_timestamp;
    require!(
        now < ctx.accounts.escrow.expires_at,
        TimedEscrowError::Expired
    );
    require!(expires_at > now, TimedEscrowError::InvalidDeadline);

    ctx.accounts.escrow.receive = receive;
    ctx.accounts.escrow.expires_at = expires_at;

    Ok(())
}
