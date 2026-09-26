use std::{collections::BTreeSet, fmt, str::FromStr};

use borsh::{BorshDeserialize, BorshSerialize};
use crypto::{Address, TransactionHash};

use crate::error::IntentError;

use common::Nakama;

pub const DECIMALS: u8 = 8;

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct Pearl(u64);

impl Pearl {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);
    pub const PEARL_PER_BELLYCOIN: u64 = 10u64.pow(DECIMALS as u32);

    pub const fn from_pearl(pearl: u64) -> Self {
        Self(pearl)
    }

    pub const fn as_pearl(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(pearl) => Some(Self(pearl)),
            None => None,
        }
    }

    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(pearl) => Some(Self(pearl)),
            None => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize,
)]
pub struct UtxoRef {
    pub txid: TransactionHash,
    pub index: u32,
}

impl UtxoRef {
    pub const fn new(txid: TransactionHash, index: u32) -> Self {
        Self { txid, index }
    }
}

impl fmt::Display for UtxoRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        crypto::format("", &self.txid.as_hash(), formatter)?;
        write!(formatter, ":{}", self.index)
    }
}

impl FromStr for UtxoRef {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (hash, index) = value.rsplit_once(':').ok_or("missing output index")?;
        let txid = crypto::parse("", hash).map_err(|_| "invalid transaction hash")?;
        let index = index.parse().map_err(|_| "invalid output index")?;
        Ok(Self::new(TransactionHash(txid.into_bytes()), index))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Output {
    pub output: Nakama,
    pub amount: Pearl,
}

impl Output {
    pub const fn new(nakama: Address, amount: Pearl) -> Self {
        Self {
            output: Nakama::Address(nakama),
            amount,
        }
    }

    pub const fn block_miner(amount: Pearl) -> Self {
        Self {
            output: Nakama::BlockMiner,
            amount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Input {
    pub utxo: UtxoRef,
}

impl Input {
    pub const fn new(utxo: UtxoRef) -> Self {
        Self { utxo }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SpendIntent {
    pub sender: Address,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
}

impl SpendIntent {
    pub fn validate_structure(&self) -> Result<(), IntentError> {
        if self.inputs.is_empty() {
            return Err(IntentError::EmptyInputs);
        }

        if self.outputs.is_empty() {
            return Err(IntentError::EmptyOutputs);
        }

        let mut unique = BTreeSet::new();

        for input in &self.inputs {
            if !unique.insert(input.utxo) {
                return Err(IntentError::DuplicateInput);
            }
        }

        if self
            .outputs
            .iter()
            .any(|output| output.amount == Pearl::ZERO)
        {
            return Err(IntentError::ZeroAmount);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outpoint_text_roundtrips_and_rejects_legacy_coin_ids() {
        let outpoint = UtxoRef::new(TransactionHash([0xab; 32]), 7);
        assert_eq!(outpoint.to_string().parse::<UtxoRef>(), Ok(outpoint));
        assert!("ab".repeat(32).parse::<UtxoRef>().is_err());
        assert!(format!("{}:x", "ab".repeat(32)).parse::<UtxoRef>().is_err());
    }

    #[test]
    fn structural_validation_rejects_duplicate_outpoints() {
        let outpoint = UtxoRef::new(TransactionHash([0x11; 32]), 0);
        let intent = SpendIntent {
            sender: Address::ZERO,
            inputs: vec![Input::new(outpoint), Input::new(outpoint)],
            outputs: vec![Output::new(Address::ZERO, Pearl::ONE)],
        };
        assert_eq!(
            intent.validate_structure(),
            Err(IntentError::DuplicateInput)
        );
    }
}
