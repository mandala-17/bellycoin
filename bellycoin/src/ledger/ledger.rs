use std::{collections::BTreeMap, error::Error as StdError, fmt};

use borsh::{BorshDeserialize, BorshSerialize};

use crypto::{BlockHash, HashDomain, StateRoot, canonical_bytes, domain};

use common::{ChainContext, Height};

use crate::{
    blockchain::{Block, Chain, ChainError},
    consensus::{
        ApplyBlockState, BellycoinInputState, ConsensusError, EmissionError,
        TransactionConsensusError, TransactionStateView, ValidatedBlock, validate_emission,
        validate_transaction,
    },
    ledger::{Bellycoin, LedgerState, SpendRollbackJournal, StateError, StateRollbackJournal},
};

use crate::transaction::UtxoId;

#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    pub chain: Chain,
    pub state: LedgerState,

    journals: BTreeMap<Height, Vec<StateRollbackJournal>>,

    chain_context: Option<ChainContext>,
}

struct ExecutedBlock {
    state: LedgerState,
    journals: Vec<StateRollbackJournal>,
    state_root: StateRoot,
    block_size: u32,
    chain_context: ChainContext,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tip_height(&self) -> Option<Height> {
        self.chain.tip_height()
    }

    pub fn tip_hash(&self) -> Option<BlockHash> {
        self.chain.tip_hash()
    }

    pub fn state(&self) -> &LedgerState {
        &self.state
    }

    pub fn state_root(&self) -> Result<StateRoot, LedgerError> {
        self.state.application_state_root()
    }

    pub fn preview_block_commitments(
        &self,
        block: &Block,
    ) -> Result<(StateRoot, u32), LedgerError> {
        let executed = self.execute_block(block)?;
        Ok((executed.state_root, executed.block_size))
    }

    fn execute_block(&self, block: &Block) -> Result<ExecutedBlock, LedgerError> {
        let mut state = self.state.clone();
        let mut journals = Vec::new();
        let block_size = u32::try_from(block.size()?).map_err(|_| LedgerError::InvalidBlockSize)?;
        let height = block.height();
        let chain_context = match self.chain_context {
            Some(context) => context,
            None if block.is_genesis() => ChainContext::new(block.hash()?.into_bytes()),
            None => return Err(LedgerError::EmptyChain),
        };

        if !block.is_genesis() {
            let emission = validate_emission(block)?;
            let id = UtxoId::emission(emission.origin());
            state.utxos.insert_pearl(
                id,
                Bellycoin {
                    amount: emission.subsidy(),
                    owner: emission.recipient(),
                },
            )?;
            state.bellycoin.total_mined = state
                .bellycoin
                .total_mined
                .checked_add(emission.subsidy())
                .ok_or(StateError::AmountOverflow)?;

            let spend = SpendRollbackJournal {
                created_pearl_ids: vec![id],
                mined: emission.subsidy(),
                ..SpendRollbackJournal::default()
            };
            journals.push(StateRollbackJournal {
                spend: Some(spend),
                registered_name: None,
                registered_public_key: None,
            });
        }

        for transaction in block.transactions() {
            let validated =
                validate_transaction(transaction.clone(), chain_context, height.0, &state)?;
            journals.push(state.apply_validated_transaction(&validated, block.miner_address())?);
        }

        let state_root = state.application_state_root()?;
        Ok(ExecutedBlock {
            state,
            journals,
            state_root,
            block_size,
            chain_context,
        })
    }

    pub fn rollback_tip(&mut self) -> Result<Block, LedgerError> {
        let height = self.chain.tip_height().ok_or(LedgerError::EmptyChain)?;

        let hash = self.chain.tip_hash().ok_or(LedgerError::EmptyChain)?;

        let journals = self
            .journals
            .get(&height)
            .cloned()
            .ok_or(LedgerError::MissingRollbackJournal)?;

        let mut staged_state = self.state.clone();

        for journal in journals.into_iter().rev() {
            staged_state.rollback_state(journal)?;
        }

        let mut staged_chain = self.chain.clone();

        let block = staged_chain.remove_tip(hash)?;

        self.state = staged_state;

        self.chain = staged_chain;

        self.journals.remove(&height);

        if self.chain.tip_height().is_none() {
            self.chain_context = None;
        }

        Ok(block)
    }

    fn apply_validated_block(&mut self, validated: ValidatedBlock) -> Result<(), LedgerError> {
        let block = validated.block();
        let height = block.height();
        let executed = self.execute_block(block)?;

        if executed.block_size != block.block_size() {
            return Err(LedgerError::InvalidBlockSize);
        }
        if block.state_root() != executed.state_root {
            return Err(LedgerError::InvalidStateRoot);
        }
        let mut staged_chain = self.chain.clone();
        staged_chain.insert_block(block.clone())?;
        self.state = executed.state;
        self.chain = staged_chain;
        self.chain_context = Some(executed.chain_context);
        self.journals.insert(height, executed.journals);
        Ok(())
    }
}

//
// Consensus state interface
//

impl ApplyBlockState for Ledger {
    type Error = LedgerError;

    fn consensus_chain(&self) -> &Chain {
        &self.chain
    }

    fn commit_validated_block(&mut self, block: ValidatedBlock) -> Result<(), Self::Error> {
        self.apply_validated_block(block)
    }
}

//
// Transaction state view
//

impl TransactionStateView for LedgerState {
    fn public_key(&self, address: crypto::Address) -> Option<crypto::PublicKey> {
        self.nakama.public_key(address).cloned()
    }
    fn pearl(&self, id: UtxoId) -> Option<BellycoinInputState> {
        self.utxos.pearl(&id).map(|pearl| BellycoinInputState {
            amount: pearl.amount,
            owner: pearl.owner,
        })
    }

    fn validate_registration(
        &self,
        registration: &crate::ledger::nakama::RegisterNakama,
        chain: ChainContext,
    ) -> Result<(), crate::ledger::nakama::NakamaError> {
        self.nakama.validate_registration(registration, chain)
    }
}

//
// Canonical application state root
//

impl LedgerState {
    pub(crate) fn application_state_root(&self) -> Result<StateRoot, LedgerError> {
        if self.utxos.is_empty()
            && self.bellycoin.total_mined.is_zero()
            && self.bellycoin.total_burned.is_zero()
            && self.nakama.is_empty()
        {
            return Ok(StateRoot::ZERO);
        }

        let state = canonical_bytes(&(&self.utxos, &self.bellycoin, &self.nakama))?;

        Ok(StateRoot(
            domain(HashDomain::ProtocolState, &state).into_bytes(),
        ))
    }
}

//
// Ledger errors
//

#[derive(Debug)]
pub enum LedgerError {
    Consensus(ConsensusError),

    Transaction(TransactionConsensusError),

    State(StateError),

    Chain(ChainError),

    Emission(EmissionError),

    EmptyChain,

    MissingParentEmission,

    MissingRollbackJournal,

    InvalidStateRoot,

    InvalidBlockSize,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Consensus(error) => {
                write!(formatter, "consensus validation failed: {error}")
            }

            Self::Transaction(error) => {
                write!(formatter, "transaction validation failed: {error}")
            }

            Self::State(error) => {
                write!(formatter, "ledger state transition failed: {error}")
            }

            Self::Chain(error) => {
                write!(formatter, "chain transition failed: {error}")
            }

            Self::Emission(error) => {
                write!(formatter, "emission validation failed: {error}")
            }

            Self::EmptyChain => formatter.write_str("ledger chain is empty"),

            Self::MissingParentEmission => formatter.write_str("parent emission is missing"),

            Self::MissingRollbackJournal => formatter.write_str("rollback journal is missing"),

            Self::InvalidStateRoot => formatter.write_str("block state root does not match ledger"),

            Self::InvalidBlockSize => {
                formatter.write_str("block execution size does not match ledger")
            }
        }
    }
}

impl StdError for LedgerError {}

impl From<ConsensusError> for LedgerError {
    fn from(error: ConsensusError) -> Self {
        Self::Consensus(error)
    }
}

impl From<TransactionConsensusError> for LedgerError {
    fn from(error: TransactionConsensusError) -> Self {
        Self::Transaction(error)
    }
}

impl From<EmissionError> for LedgerError {
    fn from(error: EmissionError) -> Self {
        Self::Emission(error)
    }
}

impl From<StateError> for LedgerError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<crate::ledger::utxo::Error> for LedgerError {
    fn from(error: crate::ledger::utxo::Error) -> Self {
        Self::State(StateError::Utxo(error))
    }
}

impl From<ChainError> for LedgerError {
    fn from(error: ChainError) -> Self {
        Self::Chain(error)
    }
}

impl From<crypto::CodecError> for LedgerError {
    fn from(_error: crypto::CodecError) -> Self {
        Self::Consensus(ConsensusError::Serialization)
    }
}

#[cfg(test)]
mod p3e_block_atomicity_tests {
    use super::*;

    use crate::{
        blockchain::{Block, Emission},
        consensus::{
            ConsensusError, expected_emission_for_height, expected_next_difficulty,
            validate_candidate_for_apply,
        },
        genesis,
    };
    use common::Nonce;

    #[test]
    fn full_subsidy_is_mined() {
        let mut ledger = genesis::genesis_ledger().expect("genesis ledger");
        let miner = crypto::Address([0x31; crypto::ADDRESS_SIZE]);
        commit_empty_block(&mut ledger, miner);
        let subsidy = crate::consensus::expected_emission_for_height(Height(1));
        assert_eq!(ledger.state.bellycoin.total_mined, subsidy);
        let outputs = ledger.state.utxos.pearls().collect::<Vec<_>>();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].1.amount, subsidy);
        assert_eq!(outputs[0].1.owner, miner);
    }

    fn ledger_bytes(ledger: &Ledger) -> Vec<u8> {
        borsh::to_vec(ledger).expect("ledger must serialize canonically")
    }

    fn empty_next_candidate(ledger: &Ledger, miner: crypto::Address) -> Block {
        let height = Height(
            ledger
                .tip_height()
                .map_or(0, |height| height.0.saturating_add(1)),
        );

        let previous = ledger.tip_hash().expect("canonical tip");

        let target_bits = expected_next_difficulty(&ledger.chain).expect("next target bits");

        let subsidy = expected_emission_for_height(height);

        Block::from_protocol_transactions(
            height,
            previous,
            target_bits,
            60 * height.0,
            Nonce(0),
            Some(Emission::new(miner, subsidy)),
            vec![],
        )
        .expect("empty candidate")
    }

    fn commit_empty_block(ledger: &mut Ledger, miner: crypto::Address) -> Block {
        let mut block = empty_next_candidate(ledger, miner);

        let (state_root, block_size) = ledger
            .preview_block_commitments(&block)
            .expect("preview commitments");

        block.set_state_root(state_root);
        block.set_block_size(block_size);

        let validated =
            validate_candidate_for_apply(&block, &ledger.chain).expect("valid candidate");

        ledger
            .apply_validated_block(validated)
            .expect("commit block");

        block
    }

    fn empty_height_one_candidate(ledger: &Ledger, miner: crypto::Address) -> Block {
        let previous = ledger.tip_hash().expect("genesis tip");

        let target_bits = expected_next_difficulty(&ledger.chain).expect("next target bits");

        Block::from_protocol_transactions(
            Height(1),
            previous,
            target_bits,
            60,
            Nonce(0),
            Some(Emission::new(
                miner,
                expected_emission_for_height(Height(1)),
            )),
            vec![],
        )
        .expect("height-one candidate")
    }

    #[test]
    fn invalid_state_root_after_staging_does_not_mutate_ledger() {
        let mut ledger = genesis::genesis_ledger().expect("genesis ledger");

        let miner = crypto::Address([0x41; crypto::ADDRESS_SIZE]);

        let before = ledger_bytes(&ledger);

        let before_tip = ledger.tip_hash();

        let block = empty_height_one_candidate(&ledger, miner);

        let validated = validate_candidate_for_apply(&block, &ledger.chain)
            .expect("candidate must pass pre-application consensus");

        assert!(matches!(
            ledger.apply_validated_block(validated),
            Err(LedgerError::InvalidStateRoot)
        ));

        assert_eq!(ledger_bytes(&ledger), before);

        assert_eq!(ledger.tip_hash(), before_tip);

        assert_eq!(ledger.tip_height(), Some(Height(0)));
    }

    #[test]
    fn committed_block_then_rollback_restores_entire_ledger_byte_for_byte() {
        let mut ledger = genesis::genesis_ledger().expect("genesis ledger");

        let miner = crypto::Address([0x42; crypto::ADDRESS_SIZE]);

        let before = ledger_bytes(&ledger);

        let before_tip = ledger.tip_hash();

        let mut block = empty_height_one_candidate(&ledger, miner);

        let (state_root, block_size) = ledger
            .preview_block_commitments(&block)
            .expect("preview commitments");

        block.set_state_root(state_root);
        block.set_block_size(block_size);

        let validated =
            validate_candidate_for_apply(&block, &ledger.chain).expect("valid candidate");

        ledger
            .apply_validated_block(validated)
            .expect("commit block");

        assert_eq!(ledger.tip_height(), Some(Height(1)));

        assert_ne!(ledger_bytes(&ledger), before);

        let removed = ledger.rollback_tip().expect("rollback tip");

        assert_eq!(removed, block);

        assert_eq!(ledger.tip_hash(), before_tip);

        assert_eq!(ledger.tip_height(), Some(Height(0)));

        assert_eq!(ledger_bytes(&ledger), before);
    }

    #[test]
    fn two_committed_blocks_then_two_rollbacks_restore_genesis_byte_for_byte() {
        let mut ledger = genesis::genesis_ledger().expect("genesis ledger");

        let genesis_bytes = ledger_bytes(&ledger);

        let genesis_tip = ledger.tip_hash();

        let miner_one = crypto::Address([0x51; crypto::ADDRESS_SIZE]);

        let miner_two = crypto::Address([0x52; crypto::ADDRESS_SIZE]);

        let block_one = commit_empty_block(&mut ledger, miner_one);

        assert_eq!(ledger.tip_height(), Some(Height(1)));

        let height_one_bytes = ledger_bytes(&ledger);

        let height_one_tip = ledger.tip_hash();

        let block_two = commit_empty_block(&mut ledger, miner_two);

        assert_eq!(ledger.tip_height(), Some(Height(2)));

        assert_ne!(ledger_bytes(&ledger), height_one_bytes);

        let removed_two = ledger.rollback_tip().expect("rollback height two");

        assert_eq!(removed_two, block_two);

        assert_eq!(ledger.tip_height(), Some(Height(1)));

        assert_eq!(ledger.tip_hash(), height_one_tip);

        assert_eq!(ledger_bytes(&ledger), height_one_bytes);

        let removed_one = ledger.rollback_tip().expect("rollback height one");

        assert_eq!(removed_one, block_one);

        assert_eq!(ledger.tip_height(), Some(Height(0)));

        assert_eq!(ledger.tip_hash(), genesis_tip);

        assert_eq!(ledger_bytes(&ledger), genesis_bytes);
    }

    #[test]
    fn failed_second_block_does_not_mutate_committed_first_block() {
        let mut ledger = genesis::genesis_ledger().expect("genesis ledger");

        let miner_one = crypto::Address([0x61; crypto::ADDRESS_SIZE]);

        let miner_two = crypto::Address([0x62; crypto::ADDRESS_SIZE]);

        let block_one = commit_empty_block(&mut ledger, miner_one);

        assert_eq!(ledger.tip_height(), Some(Height(1)));

        let before = ledger_bytes(&ledger);

        let before_tip = ledger.tip_hash();

        let block_two = empty_next_candidate(&ledger, miner_two);

        let validated = validate_candidate_for_apply(&block_two, &ledger.chain)
            .expect("candidate must pass pre-application consensus");

        assert!(matches!(
            ledger.apply_validated_block(validated),
            Err(LedgerError::InvalidStateRoot)
        ));

        assert_eq!(ledger.tip_height(), Some(Height(1)));

        assert_eq!(ledger.tip_hash(), before_tip);

        assert_eq!(ledger_bytes(&ledger), before);

        let removed = ledger.rollback_tip().expect("height-one rollback");

        assert_eq!(removed, block_one);

        assert_eq!(ledger.tip_height(), Some(Height(0)));
    }

    #[test]
    fn invalid_block_size_after_staging_does_not_mutate_ledger() {
        let mut ledger = genesis::genesis_ledger().expect("genesis ledger");

        let miner = crypto::Address([0x71; crypto::ADDRESS_SIZE]);

        let before = ledger_bytes(&ledger);
        let before_tip = ledger.tip_hash();

        let mut block = empty_next_candidate(&ledger, miner);

        let (state_root, block_size) = ledger
            .preview_block_commitments(&block)
            .expect("preview commitments");

        block.set_state_root(state_root);

        //
        // Keep the size structurally plausible, but make it
        // different from the canonical execution size.
        //
        block.set_block_size(
            block_size
                .checked_add(1)
                .expect("fixture block size overflow"),
        );

        //
        // The malformed size is still large enough to satisfy
        // block-local structural validation.
        //
        let validated = validate_candidate_for_apply(&block, &ledger.chain)
            .expect("candidate must pass pre-application consensus");

        assert!(matches!(
            ledger.apply_validated_block(validated),
            Err(LedgerError::InvalidBlockSize)
        ));

        assert_eq!(ledger_bytes(&ledger), before);

        assert_eq!(ledger.tip_hash(), before_tip);

        assert_eq!(ledger.tip_height(), Some(Height(0)));
    }

    #[test]
    fn invalid_next_height_is_rejected_without_mutating_ledger() {
        let ledger = genesis::genesis_ledger().expect("genesis ledger");

        let miner = crypto::Address([0x72; crypto::ADDRESS_SIZE]);

        let before = ledger_bytes(&ledger);
        let before_tip = ledger.tip_hash();

        let mut block = empty_next_candidate(&ledger, miner);

        //
        // Genesis tip is height 0, therefore the only valid
        // next block is height 1.
        //
        block.height = Height(2);

        assert!(matches!(
            validate_candidate_for_apply(&block, &ledger.chain,),
            Err(ConsensusError::InvalidHeight)
        ));

        assert_eq!(ledger_bytes(&ledger), before);

        assert_eq!(ledger.tip_hash(), before_tip);

        assert_eq!(ledger.tip_height(), Some(Height(0)));
    }
    #[test]
    fn invalid_previous_hash_is_rejected_without_mutating_ledger() {
        let ledger = genesis::genesis_ledger().expect("genesis ledger");

        let miner = crypto::Address([0x73; crypto::ADDRESS_SIZE]);

        let before = ledger_bytes(&ledger);
        let before_tip = ledger.tip_hash();

        let mut block = empty_next_candidate(&ledger, miner);

        let mut wrong_previous = ledger.tip_hash().expect("genesis tip").0;

        wrong_previous[0] ^= 0xff;

        block.header.previous_hash = crypto::PreviousHash(wrong_previous);

        assert!(matches!(
            validate_candidate_for_apply(&block, &ledger.chain,),
            Err(ConsensusError::InvalidPreviousHash)
        ));

        assert_eq!(ledger_bytes(&ledger), before);

        assert_eq!(ledger.tip_hash(), before_tip);

        assert_eq!(ledger.tip_height(), Some(Height(0)));
    }
}
