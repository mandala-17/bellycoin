//! Canonical ledger state and rollback journal types.

use super::nakama::{NakamaName, NakamaRegistryState};
use super::utxo::{self, Bellycoin};

use crate::transaction::{Pearl, UtxoId};

use borsh::{BorshDeserialize, BorshSerialize};
use crypto::Address;

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerState {
    pub utxos: utxo::UtxoSet,
    pub bellycoin: BellycoinRecord,
    pub nakama: NakamaRegistryState,
}

impl LedgerState {
    pub const fn utxos(&self) -> &utxo::UtxoSet {
        &self.utxos
    }
}

#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BellycoinRecord {
    pub total_mined: Pearl,
    pub total_burned: Pearl,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SpendRollbackJournal {
    pub(crate) consumed_pearls: Vec<(UtxoId, Bellycoin)>,
    pub(crate) created_pearl_ids: Vec<UtxoId>,
    pub(crate) mined: Pearl,
    pub(crate) burned: Pearl,
}

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct StateRollbackJournal {
    pub spend: Option<SpendRollbackJournal>,
    pub registered_name: Option<NakamaName>,
    pub registered_public_key: Option<Address>,
}
