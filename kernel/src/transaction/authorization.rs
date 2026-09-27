use borsh::{BorshDeserialize, BorshSerialize};

use crypto::{
    NakamaSignature, Address, HASH_SIZE, HashDomain, PublicKey, TransactionHash,
    address_from_public_key, canonical_bytes, domain, verify,
};

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
    pub public_key: PublicKey,
    pub signature: NakamaSignature,
}

impl NakamaAuthorization {
    pub fn verify(&self, sender: Address, commitment: &AuthorizationCommitment) -> bool {
        if self.public_key.scheme() != self.signature.scheme() {
            return false;
        }

        if !self.public_key.scheme().supported() {
            return false;
        }

        if address_from_public_key(&self.public_key) != sender {
            return false;
        }

        verify(&self.public_key, commitment.as_bytes(), &self.signature)
    }
}

/// Signed Belly Coin transaction.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Transaction {
    pub intent: SpendIntent,
    pub authorization: NakamaAuthorization,
}

impl Transaction {
    pub fn verify_authorization(
        &self,
        chain: ChainContext,
    ) -> Result<bool, TransactionEncodingError> {
        let commitment = self.intent.authorization_commitment(chain)?;

        Ok(self.authorization.verify(self.intent.sender, &commitment))
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
