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
    println!("Available: {}", format_amount(balance.total));
    println!("Reserved: {}", format_amount(balance.reserved));
    println!("UTXOs: {}", balance.utxo_count);
    println!("Total Mined: {}", format_amount(status.total_mined));

    Ok(())
}
