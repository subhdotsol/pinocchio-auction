//! Runs `../target/deploy/auction.so` in LiteSVM with v1 transactions.
//! Build it first: `cargo build-sbf --tools-version v1.58 --arch v3` from the project root.

use litesvm::LiteSVM;
use litesvm_token::{
    get_spl_account, spl_token::state::Account as TokenAccount, CreateAssociatedTokenAccount,
    CreateMint, MintTo, TOKEN_ID,
};
use solana_account::Account;
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::{v1, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::{versioned::VersionedTransaction, InstructionError, TransactionError};

const PROGRAM_SO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../target/deploy/auction.so");
const SYSTEM_PROGRAM: Address = Address::new_from_array([0; 32]);
const COUNTER_LEN: usize = 48;
// LiteSVM's mainnet feature set includes SIMD-0266, so this address runs p-token.
const TOKEN_PROGRAM: Address = TOKEN_ID;

// Unset v1 limits are ZERO, not runtime defaults: always set the CU and loaded-data limits.
const CONFIG: v1::TransactionConfig = v1::TransactionConfig::empty()
    .with_compute_unit_limit(20_000)
    .with_loaded_accounts_data_size_limit(64 * 1024)
    .with_priority_fee(1_000);
// Token instructions also load the token program (p-token, ~100 KB), which counts toward the limit.
const TOKEN_CONFIG: v1::TransactionConfig = CONFIG
    .with_compute_unit_limit(50_000)
    .with_loaded_accounts_data_size_limit(256 * 1024);

struct Ctx {
    svm: LiteSVM,
    program_id: Address,
    authority: Keypair,
}

fn setup() -> Ctx {
    let so = std::fs::read(PROGRAM_SO)
        .unwrap_or_else(|e| panic!("{PROGRAM_SO}: {e}. Run `cargo build-sbf --arch v3` first"));
    // e_flags (offset 0x30) holds the SBPF version. `cargo build-sbf` defaults to v0.
    assert_eq!(so[0x30], 3, "expected an SBPF v3 build, got v{}", so[0x30]);

    let mut svm = LiteSVM::new();
    let program_id = Address::new_unique();
    svm.add_program(program_id, &so)
        .expect("LiteSVM could not load the program");
    let authority = Keypair::new();
    svm.airdrop(&authority.pubkey(), 10_000_000_000).unwrap();
    Ctx {
        svm,
        program_id,
        authority,
    }
}

fn counter_pda(program_id: &Address, authority: &Address) -> (Address, u8) {
    Address::find_program_address(&[b"counter", authority.as_ref()], program_id)
}

fn initialize_ix(program_id: Address, authority: Address, counter: Address) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &[0],
        vec![
            AccountMeta::new(authority, true),
            AccountMeta::new(counter, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
        ],
    )
}

fn increment_ix(program_id: Address, authority: Address, counter: Address) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &[1],
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(counter, false),
        ],
    )
}

fn send(
    svm: &mut LiteSVM,
    ix: Instruction,
    payer: &Keypair,
    signers: &[&Keypair],
    config: v1::TransactionConfig,
) -> Result<(), TransactionError> {
    let message = v1::Message::try_compile_with_config(
        &payer.pubkey(),
        &[ix],
        svm.latest_blockhash(),
        config,
    )
    .unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(message), signers).unwrap();
    let result = svm.send_transaction(tx).map(|_| ()).map_err(|e| e.err);
    // Identical transactions in the same blockhash are rejected as AlreadyProcessed.
    svm.expire_blockhash();
    result
}

fn count(svm: &LiteSVM, counter: &Address) -> u64 {
    let data = svm.get_account(counter).unwrap().data;
    u64::from_le_bytes(data[40..48].try_into().unwrap())
}

fn initialized() -> (Ctx, Address) {
    let mut ctx = setup();
    let (counter, _) = counter_pda(&ctx.program_id, &ctx.authority.pubkey());
    let ix = initialize_ix(ctx.program_id, ctx.authority.pubkey(), counter);
    send(&mut ctx.svm, ix, &ctx.authority, &[&ctx.authority], CONFIG).unwrap();
    (ctx, counter)
}

fn custom(code: u32) -> TransactionError {
    TransactionError::InstructionError(0, InstructionError::Custom(code))
}

fn token_amount(svm: &LiteSVM, address: &Address) -> u64 {
    get_spl_account::<TokenAccount>(svm, address).unwrap().amount
}

fn lamports(svm: &LiteSVM, address: &Address) -> u64 {
    svm.get_account(address).map_or(0, |a| a.lamports)
}

fn deposit_ix(
    program_id: Address,
    depositor: Address,
    source: Address,
    counter: Address,
    vault: Address,
    amount: u64,
) -> Instruction {
    let mut data = vec![2];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction::new_with_bytes(
        program_id,
        &data,
        vec![
            AccountMeta::new_readonly(depositor, true),
            AccountMeta::new(source, false),
            AccountMeta::new_readonly(counter, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
        ],
    )
}

fn withdraw_ix(
    program_id: Address,
    authority: Address,
    counter: Address,
    vault: Address,
    destination: Address,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &[3],
        vec![
            AccountMeta::new(authority, true),
            AccountMeta::new_readonly(counter, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(destination, false),
            AccountMeta::new_readonly(TOKEN_PROGRAM, false),
        ],
    )
}

/// Real accounts created through the Token and Associated Token Account programs.
struct Tokens {
    mint: Address,
    /// Authority's ATA, starts with 100.
    source: Address,
    /// The counter PDA's ATA, starts empty.
    vault: Address,
}

fn with_vault() -> (Ctx, Address, Tokens) {
    let (mut ctx, counter) = initialized();
    let mint = CreateMint::new(&mut ctx.svm, &ctx.authority)
        .decimals(6)
        .send()
        .unwrap();
    let source = CreateAssociatedTokenAccount::new(&mut ctx.svm, &ctx.authority, &mint)
        .send()
        .unwrap();
    // The ATA program accepts an off-curve owner, so a PDA can have an ATA.
    let vault = CreateAssociatedTokenAccount::new(&mut ctx.svm, &ctx.authority, &mint)
        .owner(&counter)
        .send()
        .unwrap();
    MintTo::new(&mut ctx.svm, &ctx.authority, &mint, &source, 100)
        .send()
        .unwrap();
    (ctx, counter, Tokens { mint, source, vault })
}

fn deposit(
    ctx: &mut Ctx,
    counter: Address,
    tokens: &Tokens,
    amount: u64,
) -> Result<(), TransactionError> {
    let ix = deposit_ix(
        ctx.program_id,
        ctx.authority.pubkey(),
        tokens.source,
        counter,
        tokens.vault,
        amount,
    );
    send(
        &mut ctx.svm,
        ix,
        &ctx.authority,
        &[&ctx.authority],
        TOKEN_CONFIG,
    )
}

#[test]
fn initialize_creates_counter() {
    let (ctx, counter) = initialized();
    let (_, bump) = counter_pda(&ctx.program_id, &ctx.authority.pubkey());
    let account = ctx.svm.get_account(&counter).unwrap();

    assert_eq!(account.owner, ctx.program_id);
    assert_eq!(account.data.len(), COUNTER_LEN);
    assert_eq!(account.data[0], 1, "discriminator");
    assert_eq!(account.data[1], bump, "canonical bump stored");
    assert_eq!(&account.data[8..40], ctx.authority.pubkey().as_ref());
    assert_eq!(count(&ctx.svm, &counter), 0);
    assert_eq!(
        account.lamports,
        ctx.svm.minimum_balance_for_rent_exemption(COUNTER_LEN)
    );
}

#[test]
fn initialize_twice_fails() {
    let (mut ctx, counter) = initialized();
    let ix = initialize_ix(ctx.program_id, ctx.authority.pubkey(), counter);
    let err = send(&mut ctx.svm, ix, &ctx.authority, &[&ctx.authority], CONFIG).unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::AccountAlreadyInitialized)
    );
}

#[test]
fn initialize_rejects_foreign_pda() {
    let mut ctx = setup();
    let (someone_elses, _) = counter_pda(&ctx.program_id, &Address::new_unique());
    let ix = initialize_ix(ctx.program_id, ctx.authority.pubkey(), someone_elses);
    let err = send(&mut ctx.svm, ix, &ctx.authority, &[&ctx.authority], CONFIG).unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::InvalidSeeds)
    );
}

#[test]
fn increment_by_authority() {
    let (mut ctx, counter) = initialized();
    for expected in 1..=2 {
        let ix = increment_ix(ctx.program_id, ctx.authority.pubkey(), counter);
        send(&mut ctx.svm, ix, &ctx.authority, &[&ctx.authority], CONFIG).unwrap();
        assert_eq!(count(&ctx.svm, &counter), expected);
    }
}

#[test]
fn increment_unauthorized() {
    let (mut ctx, counter) = initialized();
    let intruder = Keypair::new();
    let ix = increment_ix(ctx.program_id, intruder.pubkey(), counter);
    // authority pays the fee, intruder signs as `authority`.
    let err = send(
        &mut ctx.svm,
        ix,
        &ctx.authority,
        &[&ctx.authority, &intruder],
        CONFIG,
    )
    .unwrap_err();
    assert_eq!(err, custom(0));
    assert_eq!(count(&ctx.svm, &counter), 0);
}

#[test]
fn increment_overflow() {
    let (mut ctx, counter) = initialized();
    let mut account: Account = ctx.svm.get_account(&counter).unwrap();
    account.data[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    ctx.svm.set_account(counter, account).unwrap();

    let ix = increment_ix(ctx.program_id, ctx.authority.pubkey(), counter);
    let err = send(&mut ctx.svm, ix, &ctx.authority, &[&ctx.authority], CONFIG).unwrap_err();
    assert_eq!(err, custom(1));
    assert_eq!(count(&ctx.svm, &counter), u64::MAX);
}

#[test]
fn v1_compute_limit_is_enforced() {
    let (mut ctx, counter) = initialized();
    let tight = CONFIG.with_compute_unit_limit(100);
    let ix = increment_ix(ctx.program_id, ctx.authority.pubkey(), counter);
    let err = send(&mut ctx.svm, ix, &ctx.authority, &[&ctx.authority], tight).unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(0, InstructionError::ComputationalBudgetExceeded)
    );
}

#[test]
fn deposit_moves_tokens_into_vault() {
    let (mut ctx, counter, tokens) = with_vault();
    deposit(&mut ctx, counter, &tokens, 40).unwrap();
    assert_eq!(token_amount(&ctx.svm, &tokens.vault), 40);
    assert_eq!(token_amount(&ctx.svm, &tokens.source), 60);
}

#[test]
fn deposit_rejects_vault_not_owned_by_counter() {
    let (mut ctx, counter, mut tokens) = with_vault();
    // The authority's own ATA, not the counter PDA's.
    tokens.vault = tokens.source;
    let err = deposit(&mut ctx, counter, &tokens, 40).unwrap_err();
    assert_eq!(err, custom(2));
    assert_eq!(token_amount(&ctx.svm, &tokens.source), 100);
}

#[test]
fn deposit_rejects_zero_amount() {
    let (mut ctx, counter, tokens) = with_vault();
    let err = deposit(&mut ctx, counter, &tokens, 0).unwrap_err();
    assert_eq!(err, custom(3));
}

#[test]
fn withdraw_empties_and_closes_vault() {
    let (mut ctx, counter, tokens) = with_vault();
    deposit(&mut ctx, counter, &tokens, 40).unwrap();
    let authority = ctx.authority.pubkey();
    let vault_rent = lamports(&ctx.svm, &tokens.vault);
    let before = lamports(&ctx.svm, &authority);

    // Back to the authority's own ATA.
    let ix = withdraw_ix(ctx.program_id, authority, counter, tokens.vault, tokens.source);
    send(
        &mut ctx.svm,
        ix,
        &ctx.authority,
        &[&ctx.authority],
        TOKEN_CONFIG,
    )
    .unwrap();

    assert_eq!(token_amount(&ctx.svm, &tokens.source), 100);
    assert_eq!(lamports(&ctx.svm, &tokens.vault), 0, "vault closed");
    // Vault rent goes to the authority; 6,000 = 5,000 signature fee + 1,000 priority fee.
    assert_eq!(lamports(&ctx.svm, &authority), before + vault_rent - 6_000);
}

#[test]
fn withdraw_unauthorized() {
    let (mut ctx, counter, tokens) = with_vault();
    deposit(&mut ctx, counter, &tokens, 40).unwrap();
    let intruder = Keypair::new();
    let intruder_ata = CreateAssociatedTokenAccount::new(&mut ctx.svm, &ctx.authority, &tokens.mint)
        .owner(&intruder.pubkey())
        .send()
        .unwrap();

    let ix = withdraw_ix(
        ctx.program_id,
        intruder.pubkey(),
        counter,
        tokens.vault,
        intruder_ata,
    );
    // authority pays the fee, intruder signs as `authority`.
    let err = send(
        &mut ctx.svm,
        ix,
        &ctx.authority,
        &[&ctx.authority, &intruder],
        TOKEN_CONFIG,
    )
    .unwrap_err();
    assert_eq!(err, custom(0));
    assert_eq!(token_amount(&ctx.svm, &tokens.vault), 40);
}
