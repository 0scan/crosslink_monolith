//! Explorer-facing all-time miner ranking contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PageDirection;

/// Parameters for `gettopminers`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct TopMinersRequest {
    /// Maximum number of ranked miners to return, from 1 through 100.
    pub limit: Option<u32>,
    /// Opaque cursor returned by a previous page.
    pub cursor: Option<String>,
    /// Direction to move from `cursor`.
    pub direction: PageDirection,
}

/// One miner in the all-time ranking.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct TopMinerEntry {
    /// One-based position in the ranking.
    pub rank: u64,
    /// Best-effort transparent coinbase payout address.
    pub address: String,
    /// Best-effort mining-pool attribution, or `Unknown`.
    pub pool: String,
    /// Blocks attributed to this address over the indexed chain.
    pub blocks_mined: String,
    /// Scheduled miner subsidy attributed to this address, excluding fees.
    pub mined_zat: String,
    /// Share of every indexed block, formatted with one decimal place.
    pub block_share_percent: String,
    /// Height of the most recent block attributed to this address.
    pub last_mined_height: String,
    /// Hash of the most recent block attributed to this address.
    pub last_mined_block_hash: String,
    /// Timestamp of the most recent block attributed to this address.
    pub last_mined_at: String,
    /// Stake publicly attributable to this transparent payout address.
    ///
    /// This is `null` because Crosslink bonds do not reveal a link to a transparent address.
    pub staked_zat: Option<String>,
    /// Finalizers publicly attributable to this transparent payout address.
    ///
    /// This is `null` because finalizer keys are not linked to transparent payout addresses.
    pub finalizer_count: Option<String>,
}

/// Aggregate values displayed above the all-time miner ranking.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct TopMinersSummary {
    /// Every canonical block covered by the explorer index.
    pub block_count: String,
    /// Blocks with a recognizable transparent miner payout address.
    pub attributed_block_count: String,
    /// Blocks without a recognizable transparent miner payout address.
    pub unattributed_block_count: String,
    /// Distinct attributed miners, before applying `limit`.
    pub miner_count: String,
}

/// Cursor metadata for an all-time miner ranking page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct TopMinersPagination {
    /// Maximum number of miners requested.
    pub limit: u32,
    /// Total number of attributed miners.
    pub total: String,
    /// Whether another lower-ranked page exists.
    pub has_next: bool,
    /// Whether another higher-ranked page exists.
    pub has_prev: bool,
    /// Opaque position to pass with `direction=next`.
    pub next_cursor: Option<String>,
    /// Opaque position to pass with `direction=prev`.
    pub prev_cursor: Option<String>,
}

/// Persisted all-time miner ranking from one finalized explorer snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct TopMinersResponse {
    /// Ranked miner rows.
    pub miners: Vec<TopMinerEntry>,
    /// All-time aggregate values.
    pub summary: TopMinersSummary,
    /// Page navigation metadata.
    pub pagination: TopMinersPagination,
    /// Highest finalized indexed height represented by this response.
    pub indexed_height: Option<String>,
    /// Finalized indexed block hash represented by this response.
    pub indexed_block_hash: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{PageDirection, TopMinersRequest};

    #[test]
    fn top_miners_request_rejects_time_windows() {
        assert!(
            serde_json::from_value::<TopMinersRequest>(serde_json::json!({"period": "24h"}))
                .is_err()
        );
    }

    #[test]
    fn top_miners_request_defaults_to_the_first_page() {
        let request = serde_json::from_value::<TopMinersRequest>(serde_json::json!({}))
            .expect("an empty top-miners request is valid");

        assert_eq!(request.limit, None);
        assert_eq!(request.cursor, None);
        assert_eq!(request.direction, PageDirection::Next);
    }
}
