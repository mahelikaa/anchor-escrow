use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

use crate::{constants::*, state::Escrow};

/// Settling touches eight deserialised accounts at once. Left on the stack that
/// overflows an SBF frame, so the heavy ones are boxed onto the heap.
#[derive(Accounts)]
pub struct Take<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,

    /// Receives the taker's payment plus the rent from both closed accounts.
    #[account(mut)]
    pub maker: SystemAccount<'info>,

    #[account(mint::token_program = token_program)]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,

    #[account(mint::token_program = token_program)]
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = mint_a,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    pub taker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_b,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    pub taker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = mint_b,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        close = maker,
        has_one = maker,
        has_one = mint_a,
        has_one = mint_b,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Box<Account<'info, Escrow>>,

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

pub fn handle_take(ctx: Context<Take>) -> Result<()> {
    // Leg one: the taker pays the maker. Both legs are in the same
    // transaction, so either the whole swap settles or none of it does.
    let pay_maker = CpiContext::new(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.taker_ata_b.to_account_info(),
            mint: ctx.accounts.mint_b.to_account_info(),
            to: ctx.accounts.maker_ata_b.to_account_info(),
            authority: ctx.accounts.taker.to_account_info(),
        },
    );
    transfer_checked(
        pay_maker,
        ctx.accounts.escrow.receive,
        ctx.accounts.mint_b.decimals,
    )?;

    // Leg two: the escrow releases the deposit. Only the program can authorise
    // this, by signing with the escrow PDA's seeds.
    let maker_key = ctx.accounts.escrow.maker;
    let seed_bytes = ctx.accounts.escrow.seed.to_le_bytes();
    let seeds: &[&[u8]] = &[
        ESCROW_SEED,
        maker_key.as_ref(),
        seed_bytes.as_ref(),
        &[ctx.accounts.escrow.bump],
    ];
    let signer_seeds: &[&[&[u8]]] = &[seeds];

    let release = CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        TransferChecked {
            from: ctx.accounts.vault.to_account_info(),
            mint: ctx.accounts.mint_a.to_account_info(),
            to: ctx.accounts.taker_ata_a.to_account_info(),
            authority: ctx.accounts.escrow.to_account_info(),
        },
        signer_seeds,
    );
    transfer_checked(
        release,
        ctx.accounts.vault.amount,
        ctx.accounts.mint_a.decimals,
    )?;

    // A token account belongs to the token program, so Anchor's `close`
    // constraint cannot reclaim it — it takes an explicit CPI.
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
