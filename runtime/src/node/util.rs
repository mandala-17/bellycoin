use super::*;

pub(super) fn parse_hash(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 64 || value.contains(['/', '?', '#']) {
        return Err("Tx Hash must be 64 hexadecimal characters".into());
    }
    let bytes = hex::decode(value).map_err(|_| "Tx Hash is not valid hexadecimal")?;
    bytes
        .try_into()
        .map_err(|_| "Tx Hash must be 32 bytes".to_string())
}

pub(super) fn format_work(limbs: [u64; 8]) -> String {
    limbs
        .into_iter()
        .map(|limb| format!("{limb:016x}"))
        .collect()
}

pub(super) fn parse_address(value: &str) -> Result<Address, String> {
    address_from_string(value)
        .map_err(|_| "address must use canonical blc encoding with a valid checksum".to_string())
}

pub(super) fn resolve_miner(ledger: &Ledger, value: &str) -> Result<Address, String> {
    if let Ok(address) = address_from_string(value) {
        return Ok(address);
    }
    let name = bellycoin::ledger::nakama::NakamaName::new(value)
        .map_err(|error| format!("invalid miner name: {error:?}"))?;
    let public_key = ledger
        .state()
        .nakama
        .resolve(&name)
        .ok_or_else(|| format!("miner name `{value}` is not registered on this chain"))?;
    Ok(bellycoin::crypto::address_from_public_key(public_key))
}
