use std::io::{self, Read, Write};

use borsh::{BorshDeserialize, BorshSerialize};
use fn_dsa::{
    DOMAIN_NONE, FN_DSA_LOGN_512, FN_DSA_LOGN_1024, HASH_ID_RAW, KeyPairGenerator,
    KeyPairGenerator512, KeyPairGenerator1024, SigningKey, SigningKey512, SigningKey1024,
    VerifyingKey, VerifyingKey512, VerifyingKey1024,
};
use rand_core_06::{CryptoRng, Error as RngError, OsRng, RngCore};
use sha3::{Digest, Sha3_256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const FALCON_512_PUBLIC_KEY_SIZE: usize = 897;
pub const FALCON_1024_PUBLIC_KEY_SIZE: usize = 1793;

pub const FALCON_512_SIGNATURE_SIZE: usize = 666;
pub const FALCON_1024_SIGNATURE_SIZE: usize = 1280;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize,
)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub enum AccountSignatureScheme {
    Falcon512 = 1,
    Falcon1024 = 2,
}

/// Compatibility alias for existing code.
pub type Signature = AccountSignatureScheme;

impl AccountSignatureScheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Falcon512 => "falcon512",
            Self::Falcon1024 => "falcon1024",
        }
    }

    pub const fn supported(self) -> bool {
        true
    }

    pub const fn logn(self) -> u32 {
        match self {
            Self::Falcon512 => FN_DSA_LOGN_512,
            Self::Falcon1024 => FN_DSA_LOGN_1024,
        }
    }

    pub const fn public_key_size(self) -> usize {
        match self {
            Self::Falcon512 => FALCON_512_PUBLIC_KEY_SIZE,
            Self::Falcon1024 => FALCON_1024_PUBLIC_KEY_SIZE,
        }
    }

    pub const fn signature_size(self) -> usize {
        match self {
            Self::Falcon512 => FALCON_512_SIGNATURE_SIZE,
            Self::Falcon1024 => FALCON_1024_SIGNATURE_SIZE,
        }
    }
}

impl std::str::FromStr for AccountSignatureScheme {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
            "falcon" | "falcon512" | "falconlevel1" | "level1" => Ok(Self::Falcon512),
            "falcon1024" | "falconlevel5" | "level5" => Ok(Self::Falcon1024),
            _ => Err("unknown signature scheme"),
        }
    }
}

impl std::fmt::Display for AccountSignatureScheme {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    pub account: AccountSignatureScheme,
    pub bytes: Vec<u8>,
}

impl PublicKey {
    pub const fn scheme(&self) -> AccountSignatureScheme {
        self.account
    }

    pub fn is_valid_encoding(&self) -> bool {
        self.bytes.len() == self.scheme().public_key_size()
            && valid_public_key(self.scheme(), &self.bytes)
    }
}

impl BorshSerialize for PublicKey {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        if !self.is_valid_encoding() {
            return Err(invalid_length(
                "public key",
                self.account.public_key_size(),
                self.bytes.len(),
            ));
        }

        self.account.serialize(writer)?;
        self.bytes.serialize(writer)
    }
}

impl BorshDeserialize for PublicKey {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let account = AccountSignatureScheme::deserialize_reader(reader)?;

        let length = u32::deserialize_reader(reader)? as usize;

        let expected = account.public_key_size();

        if length != expected {
            return Err(invalid_length("public key", expected, length));
        }

        let mut bytes = vec![0_u8; expected];

        reader.read_exact(&mut bytes)?;

        if !valid_public_key(account, &bytes) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid Falcon public key",
            ));
        }

        Ok(Self { account, bytes })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSignature {
    pub account: AccountSignatureScheme,
    pub bytes: Vec<u8>,
}

impl AccountSignature {
    pub const fn scheme(&self) -> AccountSignatureScheme {
        self.account
    }

    pub fn is_valid_encoding(&self) -> bool {
        self.bytes.len() == self.scheme().signature_size()
    }
}

impl BorshSerialize for AccountSignature {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        if !self.is_valid_encoding() {
            return Err(invalid_length(
                "signature",
                self.account.signature_size(),
                self.bytes.len(),
            ));
        }

        self.account.serialize(writer)?;
        self.bytes.serialize(writer)
    }
}

impl BorshDeserialize for AccountSignature {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let account = AccountSignatureScheme::deserialize_reader(reader)?;

        let length = u32::deserialize_reader(reader)? as usize;

        let expected = account.signature_size();

        if length != expected {
            return Err(invalid_length("signature", expected, length));
        }

        let mut bytes = vec![0_u8; expected];

        reader.read_exact(&mut bytes)?;

        Ok(Self { account, bytes })
    }
}

pub struct SigningSeed {
    account: AccountSignatureScheme,
    seed: Box<[u8; 32]>,
}

impl Drop for SigningSeed {
    fn drop(&mut self) {
        self.seed.as_mut().zeroize();
    }
}

impl ZeroizeOnDrop for SigningSeed {}

impl std::fmt::Debug for SigningSeed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SigningSeed")
            .field("scheme", &self.account)
            .field("seed", &"[REDACTED]")
            .finish()
    }
}

impl SigningSeed {
    pub fn new(account: AccountSignatureScheme, seed: Box<[u8; 32]>) -> Self {
        Self { account, seed }
    }

    pub const fn scheme(&self) -> AccountSignatureScheme {
        self.account
    }

    pub const fn account(&self) -> AccountSignatureScheme {
        self.account
    }

    pub fn public_key(&self) -> PublicKey {
        public_key_from_seed(self.account, self.seed.as_ref())
    }

    pub fn sign(&self, message: &[u8]) -> AccountSignature {
        sign_from_seed(self.account, self.seed.as_ref(), message)
    }

    pub fn dangerous_export_seed(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(*self.seed)
    }

    pub fn destroy(self) {
        drop(self);
    }
}

pub fn public_key_from_seed(account: AccountSignatureScheme, seed: &[u8; 32]) -> PublicKey {
    let (_, public) = keypair_bytes_from_seed(account, seed);

    debug_assert_eq!(public.len(), account.public_key_size(),);

    PublicKey {
        account,
        bytes: public,
    }
}

pub fn sign_from_seed(
    account: AccountSignatureScheme,
    seed: &[u8; 32],
    message: &[u8],
) -> AccountSignature {
    let (secret, _) = keypair_bytes_from_seed(account, seed);

    let mut signature = vec![0_u8; account.signature_size()];

    let result = match account {
        AccountSignatureScheme::Falcon512 => SigningKey512::decode(&secret)
            .expect("generated Falcon-512 secret key must decode")
            .sign(
                &mut OsRng,
                &DOMAIN_NONE,
                &HASH_ID_RAW,
                message,
                &mut signature,
            ),

        AccountSignatureScheme::Falcon1024 => SigningKey1024::decode(&secret)
            .expect("generated Falcon-1024 secret key must decode")
            .sign(
                &mut OsRng,
                &DOMAIN_NONE,
                &HASH_ID_RAW,
                message,
                &mut signature,
            ),
    };

    result.expect("Falcon signing failed");

    AccountSignature {
        account,
        bytes: signature,
    }
}

pub fn verify(public_key: &PublicKey, message: &[u8], signature: &AccountSignature) -> bool {
    if public_key.scheme() != signature.scheme() {
        return false;
    }

    if !public_key.is_valid_encoding() || !signature.is_valid_encoding() {
        return false;
    }

    match public_key.scheme() {
        AccountSignatureScheme::Falcon512 => {
            let Some(key) = VerifyingKey512::decode(&public_key.bytes) else {
                return false;
            };

            key.verify(&signature.bytes, &DOMAIN_NONE, &HASH_ID_RAW, message)
        }

        AccountSignatureScheme::Falcon1024 => {
            let Some(key) = VerifyingKey1024::decode(&public_key.bytes) else {
                return false;
            };

            key.verify(&signature.bytes, &DOMAIN_NONE, &HASH_ID_RAW, message)
        }
    }
}

fn valid_public_key(account: AccountSignatureScheme, bytes: &[u8]) -> bool {
    match account {
        AccountSignatureScheme::Falcon512 => VerifyingKey512::decode(bytes).is_some(),

        AccountSignatureScheme::Falcon1024 => VerifyingKey1024::decode(bytes).is_some(),
    }
}

fn keypair_bytes_from_seed(account: AccountSignatureScheme, seed: &[u8; 32]) -> (Vec<u8>, Vec<u8>) {
    let mut rng = SeedRng::new(account, *seed);

    match account {
        AccountSignatureScheme::Falcon512 => {
            let mut secret = vec![0_u8; 1345];

            let mut public = vec![0_u8; FALCON_512_PUBLIC_KEY_SIZE];

            KeyPairGenerator512::default().keygen(
                FN_DSA_LOGN_512,
                &mut rng,
                &mut secret,
                &mut public,
            );

            (secret, public)
        }

        AccountSignatureScheme::Falcon1024 => {
            let mut secret = vec![0_u8; 2369];

            let mut public = vec![0_u8; FALCON_1024_PUBLIC_KEY_SIZE];

            KeyPairGenerator1024::default().keygen(
                FN_DSA_LOGN_1024,
                &mut rng,
                &mut secret,
                &mut public,
            );

            (secret, public)
        }
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct SeedRng {
    seed: [u8; 32],

    #[zeroize(skip)]
    account: AccountSignatureScheme,

    counter: u64,
    block: [u8; 32],
    offset: usize,
}

impl SeedRng {
    fn new(account: AccountSignatureScheme, seed: [u8; 32]) -> Self {
        Self {
            seed,
            account,
            counter: 0,
            block: [0; 32],
            offset: 32,
        }
    }

    fn refill(&mut self) {
        let mut hash = Sha3_256::new();

        hash.update(b"BELLY Falcon deterministic keygen");

        hash.update(self.account.logn().to_le_bytes());

        hash.update(self.seed);

        hash.update(self.counter.to_le_bytes());

        self.block.copy_from_slice(&hash.finalize());

        self.counter = self.counter.wrapping_add(1);

        self.offset = 0;
    }
}

impl RngCore for SeedRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0_u8; 4];

        self.fill_bytes(&mut bytes);

        u32::from_le_bytes(bytes)
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0_u8; 8];

        self.fill_bytes(&mut bytes);

        u64::from_le_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut written = 0;

        while written < dest.len() {
            if self.offset == self.block.len() {
                self.refill();
            }

            let count = (dest.len() - written).min(self.block.len() - self.offset);

            dest[written..written + count]
                .copy_from_slice(&self.block[self.offset..self.offset + count]);

            written += count;
            self.offset += count;
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for SeedRng {}

fn invalid_length(kind: &str, expected: usize, actual: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid {kind} length: expected {expected}, got {actual}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MESSAGE: &[u8] = b"belly falcon signature test";

    fn schemes() -> [AccountSignatureScheme; 2] {
        [
            AccountSignatureScheme::Falcon512,
            AccountSignatureScheme::Falcon1024,
        ]
    }

    #[test]
    fn both_falcon_levels_sign_and_verify() {
        for (index, scheme) in schemes().into_iter().enumerate() {
            let seed = SigningSeed::new(scheme, Box::new([(index as u8) + 31; 32]));

            let public = seed.public_key();

            let signature = seed.sign(MESSAGE);

            assert!(public.is_valid_encoding());

            assert!(signature.is_valid_encoding());

            assert!(verify(&public, MESSAGE, &signature,));

            assert!(!verify(&public, b"tampered", &signature,));
        }
    }

    #[test]
    fn expected_sizes() {
        assert_eq!(AccountSignatureScheme::Falcon512.public_key_size(), 897,);

        assert_eq!(AccountSignatureScheme::Falcon512.signature_size(), 666,);

        assert_eq!(AccountSignatureScheme::Falcon1024.public_key_size(), 1793,);

        assert_eq!(AccountSignatureScheme::Falcon1024.signature_size(), 1280,);
    }

    #[test]
    fn deterministic_public_key() {
        for scheme in schemes() {
            let first = public_key_from_seed(scheme, &[11; 32]);

            let second = public_key_from_seed(scheme, &[11; 32]);

            assert_eq!(first, second,);
        }
    }

    #[test]
    fn cross_level_is_rejected() {
        let seed_512 = SigningSeed::new(AccountSignatureScheme::Falcon512, Box::new([11; 32]));

        let seed_1024 = SigningSeed::new(AccountSignatureScheme::Falcon1024, Box::new([22; 32]));

        let public_512 = seed_512.public_key();

        let signature_1024 = seed_1024.sign(MESSAGE);

        assert!(!verify(&public_512, MESSAGE, &signature_1024,));
    }

    #[test]
    fn borsh_round_trip() {
        for (index, scheme) in schemes().into_iter().enumerate() {
            let seed = SigningSeed::new(scheme, Box::new([(index as u8) + 41; 32]));

            let public = seed.public_key();

            let signature = seed.sign(MESSAGE);

            let public_bytes = borsh::to_vec(&public).unwrap();

            let signature_bytes = borsh::to_vec(&signature).unwrap();

            assert_eq!(public_bytes.len(), 1 + 4 + scheme.public_key_size(),);

            assert_eq!(signature_bytes.len(), 1 + 4 + scheme.signature_size(),);

            let decoded_public = PublicKey::try_from_slice(&public_bytes).unwrap();

            let decoded_signature = AccountSignature::try_from_slice(&signature_bytes).unwrap();

            assert_eq!(decoded_public, public,);

            assert_eq!(decoded_signature, signature,);

            assert!(verify(&decoded_public, MESSAGE, &decoded_signature,));
        }
    }
}
