use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::get_associated_token_address,
    litesvm::{types::TransactionResult, LiteSVM},
    litesvm_token::{get_spl_account, spl_token, CreateAssociatedTokenAccount, CreateMint, MintTo},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const DECIMALS: u8 = 6;
/// One whole token at `DECIMALS` precision.
const ONE: u64 = 1_000_000;
const SOL: u64 = 1_000_000_000;

/// A maker holding mint A, a taker holding mint B, and the two mints.
struct World {
    svm: LiteSVM,
    maker: Keypair,
    taker: Keypair,
    mint_authority: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
}

impl World {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let bytes = include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/escrow.so"));
        svm.add_program(escrow::id(), bytes).unwrap();

        let maker = Keypair::new();
        let taker = Keypair::new();
        let mint_authority = Keypair::new();
        for who in [&maker, &taker, &mint_authority] {
            svm.airdrop(&who.pubkey(), 100 * SOL).unwrap();
        }

        let mint_a = CreateMint::new(&mut svm, &mint_authority)
            .decimals(DECIMALS)
            .send()
            .unwrap();
        let mint_b = CreateMint::new(&mut svm, &mint_authority)
            .decimals(DECIMALS)
            .send()
            .unwrap();

        // The maker is funded in A, the taker in B. The counterpart accounts
        // (maker's B, taker's A) are deliberately left uncreated so that
        // `take` exercises its `init_if_needed` path.
        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a)
            .send()
            .unwrap();
        let taker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &taker, &mint_b)
            .send()
            .unwrap();

        MintTo::new(&mut svm, &mint_authority, &mint_a, &maker_ata_a, 100 * ONE)
            .send()
            .unwrap();
        MintTo::new(&mut svm, &mint_authority, &mint_b, &taker_ata_b, 100 * ONE)
            .send()
            .unwrap();

        World {
            svm,
            maker,
            taker,
            mint_authority,
            mint_a,
            mint_b,
        }
    }

    fn maker_ata_a(&self) -> Pubkey {
        get_associated_token_address(&self.maker.pubkey(), &self.mint_a)
    }

    fn maker_ata_b(&self) -> Pubkey {
        get_associated_token_address(&self.maker.pubkey(), &self.mint_b)
    }

    fn taker_ata_a(&self) -> Pubkey {
        get_associated_token_address(&self.taker.pubkey(), &self.mint_a)
    }

    fn taker_ata_b(&self) -> Pubkey {
        get_associated_token_address(&self.taker.pubkey(), &self.mint_b)
    }

    fn balance(&self, ata: &Pubkey) -> u64 {
        get_spl_account::<spl_token::state::Account>(&self.svm, ata).map_or(0, |a| a.amount)
    }

    fn send(&mut self, ix: Instruction, payer: &Keypair) -> TransactionResult {
        self.svm.expire_blockhash();
        let msg = Message::new_with_blockhash(
            &[ix],
            Some(&payer.pubkey()),
            &self.svm.latest_blockhash(),
        );
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
        self.svm.send_transaction(tx)
    }

    fn escrow_state(&self, escrow: &Pubkey) -> escrow::state::Escrow {
        let account = self.svm.get_account(escrow).expect("escrow should exist");
        escrow::state::Escrow::try_deserialize(&mut &account.data[..]).unwrap()
    }
}

fn escrow_pda(maker: &Pubkey, seed: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            escrow::constants::ESCROW_SEED,
            maker.as_ref(),
            &seed.to_le_bytes(),
        ],
        &escrow::id(),
    )
    .0
}

fn assert_error(result: TransactionResult, expected: &str) {
    let failure = result.expect_err("expected the transaction to fail");
    let logs = failure.meta.logs.join("\n");
    assert!(
        logs.contains(expected),
        "expected `{expected}` in program logs, got:\n{logs}"
    );
}

fn make_ix(w: &World, seed: u64, deposit: u64, receive: u64, mint_b: Pubkey) -> Instruction {
    let escrow = escrow_pda(&w.maker.pubkey(), seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Make {
            seed,
            deposit,
            receive,
        }
        .data(),
        escrow::accounts::Make {
            maker: w.maker.pubkey(),
            mint_a: w.mint_a,
            mint_b,
            maker_ata_a: w.maker_ata_a(),
            escrow,
            vault: get_associated_token_address(&escrow, &w.mint_a),
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn take_ix(w: &World, seed: u64, taker: &Pubkey) -> Instruction {
    let escrow = escrow_pda(&w.maker.pubkey(), seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Take {}.data(),
        escrow::accounts::Take {
            taker: *taker,
            maker: w.maker.pubkey(),
            mint_a: w.mint_a,
            mint_b: w.mint_b,
            taker_ata_a: get_associated_token_address(taker, &w.mint_a),
            taker_ata_b: get_associated_token_address(taker, &w.mint_b),
            maker_ata_b: w.maker_ata_b(),
            escrow,
            vault: get_associated_token_address(&escrow, &w.mint_a),
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn refund_ix(w: &World, seed: u64, maker: &Pubkey) -> Instruction {
    let escrow = escrow_pda(&w.maker.pubkey(), seed);
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Refund {}.data(),
        escrow::accounts::Refund {
            maker: *maker,
            mint_a: w.mint_a,
            maker_ata_a: get_associated_token_address(maker, &w.mint_a),
            escrow,
            vault: get_associated_token_address(&escrow, &w.mint_a),
            associated_token_program: anchor_spl::associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn update_ix(w: &World, seed: u64, maker: &Pubkey, receive: u64) -> Instruction {
    Instruction::new_with_bytes(
        escrow::id(),
        &escrow::instruction::Update { receive }.data(),
        escrow::accounts::Update {
            maker: *maker,
            escrow: escrow_pda(&w.maker.pubkey(), seed),
        }
        .to_account_metas(None),
    )
}

#[test]
fn make_locks_the_deposit_and_records_the_terms() {
    let mut w = World::new();
    let escrow = escrow_pda(&w.maker.pubkey(), 1);
    let vault = get_associated_token_address(&escrow, &w.mint_a);

    let maker_before = w.balance(&w.maker_ata_a());
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    let maker = w.maker.insecure_clone();
    w.send(ix, &maker).unwrap();

    assert_eq!(w.balance(&vault), 10 * ONE);
    assert_eq!(w.balance(&w.maker_ata_a()), maker_before - 10 * ONE);

    let state = w.escrow_state(&escrow);
    assert_eq!(state.seed, 1);
    assert_eq!(state.maker, w.maker.pubkey());
    assert_eq!(state.mint_a, w.mint_a);
    assert_eq!(state.mint_b, w.mint_b);
    assert_eq!(state.receive, 25 * ONE);
}

#[test]
fn make_rejects_a_zero_deposit() {
    let mut w = World::new();
    let ix = make_ix(&w, 1, 0, 25 * ONE, w.mint_b);
    let maker = w.maker.insecure_clone();
    assert_error(w.send(ix, &maker), "InvalidAmount");
}

#[test]
fn make_rejects_a_zero_asking_price() {
    let mut w = World::new();
    let ix = make_ix(&w, 1, 10 * ONE, 0, w.mint_b);
    let maker = w.maker.insecure_clone();
    assert_error(w.send(ix, &maker), "InvalidAmount");
}

#[test]
fn make_rejects_swapping_a_mint_for_itself() {
    let mut w = World::new();
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_a);
    let maker = w.maker.insecure_clone();
    assert_error(w.send(ix, &maker), "IdenticalMints");
}

#[test]
fn maker_can_run_several_escrows_in_parallel() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();

    let first = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(first, &maker).unwrap();
    let second = make_ix(&w, 2, 5 * ONE, 40 * ONE, w.mint_b);
    w.send(second, &maker).unwrap();

    let a = escrow_pda(&maker.pubkey(), 1);
    let b = escrow_pda(&maker.pubkey(), 2);
    assert_ne!(a, b);
    assert_eq!(w.escrow_state(&a).receive, 25 * ONE);
    assert_eq!(w.escrow_state(&b).receive, 40 * ONE);
    assert_eq!(w.balance(&get_associated_token_address(&a, &w.mint_a)), 10 * ONE);
    assert_eq!(w.balance(&get_associated_token_address(&b, &w.mint_a)), 5 * ONE);
}

#[test]
fn take_settles_both_legs_and_closes_the_escrow() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);
    let vault = get_associated_token_address(&escrow, &w.mint_a);

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let taker_b_before = w.balance(&w.taker_ata_b());
    let ix = take_ix(&w, 1, &taker.pubkey());
    w.send(ix, &taker).unwrap();

    // The taker got the deposit, the maker got the asking price.
    assert_eq!(w.balance(&w.taker_ata_a()), 10 * ONE);
    assert_eq!(w.balance(&w.maker_ata_b()), 25 * ONE);
    assert_eq!(w.balance(&w.taker_ata_b()), taker_b_before - 25 * ONE);

    // Both the vault and the escrow state are gone.
    assert_eq!(w.svm.get_account(&vault).map_or(0, |a| a.lamports), 0);
    assert_eq!(w.svm.get_account(&escrow).map_or(0, |a| a.lamports), 0);
}

#[test]
fn take_is_permissionless() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    // A third party who holds mint B can fill the offer just as well.
    let stranger = Keypair::new();
    w.svm.airdrop(&stranger.pubkey(), 100 * SOL).unwrap();
    let stranger_ata_b = CreateAssociatedTokenAccount::new(&mut w.svm, &stranger, &w.mint_b)
        .send()
        .unwrap();
    let authority = w.mint_authority.insecure_clone();
    MintTo::new(&mut w.svm, &authority, &w.mint_b, &stranger_ata_b, 30 * ONE)
        .send()
        .unwrap();

    let ix = take_ix(&w, 1, &stranger.pubkey());
    w.send(ix, &stranger).unwrap();

    assert_eq!(
        w.balance(&get_associated_token_address(&stranger.pubkey(), &w.mint_a)),
        10 * ONE
    );
    assert_eq!(w.balance(&w.maker_ata_b()), 25 * ONE);
}

#[test]
fn take_fails_when_the_taker_cannot_pay() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    // Asking for more mint B than the taker owns.
    let ix = make_ix(&w, 1, 10 * ONE, 500 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let taker = w.taker.insecure_clone();
    let ix = take_ix(&w, 1, &taker.pubkey());
    assert!(w.send(ix, &taker).is_err());

    // The deposit is still locked and the escrow still open.
    let escrow = escrow_pda(&maker.pubkey(), 1);
    assert_eq!(
        w.balance(&get_associated_token_address(&escrow, &w.mint_a)),
        10 * ONE
    );
}

#[test]
fn refund_returns_the_deposit_and_closes_the_escrow() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);
    let vault = get_associated_token_address(&escrow, &w.mint_a);

    let before = w.balance(&w.maker_ata_a());
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();
    assert_eq!(w.balance(&vault), 10 * ONE);

    let ix = refund_ix(&w, 1, &maker.pubkey());
    w.send(ix, &maker).unwrap();

    assert_eq!(w.balance(&w.maker_ata_a()), before);
    assert_eq!(w.svm.get_account(&vault).map_or(0, |a| a.lamports), 0);
    assert_eq!(w.svm.get_account(&escrow).map_or(0, |a| a.lamports), 0);
}

#[test]
fn refund_rejects_a_stranger() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let attacker = Keypair::new();
    w.svm.airdrop(&attacker.pubkey(), 100 * SOL).unwrap();
    CreateAssociatedTokenAccount::new(&mut w.svm, &attacker, &w.mint_a)
        .send()
        .unwrap();

    // The escrow PDA is derived from the signer, so pointing at someone else's
    // escrow no longer matches the seeds.
    let ix = refund_ix(&w, 1, &attacker.pubkey());
    assert_error(w.send(ix, &attacker), "ConstraintSeeds");

    let escrow = escrow_pda(&maker.pubkey(), 1);
    assert_eq!(
        w.balance(&get_associated_token_address(&escrow, &w.mint_a)),
        10 * ONE
    );
}

#[test]
fn refund_cannot_run_twice() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let ix = refund_ix(&w, 1, &maker.pubkey());
    w.send(ix, &maker).unwrap();

    let ix = refund_ix(&w, 1, &maker.pubkey());
    assert!(w.send(ix, &maker).is_err());
}

#[test]
fn update_changes_the_asking_price() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let ix = update_ix(&w, 1, &maker.pubkey(), 12 * ONE);
    w.send(ix, &maker).unwrap();

    let state = w.escrow_state(&escrow);
    assert_eq!(state.receive, 12 * ONE);
    // Everything else is untouched, including the locked deposit.
    assert_eq!(state.mint_b, w.mint_b);
    assert_eq!(
        w.balance(&get_associated_token_address(&escrow, &w.mint_a)),
        10 * ONE
    );
}

#[test]
fn update_rejects_a_zero_price() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let ix = update_ix(&w, 1, &maker.pubkey(), 0);
    assert_error(w.send(ix, &maker), "InvalidAmount");
}

#[test]
fn update_rejects_a_stranger() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();

    let attacker = Keypair::new();
    w.svm.airdrop(&attacker.pubkey(), 100 * SOL).unwrap();

    let ix = update_ix(&w, 1, &attacker.pubkey(), ONE);
    assert_error(w.send(ix, &attacker), "ConstraintSeeds");

    assert_eq!(
        w.escrow_state(&escrow_pda(&maker.pubkey(), 1)).receive,
        25 * ONE
    );
}

#[test]
fn take_settles_at_the_updated_price() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, w.mint_b);
    w.send(ix, &maker).unwrap();
    let ix = update_ix(&w, 1, &maker.pubkey(), 12 * ONE);
    w.send(ix, &maker).unwrap();

    let taker_b_before = w.balance(&w.taker_ata_b());
    let ix = take_ix(&w, 1, &taker.pubkey());
    w.send(ix, &taker).unwrap();

    // The taker paid the new price, not the original one.
    assert_eq!(w.balance(&w.maker_ata_b()), 12 * ONE);
    assert_eq!(w.balance(&w.taker_ata_b()), taker_b_before - 12 * ONE);
    assert_eq!(w.balance(&w.taker_ata_a()), 10 * ONE);
}
