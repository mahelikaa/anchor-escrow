use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

use crate::{constants::*, error::TimedEscrowError, state::TimedEscrow};

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        close = maker,
        has_one = maker,
        has_one = mint_a,
        seeds = [TIMED_ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Box<Account<'info, TimedEscrow>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handle_refund(ctx: Context<Refund>) -> Result<()> {
    // Refunding early would let the maker pull the deposit out from under a
    // taker who is mid-way through satisfying the condition, so the maker has
    // to wait for the window to close.
    let now = Clock::get()?.unix_timestamp;
    require!(
        now >= ctx.accounts.escrow.expires_at,
        TimedEscrowError::NotExpired
    );

    let maker_key = ctx.accounts.escrow.maker;
    let seed_bytes = ctx.accounts.escrow.seed.to_le_bytes();
    let seeds: &[&[u8]] = &[
        TIMED_ESCROW_SEED,
        maker_key.as_ref(),
        seed_bytes.as_ref(),
        &[ctx.accounts.escrow.bump],
    ];
    let signer_seeds: &[&[&[u8]]] = &[seeds];

    let return_deposit = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.vault.to_account_info(),
            mint: ctx.accounts.mint_a.to_account_info(),
            to: ctx.accounts.maker_ata_a.to_account_info(),
            authority: ctx.accounts.escrow.to_account_info(),
        },
        signer_seeds,
    );
    transfer_checked(
        return_deposit,
        ctx.accounts.vault.amount,
        ctx.accounts.mint_a.decimals,
    )?;

    let close_vault = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.vault.to_account_info(),
            destination: ctx.accounts.maker.to_account_info(),
            authority: ctx.accounts.escrow.to_account_info(),
        },
        signer_seeds,
    );

    close_account(close_vault)
}
