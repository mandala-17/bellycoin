use borsh::{BorshDeserialize, BorshSerialize};
use crypto::Address;

#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nakama {
    Address(Address),
    BountyHunter,
}

impl Nakama {
    pub const fn resolve(self, block_miner: Address) -> Address {
        match self {
            Self::Address(address) => address,
            Self::BountyHunter => block_miner,
        }
    }
}
