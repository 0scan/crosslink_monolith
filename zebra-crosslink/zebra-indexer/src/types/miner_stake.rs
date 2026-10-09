//! Explorer-facing current transparent-address-to-finalizer stake contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PageDirection;

/// Parameters for `getcrosslinkminerstake` and finalizer source pages.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CrosslinkMinerStakeRequest {
    /// Restrict rows to this representative transparent funding address.
    pub address: Option<String>,
    /// Select miner sources with true, non-miners with false, or all sources when omitted.
    pub is_miner: Option<bool>,
    /// Maximum number of current address-finalizer pairs to return, from 1 through 100.
    pub limit: Option<u32>,
    /// Opaque cursor returned by a previous page.
    pub cursor: Option<String>,
    /// Direction to move from `cursor`.
    pub direction: PageDirection,
}

/// One current transparent-funded bond aggregate targeting one finalizer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkMinerStakeEntry {
    /// One-based position in the selected ranking.
    pub rank: u64,
    /// Representative transparent address that originally funded these bonds.
    pub address: String,
    /// Whether this source address has been observed mining a canonical block.
    pub is_miner: bool,
    /// Best-effort mining pool attribution.
    pub pool: String,
    /// Canonical blocks attributed to this source address; zero for non-miners.
    pub blocks_mined: String,
    /// Target finalizer public key.
    pub finalizer_public_key: String,
    /// Verified finalizer address when one has appeared on chain.
    pub finalizer_address: Option<String>,
    /// Current value of active bonds attributed to this pair, including bond rewards.
    pub current_stake_zat: String,
    /// This pair's share of current stake in the selected network or finalizer scope.
    pub stake_share_percent: String,
    /// Active attributed bonds represented by this pair.
    pub active_bond_count: String,
    /// Create or retarget actions that have added stake to this pair.
    pub stake_action_count: String,
    /// Height of the latest create or retarget that added stake to this pair.
    pub last_staked_height: String,
    /// Hash of the block containing the latest create or retarget.
    pub last_staked_block_hash: String,
    /// Timestamp of the latest create or retarget.
    pub last_staked_at: String,
}

/// One current stake-source category with an observable source count.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakeSourceGroup {
    pub stake_zat: String,
    pub share_percent: String,
    pub count: String,
}

/// One current stake-source category whose identities are private or unavailable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakeSourceAmount {
    pub stake_zat: String,
    pub share_percent: String,
}

/// Complete current source breakdown for the selected network or finalizer scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakeSourceBreakdown {
    pub miners: CrosslinkStakeSourceGroup,
    /// Transparent source addresses not observed mining a canonical block.
    pub others: CrosslinkStakeSourceGroup,
    pub shielded: CrosslinkStakeSourceAmount,
    /// Mixed, missing, or otherwise ambiguous transaction sources.
    pub unknown: Option<CrosslinkStakeSourceAmount>,
    /// Finalizer reward-bank value, including value converted into reward-funded bonds.
    pub rewards: Option<CrosslinkStakeSourceAmount>,
}

/// Current stake attribution coverage for a network or finalizer snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkMinerStakeSummary {
    /// All current stake in the selected scope.
    pub total_current_stake_zat: String,
    /// Current stake in bonds with a directly attributable miner source.
    pub miner_attributed_stake_zat: String,
    /// Attributed share of current stake, formatted with one decimal place.
    pub miner_attributed_percent: String,
    /// Current stake attributed to transparent sources, including non-miners.
    pub transparent_attributed_stake_zat: String,
    /// Transparent-attributed share, formatted with one decimal place.
    pub transparent_attributed_percent: String,
    /// Current stake without an attributable transparent source address.
    pub unattributed_stake_zat: String,
    /// Distinct nonzero transparent address-finalizer pairs before pagination.
    pub pair_count: String,
    /// Current stake grouped by its observable funding source.
    pub sources: CrosslinkStakeSourceBreakdown,
}

/// Cursor metadata for a current miner stake ranking.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkMinerStakePagination {
    pub limit: u32,
    pub total: String,
    pub has_next: bool,
    pub has_prev: bool,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}

/// Current transparent-attributed stake at one best-chain explorer snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkMinerStakeResponse {
    pub items: Vec<CrosslinkMinerStakeEntry>,
    pub summary: CrosslinkMinerStakeSummary,
    pub pagination: CrosslinkMinerStakePagination,
    pub indexed_height: Option<String>,
    pub indexed_block_hash: Option<String>,
}
