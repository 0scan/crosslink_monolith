//! Request and response types for explorer JSON-RPC methods.

mod crosslink;
mod mempool;
mod requests;
mod stats;

pub(in crate::methods) use zebra_indexer::{
    AddressSummary, AddressTransactionsResponse, AddressUtxosResponse, BlockDetails,
    BlocksResponse, ChartDataRequest, ChartDataResponse, MinerInfoResponse, TopBalancesRequest,
    TopBalancesResponse, TopMinersRequest, TopMinersResponse, TransactionsResponse,
};

#[cfg(feature = "indexer")]
pub(super) use crosslink::{
    CrosslinkActivationMilestone, CrosslinkActivationOverview, CrosslinkFinalityOverview,
    CrosslinkFinalityStatus, CrosslinkFinalizersOverview, CrosslinkMinersOverview,
    CrosslinkPhase, CrosslinkStakingChange, CrosslinkStakingOverview, CrosslinkStakingStatus,
};
pub use crosslink::CrosslinkNetworkStats;
pub use mempool::{
    MempoolTransactionListItem, MempoolTransactionMetadata, MempoolTransactionSummary,
    MempoolTransactionsResponse, PendingTransactionDetails, TransactionDetailsResponse,
};
pub use requests::{
    GetAddressTransactionsRequest, GetAddressUtxosPageRequest, GetBlocksRequest,
    GetMempoolTransactionsRequest, GetTransactionsRequest,
};
#[cfg(feature = "indexer")]
pub(super) use stats::{
    BlockchainRuntimeStats, MempoolStats, MiningStats, NetworkStats, NodeSyncStats,
    SupplyPoolStats, SupplyStats,
};
pub use stats::{ExplorerNetworkStatsResponse, IndexerStatusResponse};
