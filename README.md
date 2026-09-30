<p align="center">
  <img src="assets/bellycoin.png" width="160" alt="Bellycoin">
</p>

<h1 align="center">Bellycoin</h1>

# Bellycoin

**Bellycoin** is an experimental Layer 1 blockchain inspired by the adventurous spirit, freedom, exploration, and friendship found in the anime and manga **One Piece**.

The project is built in Rust and focuses on a simple UTXO-based monetary system, lightweight consensus design, and post-quantum-ready cryptography.

> Bellycoin is an independent open-source project. It is not affiliated with, endorsed by, or associated with Eiichiro Oda, Shueisha, Toei Animation, or the official One Piece franchise.

---

## Overview

Bellycoin is designed as a simple native-coin blockchain.

The project intentionally avoids unnecessary protocol complexity. Its core model is based around native **BELLY** coins represented as UTXOs.

```text
Previous UTXO
     |
     v
   Input
     |
     v
 Transaction
     |
     +----> New UTXO
     |
     +----> New UTXO
```

A UTXO is referenced using:

```rust
pub struct UtxoId(Hash16);
```

The 16-byte ID is derived from the full transaction hash and output index, or
from the block emission origin. A distinct origin-kind byte separates those
two cases. The protocol hashes these bytes with the `BELLYCOIN_UTXO_ID_V1`
domain and keeps the first 16 bytes. Its text form is 32 hexadecimal characters.

The ledger maps a UTXO reference to the actual Bellycoin value:

```text
UtxoId -> Bellycoin
```

Conceptually:

```text
Pearl      = smallest BELLY unit
Bellycoin  = spendable coin
UtxoId     = 16-byte identifier of a Bellycoin UTXO
```

---

## Design Goals

Bellycoin aims to remain:

- **Simple** — minimal protocol complexity.
- **UTXO-based** — native coins are represented as unspent transaction outputs.
- **Open** — anyone can run a node and participate.
- **Deterministic** — consensus-critical state transitions are reproducible.
- **Cryptographically agile** — the protocol can evolve its cryptographic primitives.
- **Post-quantum oriented** — the project explores post-quantum digital signatures.
- **Experimental** — Bellycoin is a research and development blockchain.

---

## Native Currency

The native currency is:

```text
BELLY
```

The smallest unit is called:

```text
Pearl
```

Bellycoin currently uses:

```text
1 BELLY = 100,000,000 Pearl
```

or:

```rust
pub const DECIMALS: u8 = 8;
```

Amounts are represented using unsigned integers:

```rust
pub struct Pearl(u64);
```

This prevents native UTXOs from containing negative amounts.

---

## UTXO Model

A Bellycoin UTXO contains an amount and an owner.

Conceptually:

```rust
pub struct Bellycoin {
    pub amount: Pearl,
    pub owner: Address,
}
```

The UTXO set maps references to coins:

```rust
BTreeMap<UtxoId, Bellycoin>
```

An input references an existing UTXO:

```rust
pub struct CInput {
    pub previous_output: UtxoId,
}
```

When a valid transaction is applied:

```text
UTXO A
  |
  | consume
  v
Bellycoin
  |
  | transaction
  v
New Bellycoin outputs
  |
  v
UTXO B, UTXO C, ...
```

Spent UTXOs are removed from the active UTXO set and new outputs become new UTXOs.

---

## Transaction Authorization

Transaction data and authorization are separated.

The transaction intent contains the economic operation:

```text
sender
inputs
outputs
```

The authorization layer contains:

```text
public key
signature
```

This allows Bellycoin to sign a canonical transaction commitment without including the signature itself inside the signed message.

Transaction commitments are domain-separated and bound to the chain context to reduce the risk of cross-network replay.

---

## Consensus

Bellycoin uses a Proof-of-Work design.

The current design targets an average block interval of approximately:

```text
60 seconds
```

Mining follows a conventional:

```text
block data
   +
nonce
   |
   v
 hash
   |
   v
difficulty target
```

model.

Difficulty is adjusted over time to keep block production near the target interval.

Bellycoin intentionally keeps its Proof-of-Work design relatively simple rather than introducing additional block-weight-based difficulty mechanisms.

---

## Block Reward

Bellycoin uses a fixed block subsidy:

```text
1,000 BELLY per block
```

The current design does not use Bitcoin-style halvings.

This means miners continue receiving the protocol-defined block subsidy as the chain grows.

---

## Cryptography

Bellycoin is designed with cryptographic agility in mind.

The project explores post-quantum signature schemes, including **Falcon**, for transaction authorization.

Cryptographic components are isolated so that signature and hashing implementations can evolve without requiring unrelated parts of the protocol to be redesigned.

> Bellycoin is experimental software. Its cryptographic implementation should not be considered production-audited.

---

## Project Structure

The workspace is organized into several Rust crates:

```text
bellycoin/
├── common/
├── crypto/
├── bellycoin/
├── runtime/
├── wallet/
├── depend/
└── Cargo.toml
```

### `common`

Shared protocol types such as:

```text
ChainContext
Height
Nonce
Nakama
```

### `crypto`

Cryptographic primitives including:

```text
addresses
hashing
public keys
signatures
verification
```

### `bellycoin`

Consensus-critical blockchain logic:

```text
blockchain
consensus
genesis
ledger
transactions
UTXO state
emission
```

### `runtime`

Node runtime and networking components.

### `wallet`

Wallet functionality for creating keys, checking balances, constructing transactions, and interacting with a Bellycoin node.

### `depend`

Vendored dependencies used by the workspace.

---

## Requirements

Bellycoin currently targets Rust edition 2024.

Recommended environment:

```text
Rust 1.90+
Cargo
Git
Linux
```

Check your Rust installation:

```bash
rustc --version
cargo --version
```

Install Rust using the official Rust toolchain if it is not already installed.

---

## Clone

```bash
git clone https://github.com/mandala-17/bellycoin.git
cd bellycoin
```

---

## Build

Build the complete workspace:

```bash
cargo build
```

For an optimized build:

```bash
cargo build --release
```

Build individual components:

```bash
cargo build -p bellycoin
cargo build -p crypto
cargo build -p wallet
cargo build -p node
```

---

## Check the Workspace

Before running the software, check that all crates compile:

```bash
cargo check
```

For a specific crate:

```bash
cargo check -p bellycoin
cargo check -p wallet
cargo check -p node
```

---

## Run the Node

After building the release version:

```bash
cargo build --release -p node
```

Run the node binary from:

```bash
./target/release/bellycoin
```

During development you can also use:

```bash
cargo run -p bellycoin
```

Node configuration and available command-line options may evolve while Bellycoin is under development.

Check available options with:

```bash
cargo run -p bellycoin -- --help
```

or:

```bash
./target/release/bellycoin --help
```

---

## Run the Wallet

Build the wallet:

```bash
cargo build --release -p wallet
```

Run:

```bash
./target/release/wallet
```

During development:

```bash
cargo run -p wallet
```

The wallet communicates with a running Bellycoin node for blockchain state, balances, transaction broadcasting, and related operations.

---

## Development

Run formatting:

```bash
cargo fmt
```

Run checks:

```bash
cargo check
```

Run tests:

```bash
cargo test
```

For all workspace crates:

```bash
cargo test --workspace
```

Useful before committing:

```bash
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

---

## Experimental Status

Bellycoin is currently under active development.

Expect:

- consensus changes,
- serialization changes,
- database resets,
- wallet format changes,
- network incompatibilities between versions,
- cryptographic changes,
- breaking API changes.

Do not treat the current network or wallet format as permanently stable.

Do not use Bellycoin to store funds you cannot afford to lose.

---

## Inspiration

The name and theme of Bellycoin are inspired by **One Piece** and its world of adventure.

The project takes inspiration from themes such as:

```text
Freedom
Adventure
Exploration
Friendship
Decentralization
```

The technical protocol itself is independently designed and implemented as an experimental blockchain project.

---

## Repository

Bellycoin is developed openly on GitHub:

```text
https://github.com/mandala-17/bellycoin
```

Contributions, testing, bug reports, protocol discussion, and code review are welcome.

---

## License

Bellycoin is released under the **MIT License**.

See the repository's `LICENSE` file for details.

---

## Disclaimer

Bellycoin is experimental blockchain software intended for research, development, and education.

The software is provided without warranty. Consensus rules, cryptography, networking, wallet behavior, and economic parameters may change during development.

**Bellycoin is not affiliated with the official One Piece franchise or its rights holders.**
