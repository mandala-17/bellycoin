use std::{collections::BTreeSet, error::Error as StdError, fmt};

use common::ChainContext;
use crypto::{Address, TransactionHash};

use crate::ledger::nakama::{NakamaError, RegisterNakama};
use crate::transaction::{Input, IntentError, Output, Pearl, SpendIntent, Transaction, UtxoRef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedTransaction {
    pub intent: SpendIntent,
    pub txid: TransactionHash,
    pub registration: Option<RegisterNakama>,
    pub chain: ChainContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinInputState {
    pub amount: Pearl,
    pub owner: Address,
}

pub trait TransactionStateView {
    fn pearl(&self, id: UtxoRef) -> Option<CoinInputState>;
    fn validate_registration(
        &self,
        _registration: &RegisterNakama,
        _chain: ChainContext,
    ) -> Result<(), NakamaError> {
        Err(NakamaError::InvalidSignature)
    }
}

pub fn validate_transaction(
    transaction: Transaction,
    chain: ChainContext,
    _current_height: u64,
    state: &impl TransactionStateView,
) -> Result<ValidatedTransaction, TransactionConsensusError> {
    transaction
        .intent
        .validate_structure()
        .map_err(TransactionConsensusError::Intent)?;
    if !transaction
        .verify_authorization(chain)
        .map_err(|_| TransactionConsensusError::Encoding)?
    {
        return Err(TransactionConsensusError::InvalidAuthorization);
    }

    let intent = &transaction.intent;
    validate_coin_inputs(&intent.inputs, &intent.outputs, intent.sender, state)?;
    if let Some(registration) = &transaction.registration {
        if registration.public_key != transaction.authorization.public_key {
            return Err(TransactionConsensusError::InvalidRegistration(
                NakamaError::WrongOwner,
            ));
        }
        state
            .validate_registration(registration, chain)
            .map_err(TransactionConsensusError::InvalidRegistration)?;
    }

    let txid = transaction
        .transaction_id()
        .map_err(|_| TransactionConsensusError::Encoding)?;
    Ok(ValidatedTransaction {
        intent: transaction.intent,
        txid,
        registration: transaction.registration,
        chain,
    })
}

fn validate_coin_inputs(
    inputs: &[Input],
    outputs: &[Output],
    sender: Address,
    state: &impl TransactionStateView,
) -> Result<(), TransactionConsensusError> {
    let mut unique = BTreeSet::new();
    let mut input_total = Pearl::ZERO;
    for input in inputs {
        let id = input.utxo;
        if !unique.insert(id) {
            return Err(TransactionConsensusError::Intent(
                IntentError::DuplicateInput,
            ));
        }
        let previous = state
            .pearl(id)
            .ok_or(TransactionConsensusError::UtxoNotFound)?;
        if previous.owner != sender {
            return Err(TransactionConsensusError::RecipientMismatch);
        }
        input_total = input_total
            .checked_add(previous.amount)
            .ok_or(TransactionConsensusError::PearlOverflow)?;
    }
    let output_total = outputs.iter().try_fold(Pearl::ZERO, |sum, output| {
        sum.checked_add(output.amount)
            .ok_or(TransactionConsensusError::PearlOverflow)
    })?;
    if input_total != output_total {
        return Err(TransactionConsensusError::ValueMismatch);
    }
    Ok(())
}

#[derive(Debug)]
pub enum TransactionConsensusError {
    Encoding,
    Intent(IntentError),
    InvalidAuthorization,
    UtxoNotFound,
    RecipientMismatch,
    PearlOverflow,
    ValueMismatch,
    InvalidRegistration(NakamaError),
}

impl fmt::Display for TransactionConsensusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encoding => formatter.write_str("transaction encoding failed"),
            Self::Intent(error) => write!(formatter, "invalid transaction intent: {error}"),
            Self::InvalidAuthorization => {
                formatter.write_str("transaction authorization is invalid")
            }
            Self::UtxoNotFound => formatter.write_str("transaction input UTXO was not found"),
            Self::RecipientMismatch => {
                formatter.write_str("transaction input is not committed to this signer")
            }
            Self::PearlOverflow => formatter.write_str("transaction amount overflow"),
            Self::ValueMismatch => {
                formatter.write_str("transaction input and output values differ")
            }
            Self::InvalidRegistration(error) => {
                write!(formatter, "invalid name registration: {error:?}")
            }
        }
    }
}

impl StdError for TransactionConsensusError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Intent(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto::TransactionHash;

    struct OneInput {
        outpoint: UtxoRef,
        amount: Pearl,
        owner: Address,
    }

    impl TransactionStateView for OneInput {
        fn pearl(&self, id: UtxoRef) -> Option<CoinInputState> {
            (id == self.outpoint).then_some(CoinInputState {
                amount: self.amount,
                owner: self.owner,
            })
        }
    }

    #[test]
    fn spend_requires_exact_value_conservation() {
        let sender = Address([7; crypto::ADDRESS_SIZE]);
        let outpoint = UtxoRef::new(TransactionHash([9; crypto::HASH_SIZE]), 0);
        let state = OneInput {
            outpoint,
            amount: Pearl::from_pearl(100),
            owner: sender,
        };
        let inputs = [Input::new(outpoint)];
        let outputs = [
            Output::new(sender, Pearl::from_pearl(90)),
            Output::block_miner(Pearl::from_pearl(10)),
        ];
        assert!(validate_coin_inputs(&inputs, &outputs, sender, &state).is_ok());

        let underfunded = [Output::new(sender, Pearl::from_pearl(99))];
        assert!(matches!(
            validate_coin_inputs(&inputs, &underfunded, sender, &state),
            Err(TransactionConsensusError::ValueMismatch)
        ));
        let overfunded = [Output::new(sender, Pearl::from_pearl(101))];
        assert!(matches!(
            validate_coin_inputs(&inputs, &overfunded, sender, &state),
            Err(TransactionConsensusError::ValueMismatch)
        ));
    }
}
