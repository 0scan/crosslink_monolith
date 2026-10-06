//! Explorer-facing Crosslink staking-action history contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::PageDirection;

/// A Crosslink staking action kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkStakeAction {
    Create,
    BeginUnbonding,
    Withdraw,
    Retarget,
    ConvertReward,
}

/// Observable origin of the value used to create a delegation bond.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkStakeHistorySource {
    Transparent,
    Shielded,
    Unknown,
    FinalizerRewards,
}

/// Parameters for `getcrosslinkstakehistory`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct CrosslinkStakeHistoryRequest {
    /// Transparent address that originally funded the bond.
    pub address: Option<String>,
    /// Match actions entering or leaving this finalizer public key.
    pub finalizer_public_key: Option<String>,
    /// Match one delegation bond public key.
    pub bond_key: Option<String>,
    /// Match one staking action kind.
    pub action: Option<CrosslinkStakeAction>,
    /// Inclusive minimum block height.
    pub from_height: Option<u32>,
    /// Inclusive maximum block height.
    pub to_height: Option<u32>,
    /// Maximum number of entries to return, from 1 through 100.
    pub limit: Option<u32>,
    /// Opaque cursor returned by a previous page.
    pub cursor: Option<String>,
    /// Direction to move from `cursor`.
    pub direction: PageDirection,
}

/// One finalized Crosslink staking action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakeHistoryEntry {
    pub txid: String,
    pub action: CrosslinkStakeAction,
    pub bond_key: String,
    /// Present only when the original bond source is one unambiguous transparent address.
    pub address: Option<String>,
    pub source_type: CrosslinkStakeHistorySource,
    pub from_finalizer_public_key: Option<String>,
    pub to_finalizer_public_key: Option<String>,
    /// Bond value affected by the action, or `null` only if canonical attribution is unavailable.
    pub amount_zat: Option<String>,
    pub block_height: String,
    pub block_hash: String,
    pub block_time: String,
    pub transaction_index: u32,
}

/// Cursor metadata for a staking-action history page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakeHistoryPagination {
    pub limit: u32,
    pub has_next: bool,
    pub has_prev: bool,
    pub next_cursor: Option<String>,
    pub prev_cursor: Option<String>,
}

/// A newest-first page of finalized Crosslink staking actions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakeHistoryResponse {
    pub items: Vec<CrosslinkStakeHistoryEntry>,
    pub pagination: CrosslinkStakeHistoryPagination,
    pub indexed_height: Option<String>,
    pub indexed_block_hash: Option<String>,
}
