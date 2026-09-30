use std::{collections::BTreeMap, error::Error as StdError, fmt};

use borsh::{BorshDeserialize, BorshSerialize};
use crypto::Address;

use crate::transaction::{Pearl, UtxoId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Bellycoin {
    pub amount: Pearl,
    pub owner: Address,
    pub spendable_height: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct UtxoSet {
    pearls: BTreeMap<UtxoId, Bellycoin>,
}

impl UtxoSet {
    pub fn pearl(&self, outpoint: &UtxoId) -> Option<&Bellycoin> {
        self.pearls.get(outpoint)
    }

    pub fn insert_pearl(&mut self, outpoint: UtxoId, pearl: Bellycoin) -> Result<(), Error> {
        if self.pearls.contains_key(&outpoint) {
            return Err(Error::BellycoinCollision);
        }

        self.pearls.insert(outpoint, pearl);

        Ok(())
    }

    pub fn consume_pearl(&mut self, outpoint: &UtxoId) -> Result<Bellycoin, Error> {
        self.pearls.remove(outpoint).ok_or(Error::NotFound)
    }

    pub fn pearls(&self) -> impl Iterator<Item = (UtxoId, &Bellycoin)> + '_ {
        self.pearls
            .iter()
            .map(|(&outpoint, pearl)| (outpoint, pearl))
    }

    pub fn len(&self) -> usize {
        self.pearls.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pearls.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotFound,
    BellycoinCollision,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("UTXO was not found"),
            Self::BellycoinCollision => formatter.write_str("pearl UTXO ID already exists"),
        }
    }
}

impl StdError for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_compact_id_is_rejected() {
        let id = UtxoId::from_bytes([7; UtxoId::SIZE]);
        let coin = Bellycoin {
            amount: Pearl::ONE,
            owner: Address::ZERO,
            spendable_height: 0,
        };
        let mut set = UtxoSet::default();
        assert_eq!(set.insert_pearl(id, coin), Ok(()));
        assert_eq!(set.insert_pearl(id, coin), Err(Error::BellycoinCollision));
        assert_eq!(set.pearl(&id), Some(&coin));
    }
}
