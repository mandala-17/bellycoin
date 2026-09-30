use borsh::{BorshDeserialize, BorshSerialize};
use crypto::Address;

#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nakama {
    Address(Address),
    BountyHunter,
}

impl Nakama {
    pub const fn resolve(self, bounty_hunter: Address) -> Address {
        match self {
            Self::Address(address) => address,
            Self::BountyHunter => bounty_hunter,
        }
    }
}
