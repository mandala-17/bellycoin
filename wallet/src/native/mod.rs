use std::{
    fs,
    io::{self, Write},
    path::Path,
    str::FromStr,
};

use bellycoin::{
    consensus::{DECIMALS, Pearl},
    crypto::{Address, Signature, address_from_string, canonical_bytes},
    transaction::{Input, Output, SpendIntent, Transaction, UtxoId},
};
use serde::Deserialize;
use wallet::{
    NakamaWallet, generate_bip39_mnemonic, nakama_wallet_file_bytes,
    nakama_wallet_from_bip39_mnemonic, nakama_wallet_from_file_bytes,
    wallet_address_from_file_bytes,
};
use zeroize::{Zeroize, Zeroizing};

const DEFAULT_WALLET_PATH: &str = "wallet.json";
const AUTOMATIC_FEE_PEARL_PER_BYTE: u64 = 10;
const MAX_FEE_CONVERGENCE_ROUNDS: usize = 8;
const DEFAULT_HISTORY_LIMIT: usize = 50;
const MAX_HISTORY_LIMIT: usize = 250;
const HISTORY_CURSOR_HEX_LEN: usize = 34;

struct LoadedWallet(NakamaWallet);

impl LoadedWallet {
    fn address(&self) -> Address {
        self.0.address
    }
}
#[cfg(feature = "mainnet")]
const DEFAULT_RPC_ADDR: &str = "127.0.0.1:4444";

#[derive(Deserialize)]
struct NakamaResponse {
    next_height: u64,
    #[serde(rename = "utxo_snapshot")]
    _utxo_snapshot: String,
    utxos: Vec<NakamaUtxo>,
    next_utxo_offset: Option<usize>,
    next_utxo_cursor: Option<String>,
}

#[derive(Deserialize)]
struct BalanceResponse {
    total: u64,
    reserved: u64,
    utxo_count: usize,
}

#[derive(Deserialize)]
struct RegisteredNamesResponse {
    address: String,
    names: Vec<String>,
    has_more: bool,
}

#[derive(Deserialize)]
struct NodeStatusResponse {
    total_mined: u64,
}

#[derive(Deserialize)]
struct NakamaUtxo {
    id: String,
    amount: u64,
    reserved: bool,
}

#[derive(Deserialize)]
struct AddressHistoryResponse {
    address: String,
    tip_height: u64,
    emission_count: usize,
    activities: Vec<AddressActivity>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct AddressActivity {
    height: u64,
    block_hash: String,
    hash: Option<String>,
    #[serde(rename = "type")]
    activity_type: String,
    direction: String,
    amount: u64,
    #[serde(default)]
    message: Option<String>,
    size_bytes: Option<usize>,
}

#[derive(Deserialize)]
struct SubmitTransactionResponse {
    hash: String,
}

const MAX_CONSOLIDATION_INPUTS: usize = 10_000;

pub fn run(mut args: Vec<String>) -> Result<(), String> {
    let result = match args.first().map(String::as_str) {
        None | Some("menu") | Some("interactive") => interactive_menu(),
        Some("new") => create_wallet(&args[1..]),
        Some("restore") => restore_wallet(&args[1..]),
        Some("address") => print_address(&args[1..]),
        Some("balance") => print_balance(&args[1..]),
        Some("history") => print_history(&args[1..]),
        Some("utxos") | Some("utxo-tracker") => print_utxo_tracker(&args[1..]),
        Some("sign-spend") => sign_spend(&args[1..]),
        Some("register-name") => register_name(&args[1..]),
        Some("consolidate") => consolidate_coin_utxos(&args[1..]),
        Some("version") | Some("--version") | Some("-V") => {
            println!("wallet {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(command) => Err(format!("unknown command `{command}`")),
    };
    args.zeroize();
    result
}

mod balance;
mod cli;
mod history;
mod rpc;
mod transaction;
mod util;
mod utxo;
mod wallet_file;

use wallet_file::{load_wallet, write_nakama_wallet};

use cli::{create_wallet, interactive_menu, print_address, restore_wallet};
use util::{format_amount, has_flag, option, parse_amount, print_help, repeated_options};

use balance::print_balance;
use history::print_history;
use rpc::{http_get_json, http_post_bytes};
use transaction::{consolidate_coin_utxos, register_name, sign_spend};
use utxo::print_utxo_tracker;

#[cfg(test)]
use history::{parse_history_limit, validate_history_cursor};

#[cfg(test)]
mod tests;
