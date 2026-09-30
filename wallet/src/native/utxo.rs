use super::rpc::fetch_nakama;
use super::*;

pub(super) fn print_utxo_tracker(args: &[String]) -> Result<(), String> {
    let path = option(args, "--wallet").unwrap_or(DEFAULT_WALLET_PATH);
    let rpc = option(args, "--rpc").unwrap_or(DEFAULT_RPC_ADDR);
    let bytes =
        Zeroizing::new(fs::read(path).map_err(|error| format!("failed to read {path}: {error}"))?);
    let address = bellycoin::crypto::address_to_string(&wallet_address_from_file_bytes(&bytes)?);
    let nakama = fetch_nakama(rpc, &address)?;

    println!("address: {address}");
    println!("next height: {}", nakama.next_height);
    println!("utxos: {}", nakama.utxos.len());
    let mut utxos = nakama.utxos.iter().collect::<Vec<_>>();
    utxos.sort_by(|left, right| left.id.cmp(&right.id));
    for utxo in utxos {
        if nakama.next_height < utxo.spendable_height {
            println!(
                "- utxo: {}  {}  immature until block {}",
                utxo.id,
                format_amount(utxo.amount),
                utxo.spendable_height
            );
        } else {
            println!("- utxo: {}  {}", utxo.id, format_amount(utxo.amount));
        }
    }
    Ok(())
}
