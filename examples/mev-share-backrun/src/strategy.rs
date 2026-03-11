use anyhow::Result;
use async_trait::async_trait;
use artemis_core::types::Strategy;
use ethers::types::{Bytes, H256, U64};
use mev_share::rpc::{BundleItem, Inclusion, SendBundleRequest};
use tracing::info;

use crate::types::{Action, BackrunConfig, Event};

/// A simple MEV-Share backrun strategy.
///
/// For every MEV-Share event that contains logs (indicating an interesting
/// on-chain interaction), this strategy submits a backrun bundle that targets
/// the pending transaction.
pub struct MevShareBackrun {
    pub config: BackrunConfig,
    /// The most recently seen block number, used to set bundle inclusion range.
    pub current_block: u64,
}

impl MevShareBackrun {
    pub fn new(config: BackrunConfig) -> Self {
        Self {
            config,
            current_block: 1,
        }
    }

    /// Update the current block number.
    pub fn set_block(&mut self, block: u64) {
        self.current_block = block;
    }
}

#[async_trait]
impl Strategy<Event, Action> for MevShareBackrun {
    async fn sync_state(&mut self) -> Result<()> {
        Ok(())
    }

    async fn process_event(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::MevShareEvent(mev_event) => {
                if mev_event.logs.is_empty() {
                    return vec![];
                }
                info!("Received MEV-Share event with logs, submitting backrun bundle: {:?}", mev_event.hash);
                let bundle = create_backrun_bundle(mev_event.hash, self.current_block);
                vec![Action::SubmitBundle(bundle)]
            }
        }
    }
}

/// Build a backrun bundle targeting `tx_hash` to be included starting at
/// `current_block + 1`.
///
/// The placeholder backrun transaction (`0x0200`) is a stub — replace it with
/// a real signed transaction in production.
pub fn create_backrun_bundle(tx_hash: H256, current_block: u64) -> SendBundleRequest {
    let target = BundleItem::Hash { hash: tx_hash };

    // Placeholder backrun tx — replace with a real signed EIP-1559 transaction.
    let backrun = BundleItem::Tx {
        tx: Bytes::from(vec![0x02, 0x00]),
        can_revert: true,
    };

    SendBundleRequest {
        bundle_body: vec![target, backrun],
        inclusion: Inclusion {
            block: U64::from(current_block + 1),
            max_block: Some(U64::from(current_block + 25)),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creates_bundle_for_tx() {
        let hash = H256::random();
        let bundle = create_backrun_bundle(hash, 100);

        assert_eq!(bundle.inclusion.block, U64::from(101));
        assert_eq!(bundle.inclusion.max_block, Some(U64::from(125)));
        assert_eq!(bundle.bundle_body.len(), 2);
    }

    #[test]
    fn test_bundle_includes_target_hash() {
        let hash = H256::random();
        let bundle = create_backrun_bundle(hash, 50);

        match &bundle.bundle_body[0] {
            BundleItem::Hash { hash: h } => assert_eq!(*h, hash),
            _ => panic!("first bundle item should be a Hash"),
        }
    }

    #[test]
    fn test_bundle_contains_backrun_tx() {
        let bundle = create_backrun_bundle(H256::zero(), 1);

        match &bundle.bundle_body[1] {
            BundleItem::Tx { tx, can_revert } => {
                assert_eq!(tx.as_ref(), &[0x02, 0x00]);
                assert!(can_revert);
            }
            _ => panic!("second bundle item should be a Tx"),
        }
    }

    #[test]
    fn test_set_block_updates_current_block() {
        let config = BackrunConfig {
            executor_address: ethers::types::H160::zero(),
            mev_share_url: "https://mev-share.flashbots.net".to_string(),
        };
        let mut strategy = MevShareBackrun::new(config);
        assert_eq!(strategy.current_block, 1);

        strategy.set_block(999);
        assert_eq!(strategy.current_block, 999);

        let bundle = create_backrun_bundle(H256::zero(), strategy.current_block);
        assert_eq!(bundle.inclusion.block, U64::from(1000));
    }
}
