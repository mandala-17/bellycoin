use super::*;

pub(super) fn print_balance(args: &[String]) -> Result<(), String> {
    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);
    let bytes =
        Zeroizing::new(fs::read(path).map_err(|error| format!("failed to read {path}: {error}"))?);
    let address = bellycoin::crypto::address_to_string(&wallet_address_from_file_bytes(&bytes)?);
    let balance: BalanceResponse = http_get_json(rpc, &format!("/balance/{address}"))?;
    let status: NodeStatusResponse = http_get_json(rpc, "/status")?;
    let names = rpc::fetch_registered_names(rpc, &address)?;

    println!("Address: {address}");
    rpc::print_registered_names(&names);
    let available = balance
        .total
        .checked_sub(balance.reserved)
        .and_then(|amount| amount.checked_sub(balance.immature))
        .ok_or("locked balance exceeds total")?;
    println!("Available: {}", format_amount(available));
    println!("Reserved: {}", format_amount(balance.reserved));
    println!("Immature: {}", format_amount(balance.immature));
    println!("UTXOs: {}", balance.utxo_count);
    println!("Total Mined: {}", format_amount(status.total_mined));
    println!("Total Burned: {}", format_amount(status.total_burned));
    println!("Total Supply: {}", format_amount(status.total_supply));

    Ok(())
}
