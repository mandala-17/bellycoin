use super::*;
use super::{config::*, mempool::*, state::*, util::*};

pub(super) const DEFAULT_ADDRESS_ACTIVITY_LIMIT: usize = 50;
pub(super) const MAX_ADDRESS_ACTIVITY_LIMIT: usize = 250;

pub(super) fn print_nakama(path: Option<&str>, address: &str) -> Result<(), String> {
    let database = database_path(path);
    let ledger = load_or_initialize(&database)?;
    let address = parse_address(address)?;
    let response = nakama_response(&ledger, &read_mempool(&database)?, address, 0, None)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).map_err(|error| error.to_string())?
    );
    Ok(())
}

pub(super) fn nakama_response(
    ledger: &Ledger,
    mempool: &[Transaction],
    address: Address,
    utxo_offset: usize,
    utxo_after: Option<UtxoRef>,
) -> Result<serde_json::Value, String> {
    let next_height = ledger
        .tip_height()
        .map_or(0, |height| height.0.saturating_add(1));
    let reserved = reserved_coin_inputs(mempool);
    let mut total = Pearl::from_pearl(0);
    let mut nakama_utxos = ledger
        .state()
        .utxos
        .pearls()
        .filter(|(_, coin)| coin.owner == address)
        .collect::<Vec<_>>();
    nakama_utxos.sort_by_key(|(id, _)| *id);
    for (_, coin) in &nakama_utxos {
        total = total
            .checked_add(coin.amount)
            .ok_or("nakama balance overflow")?;
    }
    let page_start = utxo_after.map_or(utxo_offset, |cursor| {
        nakama_utxos.partition_point(|(id, _)| *id <= cursor)
    });
    let utxos = nakama_utxos
        .iter()
        .skip(page_start)
        .take(MAX_ACCOUNT_UTXOS_PER_PAGE)
        .map(|(id, coin)| {
            let is_reserved = reserved.contains(id);
            serde_json::json!({
                "id": id.to_string(),
                "amount": coin.amount.as_pearl(),
                "reserved": is_reserved,
            })
        })
        .collect::<Vec<_>>();
    let next_utxo_offset = page_start
        .checked_add(utxos.len())
        .filter(|offset| *offset < nakama_utxos.len());
    let next_utxo_cursor = next_utxo_offset
        .and_then(|_| nakama_utxos.get(page_start + utxos.len().saturating_sub(1)))
        .map(|(id, _)| id.to_string());
    let utxo_snapshot_entries = nakama_utxos
        .iter()
        .map(|(id, coin)| (*id, coin.amount, reserved.contains(id)))
        .collect::<Vec<_>>();
    let utxo_snapshot_bytes = kernel::crypto::canonical_bytes(&(address, utxo_snapshot_entries))
        .map_err(|error| format!("encode nakama UTXO snapshot: {error}"))?;
    let utxo_snapshot = kernel::crypto::domain_hash(
        kernel::crypto::HashDomain::NakamaState,
        &utxo_snapshot_bytes,
    );
    Ok(serde_json::json!({
        "address": kernel::crypto::address_to_string(&address),
        "utxo_snapshot": hex::encode(utxo_snapshot.0),
        "tip_height": ledger.tip_height().map_or(0, |height| height.0),
        "next_height": next_height,
        "total": total.as_pearl(),
        "utxos": utxos,
        "next_utxo_offset": next_utxo_offset,
        "next_utxo_cursor": next_utxo_cursor,
    }))
}

pub(super) fn balance_response(
    ledger: &Ledger,
    mempool: &[Transaction],
    address: Address,
) -> Result<serde_json::Value, String> {
    let reserved_ids = reserved_coin_inputs(mempool);
    let mut total = Pearl::from_pearl(0);
    let mut reserved = Pearl::from_pearl(0);
    let mut utxo_count = 0_usize;
    for utxo in ledger
        .state()
        .utxos
        .pearls()
        .filter(|(_, coin)| coin.owner == address)
    {
        total = total
            .checked_add(utxo.1.amount)
            .ok_or("nakama balance overflow")?;
        if reserved_ids.contains(&utxo.0) {
            reserved = reserved
                .checked_add(utxo.1.amount)
                .ok_or("reserved nakama balance overflow")?;
        }
        utxo_count = utxo_count
            .checked_add(1)
            .ok_or("nakama UTXO count overflow")?;
    }
    let available = total
        .checked_sub(reserved)
        .ok_or("reserved nakama balance exceeds total")?;
    Ok(serde_json::json!({
        "address": kernel::crypto::address_to_string(&address),
        "tip_height": ledger.tip_height().map_or(0, |height| height.0),
        "total": total.as_pearl(),
        "available": available.as_pearl(),
        "reserved": reserved.as_pearl(),
        "utxo_count": utxo_count,
    }))
}

pub(super) fn explorer_address_response(
    database: &Path,
    ledger: &Ledger,
    mempool: &[Transaction],
    address: Address,
    include_emissions: bool,
    limit: usize,
    before: Option<[u8; crate::storage::ADDRESS_ACTIVITY_CURSOR_SIZE]>,
) -> Result<serde_json::Value, String> {
    let reserved_ids = reserved_coin_inputs(mempool);
    let mut total = Pearl::from_pearl(0);
    let mut reserved = Pearl::from_pearl(0);
    for utxo in ledger
        .state()
        .utxos
        .pearls()
        .filter(|(_, coin)| coin.owner == address)
    {
        total = total
            .checked_add(utxo.1.amount)
            .ok_or("explorer balance overflow")?;
        if reserved_ids.contains(&utxo.0) {
            reserved = reserved
                .checked_add(utxo.1.amount)
                .ok_or("explorer reserved balance overflow")?;
        }
    }
    let page = index::address_activity_page(database, ledger, address, before, limit)?;
    let mut activities = Vec::new();
    let mut emission_count = 0usize;
    for location in &page.locations {
        match *location {
            index::ActivityLocation::Emission { height } => {
                emission_count = emission_count.saturating_add(1);
                if !include_emissions {
                    continue;
                }

                let block = ledger
                    .chain
                    .block(&height)
                    .ok_or("indexed emission block is missing from the canonical chain")?;

                let emission = block
                    .emission()
                    .filter(|emission| emission.to == address)
                    .ok_or("indexed emission does not match the canonical chain")?;

                activities.push(serde_json::json!({
                    "height": block.height().0,
                    "block_hash": hex::encode(
                        block.hash().map_err(|error| error.to_string())?.0
                    ),
                    "hash": serde_json::Value::Null,
                    "type": "emission",
                    "direction": "in",
                    "amount": emission.subsidy.as_pearl(),
                    "gross_subsidy": emission.subsidy.as_pearl(),
                    "size_bytes": serde_json::Value::Null,
                }));
            }

            index::ActivityLocation::Transaction {
                height,
                transaction_index,
            } => {
                let block = ledger
                    .chain
                    .block(&height)
                    .ok_or("indexed activity block is missing from the canonical chain")?;

                let transaction = block
                    .transactions()
                    .get(transaction_index)
                    .ok_or("indexed activity transaction is missing from its block")?;

                if let Some(activity) = address_transaction_activity(transaction, address, block)? {
                    activities.push(activity);
                }
            }
        }
    }

    let next_cursor = page.next_cursor.map(hex::encode);

    Ok(serde_json::json!({
        "address": kernel::crypto::address_to_string(&address),
        "tip_height": ledger.tip_height().map_or(0, |height| height.0),
        "balance": {
            "total": total.as_pearl(),
            "reserved": reserved.as_pearl(),
        },
        "activity_count": activities.len(),
        "emission_count": emission_count,
        "activities": activities,
        "next_cursor": next_cursor,
    }))
}

pub(super) fn address_transaction_activity(
    transaction: &Transaction,
    address: Address,
    block: &Block,
) -> Result<Option<serde_json::Value>, String> {
    let miner = block.miner_address();
    let sender = Some(transaction.intent.sender);
    let outputs = &transaction.intent.outputs;
    let extra_sent = Pearl::ZERO;
    let received = checked_output_sum(
        outputs
            .iter()
            .filter(|output| output_recipient(output, miner) == Some(address))
            .map(|output| output.amount),
    )?;
    let (direction, amount) = if sender == Some(address) {
        let external = checked_output_sum(
            outputs
                .iter()
                .filter(|output| output_recipient(output, miner) != Some(address))
                .map(|output| output.amount),
        )?
        .checked_add(extra_sent)
        .ok_or("explorer transaction amount overflow")?;
        (
            if external.as_pearl() == 0 {
                "self"
            } else {
                "out"
            },
            external,
        )
    } else if received.as_pearl() > 0 {
        ("in", received)
    } else {
        return Ok(None);
    };
    Ok(Some(serde_json::json!({
        "height": block.height().0,
        "block_hash": hex::encode(block.hash().map_err(|error| error.to_string())?.0),
        "hash": hex::encode(transaction.id().map_err(|error| error.to_string())?),
        "type": transaction_kind(transaction),
        "direction": direction,
        "amount": amount.as_pearl(),
        "size_bytes": canonical_bytes(transaction).map_err(|error| error.to_string())?.len(),
    })))
}

pub(super) fn explorer_transaction_response(
    database: &Path,
    ledger: &Ledger,
    hash: [u8; 32],
) -> Result<serde_json::Value, String> {
    let tip_height = ledger.tip_height().map_or(0, |height| height.0);

    let location = index::transaction_location(database, ledger, hash)?
        .ok_or("transaction was not found in the canonical chain")?;

    let block = ledger
        .chain
        .block(&location.height)
        .ok_or("indexed transaction block is missing from the canonical chain")?;

    let transaction = block
        .transactions()
        .get(location.transaction_index)
        .ok_or("indexed transaction position is missing from its block")?;

    if transaction.id().map_err(|error| error.to_string())? != hash {
        return Err("transaction index does not match the canonical chain".into());
    }

    Ok(serde_json::json!({
        "hash": hex::encode(hash),
        "type": transaction_kind(transaction),
        "status": "confirmed",
        "height": block.height().0,
        "block_hash": hex::encode(
            block.hash().map_err(|error| error.to_string())?.0
        ),
        "confirmations": tip_height
            .saturating_sub(block.height().0)
            .saturating_add(1),
        "size_bytes": canonical_bytes(transaction)
            .map_err(|error| error.to_string())?
            .len(),
        "transaction": transaction_response(transaction, block.miner_address()),
    }))
}

pub(super) fn transaction_response(transaction: &Transaction, miner: Address) -> serde_json::Value {
    spend_transaction_response(transaction, miner)
}

pub(super) fn spend_transaction_response(
    transaction: &Transaction,
    miner: Address,
) -> serde_json::Value {
    let intent = &transaction.intent;
    serde_json::json!({
        "type": if transaction.registration.is_some() { "name_registration" } else { "coin" },
        "registered_name": transaction.registration.as_ref().map(|registration| registration.name.as_str()),
        "signer": kernel::crypto::address_to_string(&intent.sender),
        "inputs": intent.inputs.iter().map(|input| input.utxo.to_string()).collect::<Vec<_>>(),
        "outputs": public_outputs_response(&intent.outputs, miner, Some(intent.sender)),
        "miner_fee": miner_fee_from_outputs(&intent.outputs).unwrap_or(0),
    })
}

pub(super) fn public_outputs_response(
    outputs: &[Output],
    miner: Address,
    sender: Option<Address>,
) -> Vec<serde_json::Value> {
    outputs
        .iter()
        .map(|output| {
            let (address, output_type, role) = match output.output {
                Nakama::Address(address) => (
                    Some(kernel::crypto::address_to_string(&address)),
                    "address",
                    if sender == Some(address) {
                        "change"
                    } else {
                        "recipient"
                    },
                ),
                Nakama::BountyHunter => (
                    Some(kernel::crypto::address_to_string(&miner)),
                    "miner",
                    "miner_fee",
                ),
            };
            serde_json::json!({
                "address": address,
                "amount": output.amount.as_pearl(),
                "unit": "pearl",
                "type": output_type,
                "role": role,
            })
        })
        .collect()
}

pub(super) fn output_recipient(output: &Output, miner: Address) -> Option<Address> {
    match output.output {
        Nakama::Address(address) => Some(address),
        Nakama::BountyHunter => Some(miner),
    }
}

pub(super) fn checked_output_sum(
    amounts: impl IntoIterator<Item = Pearl>,
) -> Result<Pearl, String> {
    amounts
        .into_iter()
        .try_fold(Pearl::from_pearl(0), |total, amount| {
            total
                .checked_add(amount)
                .ok_or_else(|| "explorer amount overflow".to_string())
        })
}

pub(super) fn transaction_kind(_transaction: &Transaction) -> &'static str {
    "transfer"
}

pub(super) fn status_response(
    ledger: &Ledger,
    cumulative_work: Work,
    cumulative_weight: u64,
) -> Result<serde_json::Value, String> {
    let tip_height = ledger.tip_height().ok_or("canonical genesis is missing")?;
    let tip_hash = ledger.tip_hash().ok_or("canonical genesis is missing")?;
    let next_difficulty =
        expected_next_difficulty(&ledger.chain).map_err(|error| error.to_string())?;

    Ok(serde_json::json!({
        "tip_height": tip_height.0,
        "next_height": tip_height.0.saturating_add(1),
        "tip_hash": hex::encode(tip_hash.0),
        "next_difficulty": next_difficulty,
        "cumulative_work": format_work(cumulative_work.to_be_limbs()),
        "cumulative_weight": cumulative_weight.to_string(),
        "total_mined": ledger.state().coin.total_mined.as_pearl(),
        "supply": ledger.state().coin.supply().as_pearl(),
    }))
}

pub(super) fn latest_blocks_response(ledger: &Ledger) -> Result<serde_json::Value, String> {
    let blocks = ledger
        .chain
        .blocks()
        .rev()
        .take(20)
        .map(|block| block_response(ledger, block))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::json!({ "blocks": blocks }))
}

pub(super) fn block_response(_ledger: &Ledger, block: &Block) -> Result<serde_json::Value, String> {
    let gross_subsidy = block
        .emission()
        .map_or(Pearl::from_pearl(0), |emission| emission.subsidy);
    let transaction_details = block
        .transactions()
        .iter()
        .map(|transaction| {
            Ok(serde_json::json!({
                "hash": hex::encode(
                    transaction.id().map_err(|error| error.to_string())?
                ),
                "type": transaction_kind(transaction),
                "size_bytes": canonical_bytes(transaction)
                    .map_err(|error| error.to_string())?
                    .len(),
                "transaction": transaction_response(transaction, block.miner_address()),
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let hash = transaction_details
        .iter()
        .filter_map(|transaction| transaction.get("hash").cloned())
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "height": block.height().0,
        "hash": hex::encode(block.hash().map_err(|error| error.to_string())?.0),
        "previous_hash": hex::encode(block.previous_hash().0),
        "difficulty": block.target_bits(),
        "block_weight": block.block_weight(),
        "nonce": block.header.nonce.0,
        "transactions": block.transaction_count(),
        "hash": hash,
        "transaction_details": transaction_details,
        "miner": kernel::crypto::address_to_string(&block.miner_address()),
        "subsidy": gross_subsidy.as_pearl(),
        "miner_emission": gross_subsidy.as_pearl(),
    }))
}
