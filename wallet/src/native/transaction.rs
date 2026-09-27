use super::rpc::fetch_nakama;
use super::*;

pub(super) fn sign_spend(args: &[String]) -> Result<(), String> {
    reject_manual_fee(args)?;

    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);

    let recipient = option(args, "--to")
        .map(|value| address_from_string(value).map_err(|error| error.to_string()))
        .transpose()?;
    let recipient = recipient.ok_or("missing --to")?;

    let amount = parse_amount(option(args, "--amount").ok_or("missing --amount")?)?;

    let inputs = repeated_options(args, "--input")
        .into_iter()
        .map(UtxoRef::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid --input outpoint; expected 64-hex-txid:index".to_string())?;

    let wallet = load_wallet(path)?;
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);

    let explicit_change = option(args, "--change").map(parse_amount).transpose()?;

    let change_target = option(args, "--change-to")
        .map(|address| address_from_string(address).map_err(|error| error.to_string()))
        .transpose()?;

    if inputs.is_empty() && (explicit_change.is_some() || change_target.is_some()) {
        return Err("automatic input selection also calculates change automatically".into());
    }

    let transaction = automatic_fee_transaction(|fee| {
        let required = amount
            .as_pearl()
            .checked_add(fee)
            .ok_or("transaction amount plus fee overflow")?;

        let (selected, change, change_address) = if inputs.is_empty() {
            let (selected, change) = select_nakama_inputs(rpc, &wallet, required)?;
            (selected, change, wallet.address())
        } else {
            let gross_change = explicit_change.map_or(0, Pearl::as_pearl);
            let change = gross_change
                .checked_sub(fee)
                .ok_or("explicit change is smaller than the miner fee")?;
            (
                inputs.clone(),
                change,
                change_target.unwrap_or(wallet.address()),
            )
        };

        let mut outputs = vec![Output::new(recipient, amount)];

        if change > 0 {
            outputs.push(Output::new(change_address, Pearl::from_pearl(change)));
        }

        if fee > 0 {
            outputs.push(Output::block_miner(Pearl::from_pearl(fee)));
        }

        let intent = SpendIntent {
            sender: wallet.address(),
            inputs: selected.into_iter().map(Input::new).collect(),
            outputs,
        };
        intent
            .validate_structure()
            .map_err(|error| error.to_string())?;
        wallet.sign_onchain_spend(intent)
    })?;

    submit_or_print_transaction(args, &transaction)
}

pub(super) fn consolidate_coin_utxos(args: &[String]) -> Result<(), String> {
    reject_manual_fee(args)?;

    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);
    let wallet = load_wallet(path)?;

    let mut candidates = nakama_input_candidates(rpc, &wallet)?;

    if candidates.len() < 2 {
        return Err("consolidation requires at least two available bellycoin UTXOs".into());
    }

    candidates.sort_by(|left, right| {
        left.amount
            .cmp(&right.amount)
            .then_with(|| left.id.cmp(&right.id))
    });

    candidates.truncate(MAX_CONSOLIDATION_INPUTS);

    let inputs = candidates
        .iter()
        .map(|utxo| {
            UtxoRef::from_str(&utxo.id).map_err(|_| "node returned an invalid coin id".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let total = candidates.iter().try_fold(0_u64, |total, utxo| {
        total
            .checked_add(utxo.amount)
            .ok_or_else(|| "consolidation input amount overflow".to_string())
    })?;

    let transaction = automatic_fee_transaction(|fee| {
        let consolidated = total
            .checked_sub(fee)
            .filter(|amount| *amount > 0)
            .ok_or("UTXO total is insufficient for consolidation fee")?;

        let mut outputs = vec![Output::new(
            wallet.address(),
            Pearl::from_pearl(consolidated),
        )];

        if fee > 0 {
            outputs.push(Output::block_miner(Pearl::from_pearl(fee)));
        }

        let intent = SpendIntent {
            sender: wallet.address(),
            inputs: inputs.clone().into_iter().map(Input::new).collect(),
            outputs,
        };
        intent
            .validate_structure()
            .map_err(|error| error.to_string())?;
        wallet.sign_onchain_spend(intent)
    })?;

    submit_or_print_transaction(args, &transaction)
}

fn nakama_input_candidates(rpc: &str, wallet: &LoadedWallet) -> Result<Vec<NakamaUtxo>, String> {
    let address = kernel::crypto::address_to_string(&wallet.address());
    let response = fetch_nakama(rpc, &address)?;
    let mut candidates = response
        .utxos
        .into_iter()
        .filter(|utxo| !utxo.reserved)
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .amount
            .cmp(&left.amount)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(candidates)
}

pub(super) fn select_nakama_inputs(
    rpc: &str,
    wallet: &LoadedWallet,
    required: u64,
) -> Result<(Vec<UtxoRef>, u64), String> {
    let candidates = nakama_input_candidates(rpc, wallet)?;
    let mut selected = Vec::new();
    let mut total = 0_u64;
    for utxo in candidates {
        selected.push(
            UtxoRef::from_str(&utxo.id)
                .map_err(|_| "node returned an invalid coin id".to_string())?,
        );
        total = total
            .checked_add(utxo.amount)
            .ok_or_else(|| "selected input amount overflow".to_string())?;
        if total >= required {
            return Ok((selected, total - required));
        }
    }
    Err(format!(
        "insufficient available balance for amount and fee: available {total} units"
    ))
}

pub(super) fn reject_manual_fee(args: &[String]) -> Result<(), String> {
    if option(args, "--miner").is_some() {
        return Err(
            "--miner is no longer supported; wallet fee is automatic at 10 pearl/byte".into(),
        );
    }
    Ok(())
}

pub(super) fn automatic_fee_transaction(
    mut build: impl FnMut(u64) -> Result<Transaction, String>,
) -> Result<Transaction, String> {
    let mut fee = 0_u64;

    for _ in 0..MAX_FEE_CONVERGENCE_ROUNDS {
        let transaction = build(fee)?;

        let size = u64::try_from(
            canonical_bytes(&transaction)
                .map_err(|error| error.to_string())?
                .len(),
        )
        .map_err(|_| "transaction size overflow")?;

        let required_fee = size
            .checked_mul(AUTOMATIC_FEE_PEARL_PER_BYTE)
            .ok_or("automatic miner fee overflow")?;

        if fee == required_fee {
            return Ok(transaction);
        }

        fee = required_fee;
    }

    Err("automatic transaction fee did not converge".into())
}

pub(super) fn submit_or_print_transaction(
    args: &[String],
    transaction: &Transaction,
) -> Result<(), String> {
    let chain = kernel::genesis::chain_context().map_err(|error| error.to_string())?;

    let authorization_valid = transaction
        .verify_authorization(chain)
        .map_err(|error| format!("local authorization verification failed: {error}"))?;

    println!("Local Authorization Valid: {authorization_valid}");

    if !authorization_valid {
        return Err("wallet produced an invalid transaction authorization".into());
    }

    let transaction_bytes = canonical_bytes(transaction).map_err(|error| error.to_string())?;

    if has_flag(args, "--offline") {
        println!("Transaction Hex: {}", hex::encode(&transaction_bytes));
        println!("Bytes: {}", transaction_bytes.len());
        return Ok(());
    }

    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);
    let response: SubmitTransactionResponse =
        http_post_bytes(rpc, "/transaction", &transaction_bytes)?;

    println!("Tx Hash: {}", response.hash);
    println!("Bytes: {}", transaction_bytes.len());
    Ok(())
}
