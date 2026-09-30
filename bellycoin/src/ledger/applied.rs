use crypto::Address;

use crate::{
    consensus::ValidatedTransaction,
    ledger::nakama::nakama_registration_burn,
    ledger::{Bellycoin, LedgerState, SpendRollbackJournal, StateError, StateRollbackJournal},
    transaction::{SpendIntent, UtxoId},
};
use common::Nakama;
use crypto::TransactionHash;

impl LedgerState {
    pub fn apply_validated_transaction(
        &mut self,
        transaction: &ValidatedTransaction,
        bounty_hunter: Address,
    ) -> Result<StateRollbackJournal, StateError> {
        let burn = transaction
            .registration
            .as_ref()
            .map(nakama_registration_burn)
            .transpose()
            .map_err(|_| StateError::InvalidTransaction)?
            .unwrap_or_default();
        let registered_public_key = if let Some(key) = &transaction.revealed_key {
            self.nakama
                .register_public_key(transaction.intent.sender, key.clone())
                .map_err(|_| StateError::InvalidTransaction)?
                .then_some(transaction.intent.sender)
        } else {
            None
        };
        let (registered_name, previous_name_record) =
            if let Some(registration) = &transaction.registration {
                let previous = match self.nakama.register(
                    registration.clone(),
                    transaction.chain,
                    transaction.height,
                ) {
                    Ok(previous) => previous,
                    Err(_) => {
                        if let Some(address) = registered_public_key {
                            self.nakama.remove_public_key(address);
                        }
                        return Err(StateError::InvalidTransaction);
                    }
                };
                (Some(registration.name.clone()), previous)
            } else {
                (None, None)
            };
        let spend = match self.apply_onchain_spend(
            &transaction.intent,
            transaction.txid,
            bounty_hunter,
            burn,
            transaction.height,
        ) {
            Ok(spend) => spend,
            Err(error) => {
                if let Some(name) = &registered_name {
                    self.nakama.restore(name.clone(), previous_name_record);
                }
                if let Some(address) = registered_public_key {
                    self.nakama.remove_public_key(address);
                }
                return Err(error);
            }
        };
        Ok(StateRollbackJournal {
            spend: Some(spend),
            registered_name,
            previous_name_record,
            registered_public_key,
        })
    }

    fn apply_onchain_spend(
        &mut self,
        intent: &SpendIntent,
        txid: TransactionHash,
        bounty_hunter: Address,
        burn: crate::transaction::Pearl,
        height: u64,
    ) -> Result<SpendRollbackJournal, StateError> {
        let mut journal = SpendRollbackJournal::default();

        let result = (|| {
            let (inputs, outputs) = (&intent.inputs, &intent.outputs);

            for id in inputs {
                let pearl = self.utxos.consume_pearl(&id.utxo)?;

                journal.consumed_pearls.push((id.utxo, pearl));
            }

            for (index, output) in outputs.iter().enumerate() {
                let id = UtxoId::transaction(txid, output_index(index)?);

                let owner = match output.output {
                    Nakama::Address(address) => address,
                    Nakama::BountyHunter => bounty_hunter,
                };

                self.utxos.insert_pearl(
                    id,
                    Bellycoin {
                        amount: output.amount,
                        owner,
                        spendable_height: height,
                    },
                )?;

                journal.created_pearl_ids.push(id);
            }

            self.bellycoin.total_burned = self
                .bellycoin
                .total_burned
                .checked_add(burn)
                .ok_or(StateError::AmountOverflow)?;
            journal.burned = burn;

            Ok(())
        })();

        self.finish_spend_transition(journal, result)
    }

    pub(crate) fn rollback_state(
        &mut self,
        journal: StateRollbackJournal,
    ) -> Result<(), StateError> {
        if let Some(name) = journal.registered_name {
            self.nakama.restore(name, journal.previous_name_record);
        }
        if let Some(address) = journal.registered_public_key {
            self.nakama.remove_public_key(address);
        }
        if let Some(spend) = journal.spend {
            self.rollback_spend(spend)?;
        }

        Ok(())
    }

    pub(crate) fn rollback_spend(
        &mut self,
        journal: SpendRollbackJournal,
    ) -> Result<(), StateError> {
        self.bellycoin.total_mined = self
            .bellycoin
            .total_mined
            .checked_sub(journal.mined)
            .ok_or(StateError::AmountOverflow)?;
        self.bellycoin.total_burned = self
            .bellycoin
            .total_burned
            .checked_sub(journal.burned)
            .ok_or(StateError::AmountOverflow)?;

        for id in journal.created_pearl_ids {
            self.utxos.consume_pearl(&id)?;
        }

        for (id, pearl) in journal.consumed_pearls {
            self.utxos.insert_pearl(id, pearl)?;
        }

        Ok(())
    }

    fn finish_spend_transition(
        &mut self,
        journal: SpendRollbackJournal,
        result: Result<(), StateError>,
    ) -> Result<SpendRollbackJournal, StateError> {
        match result {
            Ok(()) => Ok(journal),

            Err(error) => {
                self.rollback_spend(journal)?;
                Err(error)
            }
        }
    }
}

fn output_index(index: usize) -> Result<u32, StateError> {
    u32::try_from(index).map_err(|_| StateError::OutputIndexOverflow)
}
