use crypto::Address;

use crate::{
    consensus::ValidatedTransaction,
    ledger::{Bellycoin, LedgerState, SpendRollbackJournal, StateError, StateRollbackJournal},
    transaction::{SpendIntent, UtxoRef},
};
use common::Nakama;
use crypto::TransactionHash;

impl LedgerState {
    pub fn apply_validated_transaction(
        &mut self,
        transaction: &ValidatedTransaction,
        block_miner: Address,
    ) -> Result<StateRollbackJournal, StateError> {
        let spend = self.apply_onchain_spend(&transaction.intent, transaction.txid, block_miner)?;
        Ok(StateRollbackJournal { spend: Some(spend) })
    }

    fn apply_onchain_spend(
        &mut self,
        intent: &SpendIntent,
        txid: TransactionHash,
        block_miner: Address,
    ) -> Result<SpendRollbackJournal, StateError> {
        let mut journal = SpendRollbackJournal::default();

        let result = (|| {
            let (inputs, outputs) = (&intent.inputs, &intent.outputs);

            for id in inputs {
                let pearl = self.utxos.consume_pearl(&id.utxo)?;

                journal.consumed_pearls.push((id.utxo, pearl));
            }

            for (index, output) in outputs.iter().enumerate() {
                let id = UtxoRef::new(txid, output_index(index)?);

                let owner = match output.output {
                    Nakama::Address(address) => address,
                    Nakama::BountyHunter => block_miner,
                };

                self.utxos.insert_pearl(
                    id,
                    Bellycoin {
                        amount: output.amount,
                        owner,
                    },
                )?;

                journal.created_pearl_ids.push(id);
            }

            Ok(())
        })();

        self.finish_spend_transition(journal, result)
    }

    pub(crate) fn rollback_state(
        &mut self,
        journal: StateRollbackJournal,
    ) -> Result<(), StateError> {
        if let Some(spend) = journal.spend {
            self.rollback_spend(spend)?;
        }

        Ok(())
    }

    pub(crate) fn rollback_spend(
        &mut self,
        journal: SpendRollbackJournal,
    ) -> Result<(), StateError> {
        self.coin.total_mined = self
            .coin
            .total_mined
            .checked_sub(journal.mined)
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
