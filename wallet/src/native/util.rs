use super::*;

pub(super) fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

pub(super) fn repeated_options<'a>(args: &'a [String], name: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
        .collect()
}

pub(super) fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|argument| argument == name)
}

pub(super) fn parse_amount(value: &str) -> Result<Pearl, String> {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > DECIMALS as usize || whole.is_empty() {
        return Err(format!("invalid bellycoin amount `{value}`"));
    }
    let whole = whole
        .parse::<u128>()
        .map_err(|_| format!("invalid bellycoin amount `{value}`"))?;
    let mut fraction_text = fraction.to_string();
    fraction_text.extend(std::iter::repeat_n('0', DECIMALS as usize - fraction.len()));
    let fraction = fraction_text
        .parse::<u128>()
        .map_err(|_| format!("invalid bellycoin amount `{value}`"))?;
    let units = whole
        .checked_mul(Pearl::PEARL_PER_BELLYCOIN)
        .and_then(|units| units.checked_add(fraction))
        .ok_or_else(|| "bellycoin amount overflow".to_string())?;
    if units == 0 {
        return Err("bellycoin amount must be positive".to_string());
    }
    Ok(Pearl::from_pearl(units))
}

pub(super) fn format_amount(units: u128) -> String {
    let whole = units / Pearl::PEARL_PER_BELLYCOIN;
    let fraction = units % Pearl::PEARL_PER_BELLYCOIN;
    let width = DECIMALS as usize;
    format!("{whole}.{fraction:0width$} bellycoin")
}

pub(super) fn print_help() {
    println!(
        "wallet [menu]\nwallet new [--wallet PATH] [--words 12|24] [--nakama nakama]\nwallet restore --mnemonic PHRASE [--wallet PATH] [--nakama ACCOUNT]\nwallet address [--wallet PATH]\nwallet balance [--wallet PATH] [--rpc ADDRESS]\nwallet history [--wallet PATH] [--rpc ADDRESS] [--limit 1..=250] [--before CURSOR]\nwallet utxos [--wallet PATH] [--rpc ADDRESS]\nwallet sign-spend [--input UTXO_ID...] --to ADDRESS|NAME --amount bellycoin [--message TEXT] [--change bellycoin --change-to ADDRESS] [--rpc ADDRESS] [--wallet PATH] [--offline]\nwallet register-name --name NAME [--years 1..=100] [--wallet PATH] [--rpc ADDRESS] [--offline]\nwallet consolidate [--wallet PATH] [--rpc ADDRESS] [--offline]\nwallet version\n\nAll signature nakamas are active from genesis. Signed transactions are submitted to node RPC automatically. Use --offline to print canonical transaction hex instead. The wallet automatically pays the BountyHunter fee of 10 pearl per canonical transaction byte under node policy; manual fee input is not supported. Consolidation merges selected bellycoin UTXOs into one self-owned output and pays the BountyHunter fee. History reports canonical address activity; UTXO tracker reads the wallet nakama endpoint and follows paginated UTXOs. Spend messages are public, signed text up to 256 UTF-8 bytes.\nRunning without a command opens the interactive menu.\nWithout --input, spend selects active bellycoin inputs and calculates change through node RPC."
    );
}
