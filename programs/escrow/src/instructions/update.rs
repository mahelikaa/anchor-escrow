use anchor_lang::prelude::*;

use crate::{constants::*, error::EscrowError, state::Escrow};

#[derive(Accounts)]
pub struct Update<'info> {
    pub maker: Signer<'info>,

    #[account(
        mut,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,
}

/// Re-prices an open offer. The deposit is untouched, so the maker can react to
/// the market without cancelling and re-opening the escrow.
pub fn handle_update(ctx: Context<Update>, receive: u64) -> Result<()> {
    require!(receive > 0, EscrowError::InvalidAmount);

    ctx.accounts.escrow.receive = receive;

    Ok(())
}
