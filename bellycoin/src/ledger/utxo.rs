use std::{collections::BTreeMap, error::Error as StdError, fmt};

use borsh::{BorshDeserialize, BorshSerialize};
use crypto::Address;

use crate::transaction::{Pearl, UtxoRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Bellycoin {
    pub amount: Pearl,
    pub owner: Address,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct UtxoSet {
    pearls: BTreeMap<UtxoRef, Bellycoin>,
}

impl UtxoSet {
    pub fn pearl(&self, outpoint: &UtxoRef) -> Option<&Bellycoin> {
        self.pearls.get(outpoint)
    }

    pub fn insert_pearl(&mut self, outpoint: UtxoRef, pearl: Bellycoin) -> Result<(), Error> {
        if self.pearls.contains_key(&outpoint) {
            return Err(Error::CoinCollision);
        }

        self.pearls.insert(outpoint, pearl);

        Ok(())
    }

    pub fn consume_pearl(&mut self, outpoint: &UtxoRef) -> Result<Bellycoin, Error> {
        self.pearls.remove(outpoint).ok_or(Error::NotFound)
    }

    pub fn pearls(&self) -> impl Iterator<Item = (UtxoRef, &Bellycoin)> + '_ {
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
    CoinCollision,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("UTXO was not found"),
            Self::CoinCollision => formatter.write_str("pearl UTXO outpoint already exists"),
        }
    }
}

impl StdError for Error {}
