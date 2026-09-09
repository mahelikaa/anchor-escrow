use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::get_associated_token_address,
    litesvm::{types::TransactionResult, LiteSVM},
    litesvm_token::{get_spl_account, spl_token, CreateAssociatedTokenAccount, CreateMint, MintTo},
    solana_clock::Clock,
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const DECIMALS: u8 = 6;
const ONE: u64 = 1_000_000;
const SOL: u64 = 1_000_000_000;

/// Fixed starting point so deadlines in the tests read as absolute times.
const START: i64 = 1_700_000_000;
const HOUR: i64 = 3_600;

/// The secret the taker must reveal, and the hash the maker publishes.
const PREIMAGE: [u8; 32] = [7u8; 32];

fn hashlock() -> [u8; 32] {
    solana_sha256_hasher::hash(&PREIMAGE).to_bytes()
}

struct World {
    svm: LiteSVM,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
}

impl World {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let bytes = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/timed_escrow.so"
        ));
        svm.add_program(timed_escrow::id(), bytes).unwrap();

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

        let mut world = World {
            svm,
            maker,
            taker,
            mint_a,
            mint_b,
        };
        world.set_time(START);
        world
    }

    /// Overwrites the `Clock` sysvar the program reads through `Clock::get()`.
    fn set_time(&mut self, unix_timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar(&clock);
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
        let msg =
            Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &self.svm.latest_blockhash());
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
        self.svm.send_transaction(tx)
    }

    fn escrow_state(&self, escrow: &Pubkey) -> timed_escrow::state::TimedEscrow {
        let account = self.svm.get_account(escrow).expect("escrow should exist");
        timed_escrow::state::TimedEscrow::try_deserialize(&mut &account.data[..]).unwrap()
    }
}

fn escrow_pda(maker: &Pubkey, seed: u64) -> Pubkey {
    Pubkey::find_program_address(
        &[
            timed_escrow::constants::TIMED_ESCROW_SEED,
            maker.as_ref(),
            &seed.to_le_bytes(),
        ],
        &timed_escrow::id(),
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

fn make_ix(w: &World, seed: u64, deposit: u64, receive: u64, expires_at: i64) -> Instruction {
    let escrow = escrow_pda(&w.maker.pubkey(), seed);
    Instruction::new_with_bytes(
        timed_escrow::id(),
        &timed_escrow::instruction::Make {
            seed,
            deposit,
            receive,
            expires_at,
            hashlock: hashlock(),
        }
        .data(),
        timed_escrow::accounts::Make {
            maker: w.maker.pubkey(),
            mint_a: w.mint_a,
            mint_b: w.mint_b,
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

fn take_ix(w: &World, seed: u64, preimage: [u8; 32]) -> Instruction {
    let escrow = escrow_pda(&w.maker.pubkey(), seed);
    let taker = w.taker.pubkey();
    Instruction::new_with_bytes(
        timed_escrow::id(),
        &timed_escrow::instruction::Take { preimage }.data(),
        timed_escrow::accounts::Take {
            taker,
            maker: w.maker.pubkey(),
            mint_a: w.mint_a,
            mint_b: w.mint_b,
            taker_ata_a: w.taker_ata_a(),
            taker_ata_b: w.taker_ata_b(),
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

fn refund_ix(w: &World, seed: u64) -> Instruction {
    let escrow = escrow_pda(&w.maker.pubkey(), seed);
    Instruction::new_with_bytes(
        timed_escrow::id(),
        &timed_escrow::instruction::Refund {}.data(),
        timed_escrow::accounts::Refund {
            maker: w.maker.pubkey(),
            mint_a: w.mint_a,
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

fn update_ix(w: &World, seed: u64, maker: &Pubkey, receive: u64, expires_at: i64) -> Instruction {
    Instruction::new_with_bytes(
        timed_escrow::id(),
        &timed_escrow::instruction::Update {
            receive,
            expires_at,
        }
        .data(),
        timed_escrow::accounts::Update {
            maker: *maker,
            escrow: escrow_pda(&w.maker.pubkey(), seed),
        }
        .to_account_metas(None),
    )
}

#[test]
fn make_records_the_deadline_and_the_hashlock() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    let state = w.escrow_state(&escrow);
    assert_eq!(state.expires_at, START + HOUR);
    assert_eq!(state.hashlock, hashlock());
    assert_eq!(
        w.balance(&get_associated_token_address(&escrow, &w.mint_a)),
        10 * ONE
    );
}

#[test]
fn make_rejects_a_deadline_in_the_past() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START - HOUR);
    assert_error(w.send(ix, &maker), "InvalidDeadline");
}

#[test]
fn take_settles_with_the_right_secret_before_the_deadline() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    // Still inside the window.
    w.set_time(START + HOUR / 2);
    let ix = take_ix(&w, 1, PREIMAGE);
    w.send(ix, &taker).unwrap();

    assert_eq!(w.balance(&w.taker_ata_a()), 10 * ONE);
    assert_eq!(w.balance(&w.maker_ata_b()), 25 * ONE);
    assert_eq!(w.svm.get_account(&escrow).map_or(0, |a| a.lamports), 0);
}

#[test]
fn take_rejects_the_wrong_secret() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    let ix = take_ix(&w, 1, [9u8; 32]);
    assert_error(w.send(ix, &taker), "InvalidPreimage");

    // Nothing moved.
    assert_eq!(w.balance(&w.taker_ata_a()), 0);
}

#[test]
fn take_rejects_once_the_deadline_has_passed() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    // One second past expiry is already too late, even with the right secret.
    w.set_time(START + HOUR + 1);
    let ix = take_ix(&w, 1, PREIMAGE);
    assert_error(w.send(ix, &taker), "Expired");

    assert_eq!(w.balance(&w.taker_ata_a()), 0);
}

#[test]
fn refund_rejects_while_the_offer_is_still_live() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    let ix = refund_ix(&w, 1);
    assert_error(w.send(ix, &maker), "NotExpired");

    let escrow = escrow_pda(&maker.pubkey(), 1);
    assert_eq!(
        w.balance(&get_associated_token_address(&escrow, &w.mint_a)),
        10 * ONE
    );
}

#[test]
fn refund_returns_the_deposit_after_expiry() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);
    let vault = get_associated_token_address(&escrow, &w.mint_a);

    let before = w.balance(&w.maker_ata_a());
    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    w.set_time(START + HOUR);
    let ix = refund_ix(&w, 1);
    w.send(ix, &maker).unwrap();

    assert_eq!(w.balance(&w.maker_ata_a()), before);
    assert_eq!(w.svm.get_account(&vault).map_or(0, |a| a.lamports), 0);
    assert_eq!(w.svm.get_account(&escrow).map_or(0, |a| a.lamports), 0);
}

#[test]
fn the_two_windows_never_overlap() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    // Before the deadline: the taker may act, the maker may not.
    w.set_time(START + HOUR - 1);
    let ix = refund_ix(&w, 1);
    assert_error(w.send(ix, &maker), "NotExpired");

    // After it: the maker may act, the taker may not.
    w.set_time(START + HOUR);
    let ix = take_ix(&w, 1, PREIMAGE);
    assert_error(w.send(ix, &taker), "Expired");

    let ix = refund_ix(&w, 1);
    w.send(ix, &maker).unwrap();
}

#[test]
fn update_extends_the_deadline_and_reprices() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();
    let taker = w.taker.insecure_clone();
    let escrow = escrow_pda(&maker.pubkey(), 1);

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    let ix = update_ix(&w, 1, &maker.pubkey(), 12 * ONE, START + 4 * HOUR);
    w.send(ix, &maker).unwrap();

    let state = w.escrow_state(&escrow);
    assert_eq!(state.receive, 12 * ONE);
    assert_eq!(state.expires_at, START + 4 * HOUR);

    // A take that would have been too late under the original deadline now
    // settles, and at the new price.
    w.set_time(START + 2 * HOUR);
    let ix = take_ix(&w, 1, PREIMAGE);
    w.send(ix, &taker).unwrap();

    assert_eq!(w.balance(&w.maker_ata_b()), 12 * ONE);
    assert_eq!(w.balance(&w.taker_ata_a()), 10 * ONE);
}

#[test]
fn update_rejects_an_expired_offer() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    // An expired offer cannot be revived; refund is the only way out.
    w.set_time(START + 2 * HOUR);
    let ix = update_ix(&w, 1, &maker.pubkey(), 12 * ONE, START + 8 * HOUR);
    assert_error(w.send(ix, &maker), "Expired");
}

#[test]
fn update_rejects_a_deadline_in_the_past() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    let ix = update_ix(&w, 1, &maker.pubkey(), 12 * ONE, START - HOUR);
    assert_error(w.send(ix, &maker), "InvalidDeadline");
}

#[test]
fn update_rejects_a_stranger() {
    let mut w = World::new();
    let maker = w.maker.insecure_clone();

    let ix = make_ix(&w, 1, 10 * ONE, 25 * ONE, START + HOUR);
    w.send(ix, &maker).unwrap();

    let attacker = Keypair::new();
    w.svm.airdrop(&attacker.pubkey(), 100 * SOL).unwrap();

    let ix = update_ix(&w, 1, &attacker.pubkey(), ONE, START + 8 * HOUR);
    assert_error(w.send(ix, &attacker), "ConstraintSeeds");

    assert_eq!(
        w.escrow_state(&escrow_pda(&maker.pubkey(), 1)).receive,
        25 * ONE
    );
}
