use super::rpc::fetch_nakama;
use super::*;

#[derive(Deserialize)]
struct NameLookup {
    name: String,
    address: String,
    public_key: String,
    signature_scheme: String,
}

#[derive(Deserialize)]
struct PublicKeyLookup {
    address: String,
    registered: bool,
    public_key: Option<String>,
    signature_scheme: Option<String>,
}

fn registered_key_available(rpc: &str, wallet: &LoadedWallet) -> Result<bool, String> {
    let address = bellycoin::crypto::address_to_string(&wallet.address());
    let lookup: PublicKeyLookup = http_get_json(rpc, &format!("/public-key/{address}"))?;
    if lookup.address != address {
        return Err("node returned a different public-key address".into());
    }
    if !lookup.registered {
        return Ok(false);
    }
    let scheme = lookup
        .signature_scheme
        .as_deref()
        .ok_or("node omitted registered key scheme")?
        .parse::<Signature>()
        .map_err(|_| "node returned invalid registered key scheme")?;
    let key = hex::decode(
        lookup
            .public_key
            .as_deref()
            .ok_or("node omitted registered public key")?,
    )
    .map_err(|_| "node returned invalid registered public key")?;
    if scheme != wallet.0.public_key.scheme() || key != wallet.0.public_key.bytes {
        return Err("node registered public key does not match this wallet".into());
    }
    Ok(true)
}

pub(super) fn recipient_address(rpc: &str, value: &str) -> Result<Address, String> {
    if let Ok(address) = address_from_string(value) {
        return Ok(address);
    }
    let name = bellycoin::ledger::nakama::NakamaName::new(value)
        .map_err(|error| format!("invalid recipient name: {error:?}"))?;
    let lookup: NameLookup = http_get_json(rpc, &format!("/name/{}", name.as_str()))?;
    if lookup.name != name.as_str() {
        return Err("node returned a different name".into());
    }
    let scheme = lookup
        .signature_scheme
        .parse::<Signature>()
        .map_err(|_| "node returned an invalid name signature scheme")?;
    let key = bellycoin::crypto::PublicKey {
        nakama: scheme,
        bytes: hex::decode(&lookup.public_key)
            .map_err(|_| "node returned an invalid name public key")?,
    };
    if !key.is_valid_encoding() {
        return Err("node returned an invalid name public key".into());
    }
    let address = address_from_string(&lookup.address)
        .map_err(|error| format!("node returned invalid name address: {error}"))?;
    if bellycoin::crypto::address_from_public_key(&key) != address {
        return Err("node returned a name address that does not match its public key".into());
    }
    Ok(address)
}

pub(super) fn register_name(args: &[String]) -> Result<(), String> {
    reject_manual_fee(args)?;
    let raw = option(args, "--name").ok_or("missing --name")?;
    let name = bellycoin::ledger::nakama::NakamaName::new(raw)
        .map_err(|error| format!("invalid name: {error:?}"))?;
    let periods = option(args, "--years")
        .unwrap_or("1")
        .parse::<u16>()
        .map_err(|_| "--years must be an integer from 1 to 100")?;
    if !(1..=bellycoin::ledger::nakama::MAX_NAME_PERIODS).contains(&periods) {
        return Err("--years must be an integer from 1 to 100".into());
    }
    let wallet = load_wallet(option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH))?;
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);
    let registration = wallet.0.sign_name_registration(name, periods)?;
    let burn = bellycoin::ledger::nakama::nakama_registration_burn(&registration)
        .map_err(|error| format!("invalid name duration: {error:?}"))?
        .as_pearl();

    let transaction = automatic_fee_transaction(|fee| {
        let required = burn
            .checked_add(fee)
            .and_then(|amount| amount.checked_add(1))
            .ok_or("registration cost overflow")?;
        let (inputs, change) = select_nakama_inputs(rpc, &wallet, required)?;
        let mut outputs = vec![Output::new(wallet.address(), Pearl::from_pearl(change + 1))];
        if fee > 0 {
            outputs.push(Output::bounty_hunter(Pearl::from_pearl(fee)));
        }
        let intent = SpendIntent {
            sender: wallet.address(),
            inputs: inputs.into_iter().map(Input::new).collect(),
            outputs,
            message: None,
        };
        let mut transaction = wallet
            .0
            .sign_nakama_intent_with_registered_key(intent, true)?;
        transaction.registration = Some(registration.clone());
        Ok(transaction)
    })?;
    submit_or_print_transaction(args, &transaction, &wallet.0.public_key)
}

pub(super) fn sign_spend(args: &[String]) -> Result<(), String> {
    reject_manual_fee(args)?;

    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);

    let recipient = option(args, "--to")
        .map(|value| recipient_address(rpc, value))
        .transpose()?;
    let recipient = recipient.ok_or("missing --to")?;

    let amount = parse_amount(option(args, "--amount").ok_or("missing --amount")?)?;
    let message = option(args, "--message").map(str::to_string);

    let inputs = repeated_options(args, "--input")
        .into_iter()
        .map(UtxoId::from_str)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "invalid --input UTXO ID; expected 32 hexadecimal characters".to_string())?;

    let wallet = load_wallet(path)?;
    let registered_key = registered_key_available(rpc, &wallet)?;

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
                .ok_or("explicit change is smaller than the BountyHunter fee")?;
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
            outputs.push(Output::bounty_hunter(Pearl::from_pearl(fee)));
        }

        let intent = SpendIntent {
            sender: wallet.address(),
            inputs: selected.into_iter().map(Input::new).collect(),
            outputs,
            message: message.clone(),
        };
        intent
            .validate_structure()
            .map_err(|error| error.to_string())?;
        wallet
            .0
            .sign_nakama_intent_with_registered_key(intent, registered_key)
    })?;

    submit_or_print_transaction(args, &transaction, &wallet.0.public_key)
}

pub(super) fn consolidate_coin_utxos(args: &[String]) -> Result<(), String> {
    reject_manual_fee(args)?;

    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);
    let wallet = load_wallet(path)?;
    let registered_key = registered_key_available(rpc, &wallet)?;

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
            UtxoId::from_str(&utxo.id).map_err(|_| "node returned an invalid coin id".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let total = candidates.iter().try_fold(0_u128, |total, utxo| {
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
            outputs.push(Output::bounty_hunter(Pearl::from_pearl(fee)));
        }

        let intent = SpendIntent {
            sender: wallet.address(),
            inputs: inputs.clone().into_iter().map(Input::new).collect(),
            outputs,
            message: None,
        };
        intent
            .validate_structure()
            .map_err(|error| error.to_string())?;
        wallet
            .0
            .sign_nakama_intent_with_registered_key(intent, registered_key)
    })?;

    submit_or_print_transaction(args, &transaction, &wallet.0.public_key)
}

fn nakama_input_candidates(rpc: &str, wallet: &LoadedWallet) -> Result<Vec<NakamaUtxo>, String> {
    let address = bellycoin::crypto::address_to_string(&wallet.address());
    let response = fetch_nakama(rpc, &address)?;
    let mut candidates = response
        .utxos
        .into_iter()
        .filter(|utxo| !utxo.reserved && response.next_height >= utxo.spendable_height)
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
    required: u128,
) -> Result<(Vec<UtxoId>, u128), String> {
    let candidates = nakama_input_candidates(rpc, wallet)?;
    let mut selected = Vec::new();
    let mut total = 0_u128;
    for utxo in candidates {
        selected.push(
            UtxoId::from_str(&utxo.id)
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
            "--miner is no longer supported; BountyHunter fee is automatic at 10 pearl/byte".into(),
        );
    }
    Ok(())
}

pub(super) fn automatic_fee_transaction(
    mut build: impl FnMut(u128) -> Result<Transaction, String>,
) -> Result<Transaction, String> {
    let mut fee = 0_u128;

    for _ in 0..MAX_FEE_CONVERGENCE_ROUNDS {
        let transaction = build(fee)?;

        let size = u128::try_from(
            canonical_bytes(&transaction)
                .map_err(|error| error.to_string())?
                .len(),
        )
        .map_err(|_| "transaction size overflow")?;

        let required_fee = size
            .checked_mul(AUTOMATIC_FEE_PEARL_PER_BYTE)
            .ok_or("automatic BountyHunter fee overflow")?;

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
    public_key: &bellycoin::crypto::PublicKey,
) -> Result<(), String> {
    let chain = bellycoin::genesis::chain_context().map_err(|error| error.to_string())?;

    let authorization_valid = transaction
        .verify_authorization_with_key(chain, public_key)
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
