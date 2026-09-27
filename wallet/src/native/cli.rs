use super::history::{fetch_address_history, parse_history_limit, print_history_page};
use super::*;

pub(super) fn interactive_menu() -> Result<(), String> {
    loop {
        println!();
        println!("bellycoin Wallet");
        println!("1. Create Wallet");
        println!("2. Import Wallet");
        println!("3. Show Address");
        println!("4. Show Balance");
        println!("5. Transaction History");
        println!("6. UTXO");
        println!("7. Transfer");
        println!("8. Consolidate UTXOs");
        println!("9. Explorer");
        println!("10. Register name");
        println!("11. Exit");

        match prompt("Select")?.as_str() {
            "1" => {
                let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
                let words = prompt_default("Mnemonic words (12 or 24)", "12")?;
                let nakama = prompt_signature_nakama()?;
                let mut args = vec!["--wallet".into(), path, "--words".into(), words];
                args.extend(["--nakama".into(), nakama]);
                create_wallet(&args)?;
            }
            "2" => {
                let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
                let phrase = prompt("Mnemonic")?;
                let nakama = prompt_signature_nakama()?;
                let mut args = vec!["--wallet".into(), path, "--mnemonic".into(), phrase];
                args.extend(["--nakama".into(), nakama]);
                restore_wallet(&args)?;
            }
            "3" => {
                let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
                let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
                print_address(&["--wallet".into(), path, "--rpc".into(), rpc])?;
            }
            "4" => {
                let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
                let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
                print_balance(&["--wallet".into(), path, "--rpc".into(), rpc])?;
            }
            "5" => interactive_history()?,
            "6" => interactive_wallet_query(print_utxo_tracker)?,
            "7" => interactive_spend()?,
            "8" => interactive_wallet_query(consolidate_coin_utxos)?,
            "9" => interactive_block_explorer()?,
            "10" => {
                let name = prompt("Name")?;
                let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
                let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
                super::transaction::register_name(&[
                    "--name".into(),
                    name,
                    "--wallet".into(),
                    path,
                    "--rpc".into(),
                    rpc,
                ])?;
            }
            "11" | "exit" | "quit" => return Ok(()),
            choice => println!("Unknown selection `{choice}`"),
        }
    }
}

fn prompt_signature_nakama() -> Result<String, String> {
    loop {
        let value = prompt_default("Signature nakama (falcon512, falcon1024)", "falcon512")?;
        if value.parse::<Signature>().is_ok() {
            return Ok(value);
        }
        println!("Unknown signature nakama `{value}`");
    }
}

fn interactive_history() -> Result<(), String> {
    let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
    let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
    let limit_text = prompt_default("Entries per page", &DEFAULT_HISTORY_LIMIT.to_string())?;
    let limit = parse_history_limit(&limit_text)?;
    let bytes =
        Zeroizing::new(fs::read(&path).map_err(|error| format!("failed to read {path}: {error}"))?);
    let address = kernel::crypto::address_to_string(&wallet_address_from_file_bytes(&bytes)?);

    let mut before: Option<String> = None;
    let mut previous = Vec::<Option<String>>::new();

    loop {
        println!();
        let history = fetch_address_history(&rpc, &address, limit, before.as_deref())?;
        let next_cursor = print_history_page(history);

        println!();
        if next_cursor.is_some() {
            println!("N. Next Page");
        }
        if before.is_some() {
            println!("P. Previous Page");
        }
        println!("Q. Back");

        match prompt("Select")?.to_ascii_lowercase().as_str() {
            "n" | "next" if next_cursor.is_some() => {
                previous.push(before.clone());
                before = next_cursor;
            }
            "p" | "previous" if before.is_some() => {
                before = previous.pop().unwrap_or(None);
            }
            "q" | "back" | "quit" => return Ok(()),
            choice => println!("Unknown selection `{choice}`"),
        }
    }
}

fn interactive_wallet_query(query: fn(&[String]) -> Result<(), String>) -> Result<(), String> {
    let path = prompt_default("Wallet file", DEFAULT_WALLET_PATH)?;
    let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
    query(&["--wallet".into(), path, "--rpc".into(), rpc])
}

fn interactive_spend() -> Result<(), String> {
    let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
    let recipient = prompt("Nakama address or name")?;
    super::transaction::recipient_address(&rpc, &recipient)?;
    let mut args = vec![
        "--to".into(),
        recipient,
        "--amount".into(),
        prompt("$blc amount")?,
        "--rpc".into(),
        rpc,
    ];
    args.extend([
        "--wallet".into(),
        prompt_default("Wallet file", DEFAULT_WALLET_PATH)?,
    ]);
    sign_spend(&args)
}

fn interactive_block_explorer() -> Result<(), String> {
    let rpc = prompt_default("RPC", DEFAULT_RPC_ADDR)?;
    println!("1. Address activity");
    println!("2. Transaction by Hash");
    println!("3. Latest blocks");
    println!("4. Block by height");
    let response: serde_json::Value = match prompt("Select")?.as_str() {
        "1" => {
            let address = prompt("Address")?;
            address_from_string(&address).map_err(|_| "invalid address".to_string())?;
            http_get_json(&rpc, &format!("/explorer/address/{address}"))?
        }
        "2" => {
            let hash = prompt("Hash")?;
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Tx Hash must be 64 hexadecimal characters".into());
            }
            http_get_json(&rpc, &format!("/explorer/transaction/{hash}"))?
        }
        "3" => http_get_json(&rpc, "/blocks/latest")?,
        "4" => {
            let height = prompt("Block height")?;
            if height.parse::<u64>().is_err() {
                return Err("block height must be an unsigned integer".into());
            }
            http_get_json(&rpc, &format!("/block/{height}"))?
        }
        choice => return Err(format!("unknown explorer selection `{choice}`")),
    };
    print_human_json(&response);
    Ok(())
}

fn prompt(label: &str) -> Result<String, String> {
    print!("{label}: ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| format!("failed to read input: {error}"))?;
    Ok(value.trim().to_string())
}

fn prompt_default(label: &str, default: &str) -> Result<String, String> {
    let value = prompt(&format!("{label} [{default}]"))?;
    Ok(if value.is_empty() {
        default.to_string()
    } else {
        value
    })
}

pub(super) fn create_wallet(args: &[String]) -> Result<(), String> {
    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let words = option(args, "--words")
        .unwrap_or("12")
        .parse::<usize>()
        .map_err(|_| "--words must be 12 or 24".to_string())?;
    let mnemonic = generate_bip39_mnemonic(words)?;
    let nakama = signature_nakama_option(args)?.unwrap_or(Signature::Falcon512);
    let mut wallet = nakama_wallet_from_bip39_mnemonic(&mnemonic, nakama)?;
    wallet.mnemonic = Some(mnemonic.to_string());
    let address = wallet.address;
    write_nakama_wallet(path, &wallet)?;
    println!("signature_nakama: {nakama}");
    println!("address: {}", kernel::crypto::address_to_string(&address));
    println!("mnemonic: {}", mnemonic.as_str());
    println!("wallet: {path}");
    Ok(())
}

pub(super) fn restore_wallet(args: &[String]) -> Result<(), String> {
    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let phrase = option(args, "--mnemonic").ok_or("missing --mnemonic")?;
    let nakama = signature_nakama_option(args)?.unwrap_or(Signature::Falcon512);
    let mut wallet = nakama_wallet_from_bip39_mnemonic(phrase, nakama)?;
    wallet.mnemonic = Some(phrase.to_string());
    let address = wallet.address;
    write_nakama_wallet(path, &wallet)?;
    println!("signature_nakama: {nakama}");
    println!("address: {}", kernel::crypto::address_to_string(&address));
    println!("wallet: {path}");
    Ok(())
}

fn signature_nakama_option(args: &[String]) -> Result<Option<Signature>, String> {
    option(args, "--nakama")
        .map(|value| {
            value
                .parse::<Signature>()
                .map_err(|_| "invalid --nakama; use falcon512, or falcon1024".to_string())
        })
        .transpose()
}

pub(super) fn print_address(args: &[String]) -> Result<(), String> {
    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let bytes =
        Zeroizing::new(fs::read(path).map_err(|error| format!("failed to read {path}: {error}"))?);
    let address = wallet_address_from_file_bytes(&bytes)?;
    let address = kernel::crypto::address_to_string(&address);
    let names = option(args, "--rpc")
        .map(|rpc| rpc::fetch_registered_names(rpc, &address))
        .transpose()?;
    println!("Address: {address}");
    if let Some(names) = names.as_ref() {
        rpc::print_registered_names(names);
    }
    Ok(())
}

fn human_label(key: &str) -> String {
    key.split('_')
        .map(|part| match part {
            "id" => "ID".to_string(),
            "tx" => "TX".to_string(),
            "utxo" => "UTXO".to_string(),
            "$blc" => "$blc".to_string(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn print_human_json(value: &serde_json::Value) {
    print_human_json_value(None, value, 0);
}

fn print_human_json_value(label: Option<&str>, value: &serde_json::Value, indent: usize) {
    let padding = " ".repeat(indent);

    match value {
        serde_json::Value::Null => {
            if let Some(label) = label {
                println!("{padding}{}: -", human_label(label));
            } else {
                println!("{padding}-");
            }
        }
        serde_json::Value::Bool(value) => {
            if let Some(label) = label {
                println!("{padding}{}: {value}", human_label(label));
            } else {
                println!("{padding}{value}");
            }
        }
        serde_json::Value::Number(value) => {
            if let Some(label) = label {
                println!("{padding}{}: {value}", human_label(label));
            } else {
                println!("{padding}{value}");
            }
        }
        serde_json::Value::String(value) => {
            if let Some(label) = label {
                println!("{padding}{}: {value}", human_label(label));
            } else {
                println!("{padding}{value}");
            }
        }
        serde_json::Value::Array(values) => {
            let child_indent = if let Some(label) = label {
                println!("{padding}{}:", human_label(label));
                indent + 2
            } else {
                indent
            };

            if values.is_empty() {
                println!("{}None", " ".repeat(child_indent));
                return;
            }

            for (index, value) in values.iter().enumerate() {
                let item = format!("Item {}", index + 1);
                print_human_json_value(Some(&item), value, child_indent);
            }
        }
        serde_json::Value::Object(values) => {
            let child_indent = if let Some(label) = label {
                println!("{padding}{}:", human_label(label));
                indent + 2
            } else {
                indent
            };

            for (key, value) in values {
                print_human_json_value(Some(key), value, child_indent);
            }
        }
    }
}
