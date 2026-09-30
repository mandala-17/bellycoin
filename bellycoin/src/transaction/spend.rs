use std::{collections::BTreeSet, fmt, str::FromStr};

use borsh::{BorshDeserialize, BorshSerialize};
use crypto::{Address, Hash, Hash16, HashDomain, TransactionHash, domain16};

use crate::error::IntentError;

use common::Nakama;

pub const DECIMALS: u8 = 8;
pub const MAX_SPEND_MESSAGE_BYTES: usize = 256;

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
pub struct UtxoId(Hash16);

impl UtxoId {
    pub const SIZE: usize = crypto::HASH16_SIZE;

    pub const fn from_bytes(bytes: [u8; Self::SIZE]) -> Self {
        Self(Hash16::from_bytes(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; Self::SIZE] {
        self.0.as_bytes()
    }

    pub fn transaction(txid: TransactionHash, index: u32) -> Self {
        Self::derive(0, txid.as_bytes(), index)
    }

    pub fn emission(origin: Hash) -> Self {
        Self::derive(1, origin.as_bytes(), 0)
    }

    fn derive(kind: u8, origin: &[u8; crypto::HASH_SIZE], index: u32) -> Self {
        let mut bytes = [0u8; 1 + crypto::HASH_SIZE + 4];
        bytes[0] = kind;
        bytes[1..33].copy_from_slice(origin);
        bytes[33..].copy_from_slice(&index.to_le_bytes());
        Self(domain16(HashDomain::UtxoId, &bytes))
    }
}

impl fmt::Display for UtxoId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for UtxoId {
    type Err = crypto::HashParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<Hash16>().map(Self)
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
            output: Nakama::BountyHunter,
            amount,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Input {
    pub utxo: UtxoId,
}

impl Input {
    pub const fn new(utxo: UtxoId) -> Self {
        Self { utxo }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SpendIntent {
    pub sender: Address,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub message: Option<String>,
}

impl SpendIntent {
    pub fn validate_structure(&self) -> Result<(), IntentError> {
        if self.inputs.is_empty() {
            return Err(IntentError::EmptyInputs);
        }

        if self.outputs.is_empty() {
            return Err(IntentError::EmptyOutputs);
        }

        if self.message.as_ref().is_some_and(|message| {
            message.is_empty()
                || message.len() > MAX_SPEND_MESSAGE_BYTES
                || message.chars().any(char::is_control)
        }) {
            return Err(IntentError::InvalidMessage);
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
    fn utxo_id_text_roundtrips_and_rejects_old_outpoints() {
        let outpoint = UtxoId::transaction(TransactionHash([0xab; 32]), 7);
        assert_eq!(outpoint.to_string(), "d73929e8a0adf22fa8172fb38f006a93");
        assert_eq!(outpoint.to_string().parse::<UtxoId>(), Ok(outpoint));
        assert_eq!(outpoint.to_string().len(), 32);
        assert!("ab".repeat(32).parse::<UtxoId>().is_err());
        assert!(format!("{}:x", "ab".repeat(32)).parse::<UtxoId>().is_err());
        assert_ne!(
            outpoint,
            UtxoId::transaction(TransactionHash([0xab; 32]), 8)
        );
        assert_ne!(
            outpoint,
            UtxoId::emission(TransactionHash([0xab; 32]).as_hash())
        );
        assert_eq!(borsh::to_vec(&outpoint).unwrap().len(), UtxoId::SIZE);
    }

    #[test]
    fn structural_validation_rejects_duplicate_outpoints() {
        let outpoint = UtxoId::transaction(TransactionHash([0x11; 32]), 0);
        let intent = SpendIntent {
            sender: Address::ZERO,
            inputs: vec![Input::new(outpoint), Input::new(outpoint)],
            outputs: vec![Output::new(Address::ZERO, Pearl::ONE)],
            message: None,
        };
        assert_eq!(
            intent.validate_structure(),
            Err(IntentError::DuplicateInput)
        );
    }

    #[test]
    fn spend_message_is_bounded_and_signed() {
        let input = UtxoId::transaction(TransactionHash([0x22; 32]), 0);
        let mut intent = SpendIntent {
            sender: Address::ZERO,
            inputs: vec![Input::new(input)],
            outputs: vec![Output::new(Address::ZERO, Pearl::ONE)],
            message: None,
        };
        let chain = common::ChainContext::new([1; 32]);
        let without_message = intent.authorization_commitment(chain).unwrap();
        intent.message = Some("terima kasih".into());
        assert!(intent.validate_structure().is_ok());
        assert_ne!(
            intent.authorization_commitment(chain).unwrap(),
            without_message
        );
        intent.message = Some("x".repeat(MAX_SPEND_MESSAGE_BYTES + 1));
        assert_eq!(
            intent.validate_structure(),
            Err(IntentError::InvalidMessage)
        );
        intent.message = Some("hello\nworld".into());
        assert_eq!(
            intent.validate_structure(),
            Err(IntentError::InvalidMessage)
        );
    }
}
