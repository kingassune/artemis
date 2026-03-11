use mev_share::{rpc::SendBundleRequest, sse};

/// Events produced by the MEV-Share SSE collector and consumed by this strategy.
#[derive(Debug, Clone)]
pub enum Event {
    MevShareEvent(sse::Event),
}

/// Actions produced by this strategy and consumed by the MEV-Share executor.
#[derive(Debug, Clone)]
pub enum Action {
    SubmitBundle(SendBundleRequest),
}

/// Configuration for the backrun strategy.
#[derive(Debug, Clone)]
pub struct BackrunConfig {
    /// Address of the contract that executes the backrun transaction.
    pub executor_address: ethers::types::H160,
    /// MEV-Share SSE endpoint URL.
    pub mev_share_url: String,
}
