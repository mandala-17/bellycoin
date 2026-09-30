//! Signed name registrations.

use std::{
    collections::BTreeMap,
    io::{self, Read},
};

use borsh::{BorshDeserialize, BorshSerialize};
use common::ChainContext;
use crypto::{
    Address, HashDomain, NakamaSignature, PublicKey, address_from_public_key, canonical_bytes,
    domain, verify,
};

pub const MIN_NAKAMA_NAME_LEN: usize = 1;
pub const MAX_NAKAMA_NAME_LEN: usize = 32;

/// Mandatory name registration burn, in the smallest BELLY unit.
pub fn nakama_name_burn(name: &NakamaName) -> crate::transaction::Pearl {
    let halvings = name.as_str().len().min(8) - 1;
    crate::transaction::Pearl::from_pearl(
        1_000_000 * crate::transaction::Pearl::PEARL_PER_BELLYCOIN >> halvings,
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum NakamaError {
    InvalidNameLength,
    InvalidNameCharacter,
    NameAlreadyRegistered,
    InvalidSignature,
    InvalidPublicKey,
    Encoding,
    WrongOwner,
    ConflictingPublicKey,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize)]
pub struct NakamaName(String);

impl NakamaName {
    pub fn new(name: impl Into<String>) -> Result<Self, NakamaError> {
        let name = name.into();
        if name.len() < MIN_NAKAMA_NAME_LEN || name.len() > MAX_NAKAMA_NAME_LEN {
            return Err(NakamaError::InvalidNameLength);
        }
        if !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(NakamaError::InvalidNameCharacter);
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl BorshDeserialize for NakamaName {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let length = u32::deserialize_reader(reader)? as usize;
        if !(MIN_NAKAMA_NAME_LEN..=MAX_NAKAMA_NAME_LEN).contains(&length) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Nakama name length",
            ));
        }
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        let name = String::from_utf8(bytes).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid Nakama name encoding")
        })?;
        Self::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Nakama name"))
    }
}

#[derive(Debug, Clone, PartialEq, BorshSerialize, BorshDeserialize)]
pub struct RegisterNakama {
    pub name: NakamaName,
    pub public_key: PublicKey,
    pub signature: NakamaSignature,
}

impl Eq for RegisterNakama {}

impl RegisterNakama {
    pub fn signing_digest(&self, chain: ChainContext) -> Result<[u8; 32], NakamaError> {
        let bytes = canonical_bytes(&(chain.genesis_hash, &self.name, &self.public_key))
            .map_err(|_| NakamaError::Encoding)?;
        Ok(domain(HashDomain::NakamaState, &bytes).into_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NakamaRecord {
    pub address: Address,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NakamaRegistryState {
    records: BTreeMap<NakamaName, NakamaRecord>,
    public_keys: BTreeMap<Address, PublicKey>,
}

impl NakamaRegistryState {
    pub fn is_empty(&self) -> bool {
        self.records.is_empty() && self.public_keys.is_empty()
    }

    pub fn resolve(&self, name: &NakamaName) -> Option<&PublicKey> {
        self.records
            .get(name)
            .and_then(|record| self.public_keys.get(&record.address))
    }

    pub fn public_key(&self, address: Address) -> Option<&PublicKey> {
        self.public_keys.get(&address)
    }

    /// Returns true when a new key was stored.
    pub(crate) fn register_public_key(
        &mut self,
        address: Address,
        public_key: PublicKey,
    ) -> Result<bool, NakamaError> {
        if !public_key.is_valid_encoding() || address_from_public_key(&public_key) != address {
            return Err(NakamaError::InvalidPublicKey);
        }
        if let Some(existing) = self.public_keys.get(&address) {
            return if existing == &public_key {
                Ok(false)
            } else {
                Err(NakamaError::ConflictingPublicKey)
            };
        }
        self.public_keys.insert(address, public_key);
        Ok(true)
    }

    pub(crate) fn remove_public_key(&mut self, address: Address) {
        self.public_keys.remove(&address);
    }

    /// Returns registered names in lexical order, with a flag for additional names.
    pub fn names_for_address(&self, address: Address, limit: usize) -> (Vec<&str>, bool) {
        let mut names = self
            .records
            .iter()
            .filter(|(_, record)| record.address == address)
            .map(|(name, _)| name.as_str())
            .take(limit.saturating_add(1))
            .collect::<Vec<_>>();
        let has_more = names.len() > limit;
        names.truncate(limit);
        (names, has_more)
    }

    pub fn register(
        &mut self,
        registration: RegisterNakama,
        chain: ChainContext,
    ) -> Result<(), NakamaError> {
        self.validate_registration(&registration, chain)?;
        let address = address_from_public_key(&registration.public_key);
        self.register_public_key(address, registration.public_key)?;
        self.records
            .insert(registration.name, NakamaRecord { address });
        Ok(())
    }

    pub fn validate_registration(
        &self,
        registration: &RegisterNakama,
        chain: ChainContext,
    ) -> Result<(), NakamaError> {
        if self.records.contains_key(&registration.name) {
            return Err(NakamaError::NameAlreadyRegistered);
        }
        if !registration.public_key.is_valid_encoding() {
            return Err(NakamaError::InvalidPublicKey);
        }
        let address = address_from_public_key(&registration.public_key);
        if self
            .public_keys
            .get(&address)
            .is_some_and(|key| key != &registration.public_key)
        {
            return Err(NakamaError::ConflictingPublicKey);
        }
        if registration.public_key.scheme() != registration.signature.scheme()
            || !registration.signature.is_valid_encoding()
            || !verify(
                &registration.public_key,
                &registration.signing_digest(chain)?,
                &registration.signature,
            )
        {
            return Err(NakamaError::InvalidSignature);
        }
        Ok(())
    }

    pub(crate) fn remove(&mut self, name: &NakamaName) {
        self.records.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        consensus::validate_transaction,
        ledger::{Bellycoin, LedgerState},
        transaction::{
            Input, NakamaAuthorization, Output, Pearl, SpendIntent, Transaction, UtxoId,
        },
    };
    use crypto::{NakamaSignatureScheme, SigningSeed, TransactionHash, address_from_public_key};

    #[test]
    fn registration_requires_owner_signature_and_chain() {
        let seed = SigningSeed::new(NakamaSignatureScheme::Falcon512, Box::new([7; 32]));
        let chain = ChainContext::new([1; 32]);
        let name = NakamaName::new("alice").unwrap();
        let mut registration = RegisterNakama {
            name: name.clone(),
            public_key: seed.public_key(),
            signature: seed.sign(b"wrong message"),
        };
        let mut registry = NakamaRegistryState::default();
        assert_eq!(
            registry.register(registration.clone(), chain),
            Err(NakamaError::InvalidSignature)
        );
        registration.signature = seed.sign(&registration.signing_digest(chain).unwrap());
        assert_eq!(
            registry.register(registration.clone(), ChainContext::new([2; 32])),
            Err(NakamaError::InvalidSignature)
        );
        registry.register(registration.clone(), chain).unwrap();
        assert_eq!(registry.resolve(&name), Some(&registration.public_key));
        assert_eq!(
            registry.names_for_address(address_from_public_key(&registration.public_key), 100),
            (vec!["alice"], false)
        );
        assert_eq!(
            registry.register(registration, chain),
            Err(NakamaError::NameAlreadyRegistered)
        );
    }

    #[test]
    fn decoded_names_are_validated() {
        let invalid = borsh::to_vec(&"Alice".to_string()).unwrap();
        assert!(borsh::from_slice::<NakamaName>(&invalid).is_err());
        let oversized = (MAX_NAKAMA_NAME_LEN as u32 + 1).to_le_bytes();
        assert!(borsh::from_slice::<NakamaName>(&oversized).is_err());
    }

    #[test]
    fn registered_name_changes_state_and_rolls_back() {
        let seed = SigningSeed::new(NakamaSignatureScheme::Falcon512, Box::new([9; 32]));
        let chain = ChainContext::new([3; 32]);
        let sender = address_from_public_key(&seed.public_key());
        let input = UtxoId::transaction(TransactionHash([4; 32]), 0);
        let mut state = LedgerState::default();
        state
            .utxos
            .insert_pearl(
                input,
                Bellycoin {
                    amount: Pearl::ONE,
                    owner: sender,
                },
            )
            .unwrap();
        let original = borsh::to_vec(&state).unwrap();
        let original_root = state.application_state_root().unwrap();
        let intent = SpendIntent {
            sender,
            inputs: vec![Input::new(input)],
            outputs: vec![Output::new(sender, Pearl::ONE)],
            message: None,
        };
        let mut registration = RegisterNakama {
            name: NakamaName::new("alice").unwrap(),
            public_key: seed.public_key(),
            signature: seed.sign(b"placeholder"),
        };
        registration.signature = seed.sign(&registration.signing_digest(chain).unwrap());
        let transaction = Transaction {
            authorization: NakamaAuthorization {
                public_key: Some(seed.public_key()),
                signature: seed.sign(intent.authorization_commitment(chain).unwrap().as_bytes()),
            },
            intent,
            registration: Some(registration.clone()),
        };
        let other = SigningSeed::new(NakamaSignatureScheme::Falcon512, Box::new([10; 32]));
        let mut wrong_owner = transaction.clone();
        wrong_owner.registration.as_mut().unwrap().public_key = other.public_key();
        assert!(matches!(
            validate_transaction(wrong_owner, chain, 1, &state),
            Err(
                crate::consensus::TransactionConsensusError::InvalidRegistration(
                    NakamaError::WrongOwner
                )
            )
        ));
        let mut compact_registration = transaction;
        compact_registration.authorization.public_key = None;
        let validated =
            validate_transaction(compact_registration.clone(), chain, 1, &state).unwrap();
        let journal = state
            .apply_validated_transaction(&validated, sender)
            .unwrap();
        assert_eq!(
            state.nakama.resolve(&registration.name),
            Some(&registration.public_key)
        );
        assert_ne!(state.application_state_root().unwrap(), original_root);
        assert!(validate_transaction(compact_registration, chain, 1, &state).is_err());
        state.rollback_state(journal).unwrap();
        assert_eq!(borsh::to_vec(&state).unwrap(), original);
        assert_eq!(state.application_state_root().unwrap(), original_root);
    }

    #[test]
    fn first_spend_reveals_key_and_later_spend_uses_registry() {
        let seed = SigningSeed::new(NakamaSignatureScheme::Falcon512, Box::new([11; 32]));
        let chain = ChainContext::new([5; 32]);
        let sender = address_from_public_key(&seed.public_key());
        let input = UtxoId::transaction(TransactionHash([6; 32]), 0);
        let mut state = LedgerState::default();
        state
            .utxos
            .insert_pearl(
                input,
                Bellycoin {
                    amount: Pearl::ONE,
                    owner: sender,
                },
            )
            .unwrap();
        let original = borsh::to_vec(&state).unwrap();
        let first_intent = SpendIntent {
            sender,
            inputs: vec![Input::new(input)],
            outputs: vec![Output::new(sender, Pearl::ONE)],
            message: None,
        };
        let first = Transaction {
            authorization: NakamaAuthorization {
                public_key: Some(seed.public_key()),
                signature: seed.sign(
                    first_intent
                        .authorization_commitment(chain)
                        .unwrap()
                        .as_bytes(),
                ),
            },
            intent: first_intent,
            registration: None,
        };
        let first_validated = validate_transaction(first, chain, 1, &state).unwrap();
        let first_journal = state
            .apply_validated_transaction(&first_validated, sender)
            .unwrap();
        assert_eq!(state.nakama.public_key(sender), Some(&seed.public_key()));

        let next_input = UtxoId::transaction(first_validated.txid, 0);
        let next_intent = SpendIntent {
            sender,
            inputs: vec![Input::new(next_input)],
            outputs: vec![Output::new(sender, Pearl::ONE)],
            message: None,
        };
        let compact = Transaction {
            authorization: NakamaAuthorization {
                public_key: None,
                signature: seed.sign(
                    next_intent
                        .authorization_commitment(chain)
                        .unwrap()
                        .as_bytes(),
                ),
            },
            intent: next_intent,
            registration: None,
        };
        let compact_validated = validate_transaction(compact.clone(), chain, 2, &state).unwrap();
        let compact_journal = state
            .apply_validated_transaction(&compact_validated, sender)
            .unwrap();
        state.rollback_state(compact_journal).unwrap();
        state.rollback_state(first_journal).unwrap();
        assert_eq!(state.nakama.public_key(sender), None);
        assert_eq!(borsh::to_vec(&state).unwrap(), original);
        assert!(matches!(
            validate_transaction(compact, chain, 2, &state),
            Err(crate::consensus::TransactionConsensusError::UnknownPublicKey)
        ));
    }
}
