# Pinocchio counter: SBPF v3 + v1 transactions, set up from scratch

Every command and file below was run on 2026-11-02, on macOS arm64 with host rustc 1.99.1.
Verified results:

- The program builds as SBPF **v3** (`0300 0000` at offset `0x30`). The `.so` is 8,936 bytes.
- 7 LiteSVM tests pass. They send **v1 transactions** to the v3 binary.
- On a local **surfpool 1.7.0** node (2026-11-03): deployed with `solana program deploy` (CLI
  4.1.2), then the client sent v1 transactions for `initialize` (9,023 CU) and `increment`
  (257 CU). The fee was 6,000 lamports: 5,000 for the signature plus the 1,000-lamport priority
  fee. `initialize` CU depends on how many bumps `find_program_address` tries (1,500 CU each),
  so it changes with the program ID and authority: the earlier Agave 4.4.0 validator run with
  program `8fA6FDYe...` used 3,023 CU.

This guide applies the lessons from `real.md` / `anchor-vault/steps.md` to a project without
Anchor. Section 9 lists the problems this setup avoids or fixes.

## 0. Versions

| Piece | Version | Why this one |
|---|---|---|
| `surfpool` | **1.7.0** | Local node. Embeds litesvm 0.18.1 / Agave 4.3.2, the same runtime as `tests/`, so it loads SBPF v3 and runs v1 transactions without upgrading the Agave CLI. 1.0.0-rc1 refuses the v3 `.so` (section 9, #14) |
| `cargo build-sbf` | 4.5.0 (ships with Agave 4.4.0) | Defaults to `--arch v3`. 4.1.0 defaulted to v0 |
| platform-tools | **v1.58** (rustc 1.95.0-dev) | Latest release. v1.57 also works |
| host rustc | 1.99.1 | Agave 4.x crates need ≥ 1.98, litesvm 0.18.1 needs ≥ 1.97.1 |
| `pinocchio` | **0.12.0** | Latest |
| `pinocchio-system` | 0.7.0 | Latest, built for pinocchio 0.12 |
| `solana-program-log` | 1.2.0 | Replaces pinocchio's old `log` module |
| `litesvm` | 0.18.1 | Latest. Loads SBPF v3 and runs v1 transactions |
| `solana-rpc-client(-api)` | 4.4.0 | Latest |
| `solana-message` / `solana-transaction` (client) | 5.1.0 / 5.1.0 | The versions rpc-client 4.4 uses. They provide `v1::Message` |

Cluster feature gates. Values come from the feature accounts on 2026-11-02:

| Feature | Account | devnet | testnet | mainnet |
|---|---|---|---|---|
| SBPF v3 (`enable_sbpf_v3_deployment_and_execution`) | `5cC3foj77CWun58pC51ebHFUWavHWKarWyR5UUik7dnC` | active | active | active |
| Transaction v1 (`enable_tx_v1`, SIMD-0385) | `txv1aq4pp281K9um3tnPgkfX8UqtFT6wcVW3hNezGLL` | active | active | active |

## 1. Install the toolchain

A v1 transaction is validated by **the node that receives it**. The Rust client builds the v1
bytes itself with the `solana-message` crate, so the installed `solana` CLI doesn't affect what
the client sends. devnet runs `solana-core` 4.4.0-beta.1 and mainnet runs 4.3.2 (`getVersion`
on 2026-11-02). That is why the anchor-vault v1 client works with CLI 4.1.2 installed.

| Task | Agave 4.1.2 (`cargo build-sbf` 4.1.0) | Agave 4.4.0 (`cargo build-sbf` 4.5.0) |
|---|---|---|
| v1 client → devnet / mainnet | ✅ (anchor-vault client) | ✅ (CLI not involved, same client binary) |
| Build SBPF v3 | ✅ only with `--arch v3`; the default is **v0** | ✅ default is **v3**; `--arch v3` still recommended |
| `cdylib` + `lib` crate type | builds with no warning, but LTO is silently off (13.9 KB `.so` vs 8.7 KB) | builds and warns that this **disables LTO** |
| `solana program deploy` of a v3 `.so` | ✅ (anchor-vault on devnet; surfpool 1.7.0 locally) | ✅ (4.4.0 local validator) |
| v1 client → `solana-test-validator` | ❌ `Transaction did not pass signature verification`; use surfpool instead (section 7) | ✅ initialize + increment + unauthorized check pass |
| `solana confirm -v <v1 sig>` | ❌ `Transaction version (1) is not supported by the requesting client` | ✅ shows `Version: 1`, `Compute Unit Limit`, `Priority Fee` |
| `solana feature status <txv1aq4pp...>` | ❌ `Unknown feature` (the full list shows an old ID `txv1hPU7...` as inactive) | ✅ `active since epoch 1140`; SBPF v3 `active since epoch 1069` |

The two `solana confirm` cells used the same anchor-vault v1 transaction on devnet
(`5dataz3H...`). The feature-status cells were run against devnet.

With surfpool as the local node, the Agave CLI is only used for `cargo build-sbf` and
`solana program deploy`, and 4.1.2 handles both (with `--arch v3`). Upgrading to 4.4.0 is still
the simplest setup, and it is required for reading v1 transactions with `solana confirm`:

```sh
# Agave 4.4.0 CLI + cargo-build-sbf 4.5.0
agave-install init 4.4.0
# (fresh machine: sh -c "$(curl -sSfL https://release.anza.xyz/v4.4.0/install)")
solana --version            # solana-cli 4.4.0
cargo build-sbf --version   # cargo-build-sbf 4.5.0

# surfpool (local node). The installer fetches the latest release; this guide used 1.7.0.
curl -sL https://run.surfpool.run/ | bash
surfpool --version          # surfpool 1.7.0

# platform-tools v1.58 is downloaded into ~/.cache/solana/ by the first build that asks for it
rustup toolchain install 1.99.1
```

## 2. Layout: one program workspace, separate test and client workspaces

```
auction/
├── rust-toolchain.toml      host rustc 1.99.1 (applies to all three dirs)
├── Cargo.toml               workspace: program only; excludes tests/ and client/
├── .gitignore
├── program/                 on-chain code: pinocchio crates only, no Agave crates
│   ├── Cargo.toml
│   └── src/{lib.rs, state.rs, error.rs, instructions.rs}
├── tests/                   own [workspace] + own Cargo.lock: LiteSVM, Agave 4.3.x
│   ├── Cargo.toml
│   └── tests/counter.rs
└── client/                  own [workspace] + own Cargo.lock: RPC client, Agave 4.4.x
    ├── Cargo.toml
    └── src/main.rs
```

Why three lockfiles instead of one:

- **pinocchio 0.12 and litesvm 0.18.1 cannot share a `Cargo.lock`.** pinocchio 0.12 →
  `solana-account-view 3.0` needs `solana-address ^2.8`, and litesvm pins `solana-address ~2.7`.
  Cargo allows only one 2.x version per lockfile, so resolution fails.
- **litesvm needs Agave 4.3.x and the newest RPC client is Agave 4.4.x.** In anchor-vault this
  forced the client down to `~4.3` (real.md issue 10). With separate lockfiles each side uses
  the versions it needs.
- **The tests and the client don't need any Rust types from the program.** They load the `.so`
  file and encode instructions by hand (1 discriminator byte plus account metas), so nothing
  links the workspaces.

## 3. Create the project

```sh
mkdir -p auction/{program/src,tests/tests,client/src}
cd auction
git init
```

### `rust-toolchain.toml`

```toml
[toolchain]
# Host rustc: builds tests/ and client/ (Agave 4.x crates need >= 1.98).
# The program itself is compiled by the platform-tools rustc, not this one.
channel = "1.99.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

### `Cargo.toml` (root)

```toml
[workspace]
members = ["program"]
# tests/ and client/ are separate workspaces with their own Cargo.lock.
exclude = ["tests", "client"]
resolver = "3"

[workspace.package]
edition = "2024"
# Must stay <= the platform-tools rustc (1.95.0-dev in v1.57/v1.58), never the host rustc.
rust-version = "1.89.0"

[profile.release]
overflow-checks = true
lto = "fat"
codegen-units = 1
```

`resolver = "3"` makes Cargo choose dependency versions compatible with `rust-version`, so the
program's dependencies stay buildable by the platform-tools compiler.

### `.gitignore`

```
target/
.surfpool/
```

### `program/Cargo.toml`

```toml
[package]
name = "auction"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[lib]
# cdylib only: `cdylib` + `lib` disables LTO (13.9 KB vs 8.7 KB). Nothing imports this crate.
crate-type = ["cdylib"]

[dependencies]
# No default features: no heap allocator (`alloc`), no `copy` derives. `cpi` gives Seed/Signer.
pinocchio = { version = "0.12", default-features = false, features = ["cpi"] }
pinocchio-system = "0.7"
solana-program-log = "1.2"

# Host builds only (rust-analyzer, `cargo check`, unit tests): `Address::find_program_address`
# uses a syscall on-chain but needs the curve25519 feature off-chain.
[target.'cfg(not(target_os = "solana"))'.dependencies]
solana-address = { version = "2.9", features = ["curve25519"] }

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(target_os, values("solana"))'] }
```

### `program/src/lib.rs`

```rust
//! Counter program.
//!
//! | Discriminator | Instruction | Accounts |
//! |---|---|---|
//! | 0 | `Initialize` | `[signer, writable] authority`, `[writable] counter PDA`, `[] system program` |
//! | 1 | `Increment`  | `[signer] authority`, `[writable] counter PDA` |
//!
//! Counter PDA seeds: `["counter", authority]`.
#![no_std]

mod error;
mod instructions;
mod state;

pub use {error::CounterError, state::COUNTER_SEED, state::Counter};

use pinocchio::{
    AccountView, Address, ProgramResult, error::ProgramError, no_allocator, nostd_panic_handler,
    program_entrypoint,
};

program_entrypoint!(process_instruction);
no_allocator!();
nostd_panic_handler!();

pub fn process_instruction(
    program_id: &Address,
    accounts: &mut [AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data {
        [instructions::INITIALIZE] => instructions::initialize(program_id, accounts),
        [instructions::INCREMENT] => instructions::increment(program_id, accounts),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
```

Why `program_entrypoint!` + `no_allocator!` + `nostd_panic_handler!` instead of `entrypoint!`:
the program never allocates, so `no_allocator!` makes any accidental allocation panic, and
`no_std` keeps the binary small. `nostd_panic_handler!` and `no_allocator!` only emit code on
the Solana target, so host builds (rust-analyzer, `cargo check`, `cargo clippy`) also work.
Don't combine `nostd_panic_handler!` with `entrypoint!` / `default_panic_handler!`: both define
the panic handler.

### `program/src/state.rs`

```rust
use pinocchio::{Address, error::ProgramError};

pub const COUNTER_SEED: &[u8] = b"counter";

/// Counter account, 48 bytes:
///
/// | Offset | Size | Field |
/// |---|---|---|
/// | 0  | 1  | discriminator (`1`) |
/// | 1  | 1  | bump |
/// | 2  | 6  | padding |
/// | 8  | 32 | authority |
/// | 40 | 8  | count (u64 little-endian) |
///
/// Every field has alignment 1, so casting account data to `&Counter` is sound at any address.
#[repr(C)]
pub struct Counter {
    pub discriminator: u8,
    pub bump: u8,
    _padding: [u8; 6],
    pub authority: Address,
    count: [u8; 8],
}

const _: () = assert!(size_of::<Counter>() == 48 && align_of::<Counter>() == 1);

impl Counter {
    pub const DISCRIMINATOR: u8 = 1;
    pub const LEN: usize = size_of::<Self>();

    pub fn count(&self) -> u64 {
        u64::from_le_bytes(self.count)
    }

    pub fn set_count(&mut self, count: u64) {
        self.count = count.to_le_bytes();
    }

    /// Checks length and discriminator. The caller must check the account owner first.
    pub fn from_bytes_mut(data: &mut [u8]) -> Result<&mut Self, ProgramError> {
        if data.len() != Self::LEN || data[0] != Self::DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        // SAFETY: length checked; alignment is 1.
        Ok(unsafe { &mut *(data.as_mut_ptr() as *mut Self) })
    }

    /// For a freshly created (zeroed) account. Checks length only.
    pub fn from_bytes_mut_uninit(data: &mut [u8]) -> Result<&mut Self, ProgramError> {
        if data.len() != Self::LEN {
            return Err(ProgramError::InvalidAccountData);
        }
        // SAFETY: length checked; alignment is 1; every bit pattern is valid.
        Ok(unsafe { &mut *(data.as_mut_ptr() as *mut Self) })
    }
}
```

The count is stored as `[u8; 8]`, not `u64`. A `u64` field would make the struct 8-byte
aligned, and account data has no alignment guarantee for that cast.

### `program/src/error.rs`

```rust
use pinocchio::error::ProgramError;

/// Returned as `ProgramError::Custom(code)`; clients see `custom program error: 0x<code>`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterError {
    /// Signer is not the counter's authority.
    Unauthorized = 0,
    /// `count` would exceed `u64::MAX`.
    Overflow = 1,
}

impl From<CounterError> for ProgramError {
    fn from(e: CounterError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
```

### `program/src/instructions.rs`

```rust
use pinocchio::{
    AccountView, Address, ProgramResult,
    cpi::{Seed, Signer},
    error::ProgramError,
};
use pinocchio_system::instructions::CreateAccount;
use solana_program_log::log;

use crate::{error::CounterError, state::COUNTER_SEED, state::Counter};

pub const INITIALIZE: u8 = 0;
pub const INCREMENT: u8 = 1;

pub fn initialize(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [authority, counter, system_program, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if system_program.address() != &pinocchio_system::ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if counter.owned_by(program_id) {
        return Err(ProgramError::AccountAlreadyInitialized);
    }

    // Canonical bump only: never trust a bump sent by the client.
    let (expected, bump) =
        Address::find_program_address(&[COUNTER_SEED, authority.address().as_ref()], program_id);
    if counter.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }

    let bump_seed = [bump];
    let seeds = [
        Seed::from(COUNTER_SEED),
        Seed::from(authority.address().as_ref()),
        Seed::from(&bump_seed),
    ];
    CreateAccount::with_minimum_balance(authority, counter, Counter::LEN as u64, program_id, None)?
        .invoke_signed(&[Signer::from(&seeds)])?;

    let mut data = counter.try_borrow_mut()?;
    let state = Counter::from_bytes_mut_uninit(&mut data)?;
    state.discriminator = Counter::DISCRIMINATOR;
    state.bump = bump;
    state.authority = Address::new_from_array(authority.address().to_bytes());
    state.set_count(0);

    log!("counter initialized");
    Ok(())
}

pub fn increment(program_id: &Address, accounts: &mut [AccountView]) -> ProgramResult {
    let [authority, counter, ..] = accounts else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };
    if !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !counter.owned_by(program_id) {
        return Err(ProgramError::InvalidAccountOwner);
    }

    let mut data = counter.try_borrow_mut()?;
    let state = Counter::from_bytes_mut(&mut data)?;
    if &state.authority != authority.address() {
        return Err(CounterError::Unauthorized.into());
    }
    let count = state.count().checked_add(1).ok_or(CounterError::Overflow)?;
    state.set_count(count);

    log!("count = {}", count);
    Ok(())
}
```

Security checks, in the order they run:

- **Signer:** `authority.is_signer()`.
- **Program IDs:** the system program address is checked before the CPI.
- **Re-initialization:** `owned_by(program_id)` rejects a counter that already exists.
- **PDA:** the address must match the canonical bump from `find_program_address`.
- **Owner before parsing:** `owned_by` runs before the account data is read.
- **Account type:** the discriminator is checked when the data is parsed.
- **Authority:** the stored authority must equal the signer.
- **Overflow:** `checked_add`.

`increment` doesn't re-derive the PDA. Only this program can write an account it owns, and
only `initialize` creates one with discriminator `1`, so the owner check plus the stored
authority is enough.

## 4. Build as SBPF v3

```sh
cargo build-sbf --tools-version v1.58 --arch v3
xxd -s 0x30 -l 4 target/deploy/auction.so   # 0300 0000 = v3
```

- Always pass `--arch v3`. `cargo build-sbf` 4.1.0 defaults to **v0**; 4.5.0 defaults to v3.
  The flag gives the same result with either version.
- `--tools-version v1.58` pins the compiler, so a CLI upgrade doesn't silently change it.
- `cargo build-sbf` creates `target/deploy/auction-keypair.json` on the first build.
  That keypair **is the program ID**. `cargo clean` deletes it, and the next deploy then gets a
  new address. Back it up outside `target/`.
- **Switching `--arch` back can leave a stale `.so`.** Observed: build v3, then v0, then v3
  again. The last build finished in 0.06 s and `target/deploy/` still held the **v0** binary,
  because `cargo build-sbf` only copies when the per-arch output is newer. Fix:
  ```sh
  rm target/deploy/auction.so      # NOT the -keypair.json
  cargo build-sbf --tools-version v1.58 --arch v3
  ```

## 5. Tests: LiteSVM + v1 transactions

### `tests/Cargo.toml`

```toml
[package]
name = "counter-tests"
version = "0.1.0"
edition = "2024"
publish = false

# Own workspace = own Cargo.lock. LiteSVM's Solana/Agave versions never touch the program.
[workspace]

[dev-dependencies]
litesvm = "0.18.1"
# Not used directly. litesvm 0.18.1 is built against Agave 4.3 and does not compile against 4.4;
# Agave crates pin each other with `=`, so `~4.3` holds the whole family at 4.3.x.
solana-program-runtime = "~4.3"
# Same versions litesvm uses, so the types match `svm.send_transaction(...)`.
solana-message = "4.2.4"
solana-transaction = "4.1.5"
solana-instruction = "3.5"
solana-address = { version = "~2.7", features = ["curve25519"] }
solana-keypair = "3.1.2"
solana-signer = "3.0.1"
solana-account = "4.3"
```

Pin `solana-fee` once, right after creating the lockfile:

```sh
cd tests
cargo generate-lockfile
cargo update -p solana-fee --precise 4.3.2
```

This is still needed with this layout. Re-checked: with `solana-fee 4.4.0` the build fails with
`SanitizedMessage: SVMStaticMessage is not satisfied` (real.md issue 12).

### `tests/tests/counter.rs`

```rust
//! Runs `../target/deploy/auction.so` in LiteSVM with v1 transactions.
//! Build it first: `cargo build-sbf --tools-version v1.58 --arch v3` from the project root.

use litesvm::LiteSVM;
use solana_account::Account;
use solana_address::Address;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::{VersionedMessage, v1};
use solana_signer::Signer;
use solana_transaction::{InstructionError, TransactionError, versioned::VersionedTransaction};

const PROGRAM_SO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../target/deploy/auction.so"
);
const SYSTEM_PROGRAM: Address = Address::new_from_array([0; 32]);
const COUNTER_LEN: usize = 48;

// Unset v1 limits are ZERO, not runtime defaults: always set the CU and loaded-data limits.
const CONFIG: v1::TransactionConfig = v1::TransactionConfig::empty()
    .with_compute_unit_limit(20_000)
    .with_loaded_accounts_data_size_limit(64 * 1024)
    .with_priority_fee(1_000);

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
```

Run:

```sh
cd tests && cargo test      # 7 passed
```

The first test asserts the version byte, so a v0 build fails immediately with
`expected an SBPF v3 build, got v0` (checked by building without `--arch`). That catches the
"tested one binary, deployed another" problem (real.md issue 6). The overflow test uses
`svm.set_account` to write `u64::MAX` directly instead of sending 2⁶⁴ transactions.

## 6. Client: v1 transactions over RPC

### `client/Cargo.toml`

```toml
[package]
name = "counter-client"
version = "0.1.0"
edition = "2024"
publish = false

# Own workspace = own Cargo.lock, so the client can use the newest Agave RPC crates
# while tests/ stays on the versions LiteSVM supports.
[workspace]

[dependencies]
solana-rpc-client = "4.4"
solana-rpc-client-api = "4.4"
# Same versions solana-rpc-client 4.4 uses (check with `cargo tree --depth 1 -p solana-rpc-client`).
solana-message = "5.1"         # v1::Message, v1::TransactionConfig, VersionedMessage::V1
solana-transaction = "5.1"     # VersionedTransaction
solana-instruction = "4.0"
solana-pubkey = { version = "4.4", features = ["curve25519"] }
solana-keypair = "4.0"
solana-signer = "4.0"
solana-commitment-config = "3.1"
anyhow = "1"
```

### `client/src/main.rs`

```rust
//! Calls the deployed counter with v1 transactions (SIMD-0385) and checks the result.
//!
//! RPC_URL      default http://127.0.0.1:8899 (local surfpool)
//! KEYPAIR      default ~/.config/solana/id.json (fee payer + counter authority)
//! PROGRAM_ID   default: pubkey of ../target/deploy/auction-keypair.json

use anyhow::{Context, Result, bail, ensure};
use solana_commitment_config::CommitmentConfig;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::{Keypair, read_keypair_file};
use solana_message::{VersionedMessage, v1};
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::{RpcTransactionConfig, UiTransactionEncoding};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

const SYSTEM_PROGRAM: Pubkey = Pubkey::new_from_array([0; 32]);
const PROGRAM_KEYPAIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../target/deploy/auction-keypair.json"
);

/// v1 resource limits live in the message header. Unset limits are ZERO, so set both:
/// - CU limit: initialize (PDA search + CreateAccount CPI) uses a few thousand CU.
/// - Loaded accounts data: must cover every loaded account, including the ProgramData account
///   (~.so size). Too small fails with `MaxLoadedAccountsDataSizeExceeded`.
/// - Priority fee: TOTAL lamports for the transaction, not micro-lamports per CU.
const CONFIG: v1::TransactionConfig = v1::TransactionConfig::empty()
    .with_compute_unit_limit(20_000)
    .with_loaded_accounts_data_size_limit(64 * 1024)
    .with_priority_fee(1_000);

fn main() -> Result<()> {
    let rpc_url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let keypair_path = std::env::var("KEYPAIR").unwrap_or_else(|_| {
        format!(
            "{}/.config/solana/id.json",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    let payer = read_keypair_file(&keypair_path)
        .map_err(|e| anyhow::anyhow!("failed to read keypair {keypair_path}: {e}"))?;
    let program_id = match std::env::var("PROGRAM_ID") {
        Ok(id) => id.parse().context("PROGRAM_ID is not a valid pubkey")?,
        Err(_) => read_keypair_file(PROGRAM_KEYPAIR)
            .map_err(|e| anyhow::anyhow!("failed to read {PROGRAM_KEYPAIR}: {e}"))?
            .pubkey(),
    };
    let rpc = RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::confirmed());
    let (counter, _) =
        Pubkey::find_program_address(&[b"counter", payer.pubkey().as_ref()], &program_id);

    println!("cluster: {}", rpc_url.split('?').next().unwrap_or_default());
    println!("program: {program_id}");
    println!("payer:   {}", payer.pubkey());
    println!("counter: {counter}\n");

    // 1. initialize, unless this payer's counter already exists.
    let before = match fetch_count(&rpc, &counter, &program_id)? {
        Some(count) => {
            println!("[initialize] skipped, counter exists (count = {count})");
            count
        }
        None => {
            let ix = Instruction::new_with_bytes(
                program_id,
                &[0],
                vec![
                    AccountMeta::new(payer.pubkey(), true),
                    AccountMeta::new(counter, false),
                    AccountMeta::new_readonly(SYSTEM_PROGRAM, false),
                ],
            );
            let sig = send_v1(&rpc, ix, &payer, &[&payer]).context("initialize failed")?;
            println!("[initialize] tx: {sig}");
            verify_v1_on_chain(&rpc, &sig)?;
            let count = fetch_count(&rpc, &counter, &program_id)?
                .context("counter missing after initialize")?;
            ensure!(count == 0, "expected count 0 after initialize, got {count}");
            count
        }
    };

    // 2. increment by the authority.
    let sig = send_v1(
        &rpc,
        increment_ix(program_id, payer.pubkey(), counter),
        &payer,
        &[&payer],
    )
    .context("increment failed")?;
    println!("[increment]  tx: {sig}");
    verify_v1_on_chain(&rpc, &sig)?;
    let after = fetch_count(&rpc, &counter, &program_id)?.context("counter missing")?;
    ensure!(
        after == before + 1,
        "expected count {}, got {after}",
        before + 1
    );
    println!("[increment]  ok: {before} -> {after}");

    // 3. a different signer must be rejected (custom error 0 = Unauthorized).
    let intruder = Keypair::new();
    let ix = increment_ix(program_id, intruder.pubkey(), counter);
    match send_v1(&rpc, ix, &payer, &[&payer, &intruder]) {
        Ok(sig) => bail!("increment by a non-authority succeeded: {sig}"),
        Err(e) => {
            let msg = format!("{e:#}");
            ensure!(
                msg.contains("custom program error: 0x0"),
                "unexpected error: {msg}"
            );
            println!("[unauthorized] ok: rejected with custom program error 0x0");
        }
    }
    let unchanged = fetch_count(&rpc, &counter, &program_id)?;
    ensure!(
        unchanged == Some(after),
        "count changed on rejected increment"
    );

    println!("\nall checks passed");
    Ok(())
}

fn increment_ix(program_id: Pubkey, authority: Pubkey, counter: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id,
        &[1],
        vec![
            AccountMeta::new_readonly(authority, true),
            AccountMeta::new(counter, false),
        ],
    )
}

/// Compile a v1 message, sign, send (with preflight simulation) and confirm.
fn send_v1(
    rpc: &RpcClient,
    ix: Instruction,
    payer: &Keypair,
    signers: &[&Keypair],
) -> Result<String> {
    let blockhash = rpc.get_latest_blockhash()?;
    let message = v1::Message::try_compile_with_config(&payer.pubkey(), &[ix], blockhash, CONFIG)?;
    let tx = VersionedTransaction::try_new(VersionedMessage::V1(message), signers)?;
    Ok(rpc.send_and_confirm_transaction(&tx)?.to_string())
}

/// Read the transaction back and check it landed as v1 with our config.
fn verify_v1_on_chain(rpc: &RpcClient, sig: &str) -> Result<()> {
    let tx = rpc.get_transaction_with_config(
        &sig.parse()?,
        RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Base64),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(1), // required to read v1 transactions
            min_context_slot: None,                     // field added in rpc-client-api 4.4
        },
    )?;
    let decoded = tx
        .transaction
        .transaction
        .decode()
        .context("cannot decode transaction")?;
    match decoded.message {
        VersionedMessage::V1(m) if m.config == CONFIG => Ok(()),
        VersionedMessage::V1(m) => bail!("v1 config on-chain differs: {:?}", m.config),
        other => bail!("expected a v1 transaction, got {other:?}"),
    }
}

/// Count at offset 40..48 of the 48-byte counter account, after checking owner and layout.
fn fetch_count(rpc: &RpcClient, counter: &Pubkey, program_id: &Pubkey) -> Result<Option<u64>> {
    let Some(account) = rpc
        .get_account_with_commitment(counter, CommitmentConfig::confirmed())?
        .value
    else {
        return Ok(None);
    };
    ensure!(
        account.owner == *program_id,
        "counter is owned by {}, not the program",
        account.owner
    );
    ensure!(
        account.data.len() == 48 && account.data[0] == 1,
        "not a counter account"
    );
    Ok(Some(u64::from_le_bytes(account.data[40..48].try_into()?)))
}
```

No RPC API key is hard-coded. Pass any endpoint through `RPC_URL` (real.md lesson 10).

## 7. Run it on surfpool

```sh
# terminal 1, project root. --offline: no mainnet datasource, empty chain on every start.
# --no-deploy: skip surfpool's runbook auto-deploy; the program is deployed explicitly below.
# On start, ~/.config/solana/id.json is airdropped 10,000 SOL, so no `solana airdrop` is needed.
surfpool start --offline --no-deploy

# terminal 2, project root
solana program deploy -u localhost target/deploy/auction.so \
  --program-id target/deploy/auction-keypair.json
cd client
cargo build                # first build only: ~300 crates, ~5 min; shows progress
cargo run -q               # -q hides compile output, so a first build looks frozen
```

Surfpool writes logs to `.surfpool/` in the directory it starts from (hence the `.gitignore`
entry). State is in memory, so restarting it is the same as `solana-test-validator --reset`.

`tests/` (LiteSVM) and surfpool are both local, but they test different things. LiteSVM runs the
`.so` inside the test process: no RPC, no deploy, and account state can be set directly (the
overflow test relies on that). Surfpool puts the same LiteSVM 0.18.1 runtime behind a JSON-RPC
server on `:8899`, so it tests what `tests/` can't: the real `solana program deploy` upload, and
the client's RPC path (blockhash, `sendTransaction`, confirmation, `getAccountInfo`). The client
then runs unchanged against devnet with only `RPC_URL` changed.

Observed output on surfpool 1.7.0 (second run shown at the end):

```
program: LmGSyCLnSD14kNmpgvsHzM2fN6dxQ8eUyoVhPtw4Ff2
[initialize] tx: WLtUR7...
[increment]  tx: vWrZsN...
[increment]  ok: 0 -> 1
[unauthorized] ok: rejected with custom program error 0x0

all checks passed
---
[initialize] skipped, counter exists (count = 1)
[increment]  ok: 1 -> 2
```

Inspect a transaction with `getTransaction` and `maxSupportedTransactionVersion: 1`. On surfpool
it returns `"version": 1`, the fee, and `computeUnitsConsumed`:

```sh
curl -s http://127.0.0.1:8899 -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getTransaction","params":["<SIG>",{"encoding":"json","maxSupportedTransactionVersion":1}]}'
```

`solana confirm -v <SIG> -u localhost` with CLI 4.1.2 fails against surfpool (`Unable to get
finalized transaction details ... Internal error`). On devnet, use CLI 4.4.0, which shows
`Version: 1`, `Compute Unit Limit: 20000` and `Priority Fee: 1000 lamports`.

## 8. Deploy to devnet

```sh
# 1. Fresh v3 build (deploy uploads whatever is in target/deploy)
rm -f target/deploy/auction.so
cargo build-sbf --tools-version v1.58 --arch v3
xxd -s 0x30 -l 4 target/deploy/auction.so   # must be 0300 0000

# 2. Feature gates (CLI feature lists can be stale, so read the feature accounts directly).
#    "value": null = not active; data starting with AQ (0x01) = activated.
for f in 5cC3foj77CWun58pC51ebHFUWavHWKarWyR5UUik7dnC txv1aq4pp281K9um3tnPgkfX8UqtFT6wcVW3hNezGLL; do
  curl -s https://api.devnet.solana.com -X POST -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccountInfo\",\"params\":[\"$f\",{\"encoding\":\"base64\"}]}"
  echo
done

# 3. Deploy (~0.07 SOL rent for a 9 KB program) and verify the deployed binary
solana airdrop 2 -u devnet
solana program deploy -u devnet target/deploy/auction.so \
  --program-id target/deploy/auction-keypair.json
solana program dump -u devnet <PROGRAM_ID> /tmp/p.so && xxd -s 0x30 -l 4 /tmp/p.so

# 4. Client
cd client && RPC_URL=https://api.devnet.solana.com cargo run -q
```

Not run against devnet in this session. Everything up to that step ran locally.

## 9. Problems this setup avoids or fixes

Rows 1–8 and 14 were hit or reproduced while building this project. Rows 9–13 carry over from
anchor-vault.

| # | Symptom | Cause | Fix |
|---|---|---|---|
| 1 | `failed to select a version for solana-address` (pinocchio 0.12 + litesvm in one package) | `solana-account-view 3.0` needs `solana-address ^2.8`; litesvm 0.18.1 pins `~2.7` | Separate `tests/` workspace (section 2) |
| 2 | `.so` is v0 (`0000 0000`) although nothing says v0 | `cargo build-sbf` 4.1.0 defaults to `--arch v0` | Always pass `--arch v3`; the test asserts the version byte |
| 3 | `--arch v3` build "Finished in 0.06s" but `target/deploy` still holds the previous arch | The per-arch output is older than the deployed copy, so it isn't copied again | `rm target/deploy/auction.so` and rebuild |
| 4 | Client against `solana-test-validator` 4.1.2: `Transaction did not pass signature verification` | 4.1.2 predates the final v1 format. Its v1 feature ID `txv1hPU7...` doesn't exist on devnet, testnet or mainnet | Run locally on surfpool 1.7.0 (section 7), or use Agave 4.4.0 |
| 5 | `solana feature status` (4.1.2) shows SIMD-0385 inactive on devnet | It checks the old ID; the live feature is `txv1aq4pp...` and is active | Query the feature account with `getAccountInfo` (section 8) |
| 6 | `missing field min_context_slot in initializer of RpcTransactionConfig` | Field added in `solana-rpc-client-api` 4.4 | `min_context_slot: None` |
| 7 | Host `cargo check` can't find `Address::find_program_address` | In `solana-address` 2.9 it is `cfg(any(target_os = "solana", feature = "curve25519"))` | Host-only target dependency in `program/Cargo.toml` |
| 8 | `.so` 13.9 KB instead of 8.7 KB; 4.5.0 warns `two crate types defined: cdylib and lib ... precludes link-time optimizations` | `crate-type = ["cdylib", "lib"]` turns LTO off (4.1.0 does it silently) | `crate-type = ["cdylib"]`; tests and client don't import the program |
| 9 | litesvm 0.18.1 fails to compile (`new_builtin`, `Arc<SlotHashes>`) | Cargo picks Agave 4.4 | `solana-program-runtime = "~4.3"` in `tests/` |
| 10 | `SanitizedMessage: SVMStaticMessage` not satisfied | `solana-fee` 4.4.0 left outside the 4.3 pin | `cargo update -p solana-fee --precise 4.3.2` in `tests/` |
| 11 | `rustc 1.95.0-dev is not supported ... requires rustc 1.99` | `rust-version` applies to the program, which platform-tools compiles | Root `rust-version = "1.89.0"`; raise only `rust-toolchain.toml` |
| 12 | v1 tx fails with `MaxLoadedAccountsDataSizeExceeded` or runs out of CU | Unset v1 limits are zero | Always set both limits; 64 KiB covers this 9 KB program |
| 13 | `expected X, found X` / two versions of a type | Test or client deps don't match litesvm or rpc-client | Copy versions from `cargo tree --depth 1 -p <litesvm or solana-rpc-client>` |
| 14 | `solana program deploy` to surfpool: `ELF error: Detected sbpf_version required by the executable which are not enabled` | surfpool 1.0.0-rc1 embeds litesvm 0.8.1 / Agave 3.0.6 and doesn't enable SBPF v3 | Reinstall surfpool (`curl -sL https://run.surfpool.run/ \| bash`); 1.7.0 deploys and runs the v3 `.so` with default features |

## 10. Pinocchio 0.12 vs older examples

Most tutorials target 0.9–0.11. In 0.12:

- `cpi` is **not** a default feature. `Seed`, `Signer`, `invoke_signed` and `instruction::*`
  need `features = ["cpi"]`. `pinocchio-system` enables it for its own use.
- The types are `AccountView` / `Address`, not `AccountInfo` / `Pubkey`.
  `process_instruction` receives `&mut [AccountView]`. Mutating methods (`try_borrow_mut`,
  `assign`, `close`) need `&mut`.
- Resizing an account is behind the `account-resize` / `unsafe-account-resize` features.
- Logging comes from `solana-program-log` (`log!("x = {}", x)`), not `pinocchio::msg!` or
  `pinocchio-log`.
- `pinocchio-system` 0.7: `CreateAccount::with_minimum_balance(from, to, space, owner, None)`
  reads `Rent` through the sysvar syscall, so no rent account is needed.
- The default features are `alloc`, `copy` and `sha2`. This project turns all of them off and
  uses `no_allocator!`.

## 11. Bumping versions later

1. `cargo info <crate>` for each dependency, checking `rust-version` against the right compiler:
   program crates against platform-tools rustc, tests and client against host rustc.
2. Program: change `program/Cargo.toml`, then `cargo update` at the root. Rebuild with
   `--arch v3`.
3. Tests: bump `litesvm`, then set `solana-message` / `solana-transaction` / `solana-address` /
   `solana-keypair` / `solana-signer` / `solana-instruction` to the versions in
   `~/.cargo/registry/src/*/litesvm-<ver>/Cargo.toml`. Remove the `~4.3` and `solana-fee` pins
   only once the new litesvm builds against current Agave.
4. Client: bump `solana-rpc-client(-api)`, then match the other crates to
   `cargo tree --depth 1 -p solana-rpc-client`.
5. Verify: `cd tests && cargo test`, then `surfpool start --offline --no-deploy`, deploy, and
   `cd client && cargo run -q`.
