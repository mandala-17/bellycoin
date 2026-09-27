// kernel/src/native/nakama.rs

use std::collections::BTreeMap;

use borsh::{BorshDeserialize, BorshSerialize};
use crypto::{PublicKey, Signature};

pub const MIN_NAKAMA_NAME_LEN: usize = 3;
pub const MAX_NAKAMA_NAME_LEN: usize = 32;

pub type NakamaRegistry = BTreeMap<NakamaName, NakamaRecord>;

#[derive(Debug)]
pub enum NakamaError {
    InvalidNameLength,
    InvalidNameCharacter,
    NameAlreadyRegistered,
    NameNotFound,
    InvalidSignature,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BorshSerialize,
    BorshDeserialize,
)]
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

#[derive(
    Debug,
    Clone,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct RegisterNakama {
    pub name: NakamaName,
    pub public_key: PublicKey,
    pub signature: Signature,
}

#[derive(
    Debug,
    Clone,
    BorshSerialize,
    BorshDeserialize,
)]
pub struct NakamaRecord {
    pub name: NakamaName,
    pub public_key: PublicKey,
}

impl NakamaRegistryState {
    pub fn register(
        &mut self,
        registration: RegisterNakama,
    ) -> Result<(), NakamaError> {
        if self.records.contains_key(&registration.name) {
            return Err(NakamaError::NameAlreadyRegistered);
        }

        // verify registration.signature using registration.public_key

        self.records.insert(
            registration.name.clone(),
            NakamaRecord {
                name: registration.name,
                public_key: registration.public_key,
            },
        );

        Ok(())
    }
}