use borsh::{BorshDeserialize, BorshSerialize};

use crypto::{
    Address, HASH_SIZE, HashDomain, NakamaSignature, PublicKey, TransactionHash,
    address_from_public_key, canonical_bytes, domain, verify,
};

use crate::ledger::nakama::RegisterNakama;
use crate::transaction::{SpendIntent, TransactionEncodingError};
use common::ChainContext;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize,
)]
pub struct AuthorizationCommitment([u8; HASH_SIZE]);

impl AuthorizationCommitment {
    pub const fn from_bytes(bytes: [u8; HASH_SIZE]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; HASH_SIZE] {
        &self.0
    }

    pub const fn into_bytes(self) -> [u8; HASH_SIZE] {
        self.0
    }
}

impl SpendIntent {
    pub fn authorization_commitment(
        &self,
        chain: ChainContext,
    ) -> Result<AuthorizationCommitment, TransactionEncodingError> {
        let bytes = canonical_bytes(&(chain.genesis_hash, self))
            .map_err(|_| TransactionEncodingError::Encoding)?;

        Ok(AuthorizationCommitment::from_bytes(
            domain(HashDomain::SpendIntent, &bytes).into_bytes(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct NakamaAuthorization {
    pub public_key: Option<PublicKey>,
    pub signature: NakamaSignature,
}

impl NakamaAuthorization {
    pub fn verify(
        &self,
        sender: Address,
        commitment: &AuthorizationCommitment,
        public_key: &PublicKey,
    ) -> bool {
        if self
            .public_key
            .as_ref()
            .is_some_and(|embedded| embedded != public_key)
        {
            return false;
        }
        if public_key.scheme() != self.signature.scheme() {
            return false;
        }

        if !public_key.scheme().supported() {
            return false;
        }

        if address_from_public_key(public_key) != sender {
            return false;
        }

        verify(public_key, commitment.as_bytes(), &self.signature)
    }
}

/// Signed Belly Coin transaction.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Transaction {
    pub intent: SpendIntent,
    pub authorization: NakamaAuthorization,
    pub registration: Option<RegisterNakama>,
}

impl Transaction {
    /// Checks an embedded key. Compact spends require `verify_authorization_with_key`.
    pub fn verify_authorization(
        &self,
        chain: ChainContext,
    ) -> Result<bool, TransactionEncodingError> {
        let commitment = self.intent.authorization_commitment(chain)?;

        Ok(self.authorization.public_key.as_ref().is_some_and(|key| {
            self.authorization
                .verify(self.intent.sender, &commitment, key)
        }))
    }

    pub fn verify_authorization_with_key(
        &self,
        chain: ChainContext,
        public_key: &PublicKey,
    ) -> Result<bool, TransactionEncodingError> {
        let commitment = self.intent.authorization_commitment(chain)?;
        Ok(self
            .authorization
            .verify(self.intent.sender, &commitment, public_key))
    }

    pub fn transaction_id(&self) -> Result<TransactionHash, TransactionEncodingError> {
        let bytes = canonical_bytes(self).map_err(|_| TransactionEncodingError::Encoding)?;

        Ok(TransactionHash(
            domain(HashDomain::Transaction, &bytes).into_bytes(),
        ))
    }

    pub fn id(&self) -> Result<[u8; HASH_SIZE], TransactionEncodingError> {
        Ok(self.transaction_id()?.into_bytes())
    }
}
