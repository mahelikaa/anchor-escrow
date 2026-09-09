pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("GiH3qkYZ7a51T58RSw8TbzNtuxJpKEVshri7cFshPQeb");

#[program]
pub mod escrow {
    use super::*;

    /// Opens an offer: locks `deposit` of mint A in a vault and records the
    /// `receive` amount of mint B the maker wants for it.
    pub fn make(ctx: Context<Make>, seed: u64, deposit: u64, receive: u64) -> Result<()> {
        instructions::make::handle_make(ctx, seed, deposit, receive)
    }

    /// Settles the swap in one transaction and closes the escrow.
    pub fn take(ctx: Context<Take>) -> Result<()> {
        instructions::take::handle_take(ctx)
    }

    /// Cancels an unfilled offer and returns the deposit to the maker.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        instructions::refund::handle_refund(ctx)
    }

    /// Changes the asking price of an open offer.
    pub fn update(ctx: Context<Update>, receive: u64) -> Result<()> {
        instructions::update::handle_update(ctx, receive)
    }
}
