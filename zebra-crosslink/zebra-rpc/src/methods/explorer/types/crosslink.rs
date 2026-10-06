//! Crosslink explorer overview response types.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use zebra_indexer::PageDirection;

/// Current Crosslink activation phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkPhase {
    /// Proof-of-work mining before staking opens.
    Mining,
    /// Staking is available and contributes to the first roster.
    Staking,
    /// The first finalizer roster is fixed while BFT activation approaches.
    FirstFinalizers,
    /// Crosslink BFT finality is active or starting.
    Finality,
}

/// Current state of Crosslink finality.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkFinalityStatus {
    /// The configured BFT activation height has not been reached.
    NotActivated,
    /// The activation height has been reached, but no finalized block is available yet.
    Starting,
    /// At least one finalized block is available.
    Active,
}

/// Current state of the recurring staking-action window.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkStakingStatus {
    /// The first staking height has not been reached.
    NotStarted,
    /// Staking actions are currently accepted by the consensus calendar.
    Open,
    /// Staking has started, but its current action window is closed.
    Closed,
}

/// The next staking-window boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkStakingChange {
    /// The staking window opens at the boundary.
    Opens,
    /// The staking window closes at the boundary.
    Closes,
}

/// Selection state of the finalizer set shown by the explorer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkFinalizerSetStatus {
    /// Rankings can still change before the configured selection height.
    Projected,
    /// The first finalizer set has been selected, but BFT has not activated yet.
    Selected,
    /// Crosslink BFT is active.
    Active,
}

/// Current Tenderlink consensus step observed by this node.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrosslinkBftStep {
    /// A proposal is being selected or propagated.
    Propose,
    /// Finalizers are sending prevotes.
    Prevote,
    /// Finalizers are sending precommits.
    Precommit,
}

/// Compact miner totals for the overview card.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkMinersOverview {
    /// Distinct attributed miners over the complete indexed chain.
    pub count: String,
    /// Scope of `count`, currently always `all`.
    pub count_scope: String,
    /// Canonical blocks in the trailing 24-hour index window.
    pub blocks_24h: String,
    /// Whether the bounded index scan covered the complete trailing 24 hours.
    pub blocks_24h_complete: bool,
}

/// Current finality state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalityOverview {
    /// Human-readable lifecycle status.
    pub status: CrosslinkFinalityStatus,
    /// Whether the local BFT chain has bootstrapped.
    pub activated: bool,
    /// Height of the latest block finalized by Crosslink.
    pub finalized_height: Option<String>,
    /// Hash of the latest block finalized by Crosslink.
    pub finalized_hash: Option<String>,
    /// Best-chain blocks above the latest Crosslink-finalized block.
    pub lag_blocks: Option<String>,
    /// Required proof-of-work confirmation depth, sigma.
    pub confirmation_depth_blocks: String,
}

/// Current recurring staking window and public staking pools.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkStakingOverview {
    /// Current staking-window status.
    pub status: CrosslinkStakingStatus,
    /// Whether staking actions are accepted at the current height.
    pub window_open: bool,
    /// Inclusive start height of the current staking window, when open.
    pub window_start_height: Option<String>,
    /// Inclusive end height of the current staking window, when open.
    pub window_end_height: Option<String>,
    /// Whether the next staking-window boundary opens or closes the window.
    pub next_change: Option<CrosslinkStakingChange>,
    /// Height at which the next staking-window boundary occurs.
    pub next_change_height: Option<String>,
    /// Blocks until the next staking-window boundary.
    pub blocks_remaining: Option<String>,
    /// Estimated Unix time of the next staking-window boundary.
    pub estimated_at: Option<String>,
    /// Blocks between the starts of consecutive staking windows.
    pub period_blocks: String,
    /// Number of blocks in which staking actions are accepted per period.
    pub window_blocks: String,
    /// Current active delegation-bond pool balance in zatoshis.
    pub bonded_zat: String,
    /// Current released delegation-bond pool balance in zatoshis.
    pub unbonded_zat: String,
    /// Current unconverted finalizer-reward pool balance in zatoshis.
    pub finalizer_rewards_zat: String,
}

/// Compact finalizer totals for the network overview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalizersOverview {
    /// Whether the ranked set is projected, selected, or active.
    pub status: CrosslinkFinalizerSetStatus,
    /// Finalizer public keys that currently have aggregated stake.
    pub candidate_count: String,
    /// Candidates that currently fall within the voting cap.
    pub active_count: String,
    /// Consensus maximum number of active finalizers.
    pub active_limit: String,
    /// Aggregated stake held by the active top-ranked candidates, in zatoshis.
    pub active_stake_zat: String,
    /// Height that fixes the first Crosslink finalizer set.
    pub selection_height: Option<String>,
}

/// One finalizer candidate ranked by current aggregated stake.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalizerEntry {
    /// One-based rank by voting power, with public key as the deterministic tie-breaker.
    pub rank: String,
    /// Finalizer public key encoded for display and stable identity.
    pub public_key: String,
    /// Self-authenticating finalizer address, when one has been revealed on chain.
    pub finalizer_address: Option<String>,
    /// Recognized miner address contributing the most current stake to this finalizer.
    pub primary_miner_address: Option<String>,
    /// Number of distinct recognized miner addresses currently contributing stake.
    pub miner_address_count: String,
    /// Current aggregated voting power in zatoshis.
    pub voting_power_zat: String,
    /// Percentage of all candidate stake, formatted with one decimal place.
    pub total_stake_share_percent: String,
    /// Percentage of active top-ranked stake, or `None` outside the active cap.
    pub active_stake_share_percent: Option<String>,
    /// Whether this candidate currently falls within the active voting cap.
    pub active: bool,
}

/// All finalizer candidates at one best-chain stake snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalizersResponse {
    /// Whether this ranking is projected, selected, or active.
    pub status: CrosslinkFinalizerSetStatus,
    /// Height of the best-chain stake snapshot.
    pub snapshot_height: Option<String>,
    /// Hash of the best-chain stake snapshot.
    pub snapshot_hash: Option<String>,
    /// Height that fixes the first Crosslink finalizer set.
    pub selection_height: Option<String>,
    /// Height at which Crosslink BFT activates.
    pub activation_height: Option<String>,
    /// Finalizer public keys that currently have aggregated stake.
    pub candidate_count: String,
    /// Candidates that currently fall within the voting cap.
    pub active_count: String,
    /// Consensus maximum number of active finalizers.
    pub active_limit: String,
    /// Aggregated stake across all candidates, in zatoshis.
    pub total_stake_zat: String,
    /// Aggregated stake across the active top-ranked candidates, in zatoshis.
    pub active_stake_zat: String,
    /// Complete candidate ranking. The server does not truncate this list.
    pub items: Vec<CrosslinkFinalizerEntry>,
}

/// Parameters for one finalizer detail.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrosslinkFinalizerRequest {
    /// Finalizer public key in the display byte order used by list responses.
    pub public_key: String,
}

/// Parameters for one finalizer's current miner-source page.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CrosslinkFinalizerStakeSourcesRequest {
    /// Finalizer public key in the display byte order used by list responses.
    pub public_key: String,
    /// Maximum number of miner sources to return, from 1 through 100.
    pub limit: Option<u32>,
    /// Opaque cursor returned by a previous page.
    pub cursor: Option<String>,
    /// Direction to move from `cursor`.
    #[serde(default)]
    pub direction: PageDirection,
}

/// Current detail for one finalizer candidate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalizerResponse {
    /// True when the finalizer exists in the current stake snapshot.
    pub available: bool,
    pub finalizer: CrosslinkFinalizerEntry,
    pub status: CrosslinkFinalizerSetStatus,
    pub snapshot_height: Option<String>,
    pub snapshot_hash: Option<String>,
    pub selection_height: Option<String>,
    pub activation_height: Option<String>,
    pub blocks_until_selection: Option<String>,
    pub blocks_until_activation: Option<String>,
}

/// Vote messages observed by this node at its current BFT height.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkVoteSummary {
    /// Yes prevotes observed across all retained rounds at the current height.
    pub prevote_yes_count: String,
    /// Nil prevotes observed across all retained rounds at the current height.
    pub prevote_nil_count: String,
    /// Yes precommits observed across all retained rounds at the current height.
    pub precommit_yes_count: String,
    /// Nil precommits observed across all retained rounds at the current height.
    pub precommit_nil_count: String,
}

/// Node-local finalizer connection and voting observations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalizerLivenessResponse {
    /// Observation scope, currently always `local_node`.
    pub scope: String,
    /// Whether Tenderlink has published a live round-state snapshot.
    pub available: bool,
    /// Unix time at which Tenderlink produced the observation.
    pub observed_at: Option<String>,
    /// Maximum packet age used to classify a finalizer as connected.
    pub connection_window_seconds: String,
    /// Current BFT height observed by this node.
    pub bft_height: Option<String>,
    /// Current BFT round observed by this node.
    pub round: Option<String>,
    /// Current BFT step observed by this node.
    pub step: Option<CrosslinkBftStep>,
    /// Active finalizers considered by the observation.
    pub total_count: String,
    /// Voting power across the active finalizers, in zatoshis.
    pub total_stake_zat: String,
    /// Active finalizers seen through a direct connection within the configured window.
    pub connected_count: String,
    /// Voting power belonging to recently connected finalizers, in zatoshis.
    pub connected_stake_zat: String,
    /// Share of active stake that was recently connected.
    pub connected_stake_percent: String,
    /// Active finalizers not recently seen through a direct connection.
    pub offline_count: String,
    /// Voting power belonging to finalizers not recently connected, in zatoshis.
    pub offline_stake_zat: String,
    /// Distinct active finalizers from which any current-height vote was observed.
    pub voted_count: String,
    /// Voting power belonging to finalizers that voted at the current height.
    pub voted_stake_zat: String,
    /// Share of active stake that voted at the current height.
    pub voted_stake_percent: String,
    /// Active finalizers from which no current-height vote was observed.
    pub silent_count: String,
    /// Voting power belonging to silent finalizers, in zatoshis.
    pub silent_stake_zat: String,
    /// Raw current-height vote-message totals.
    pub votes: CrosslinkVoteSummary,
}

/// One configured Crosslink activation milestone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkActivationMilestone {
    /// Phase that starts at this milestone.
    pub phase: CrosslinkPhase,
    /// Configured activation height.
    pub height: String,
    /// Whether the best chain has reached this height.
    pub reached: bool,
    /// Blocks remaining before this height, or zero once reached.
    pub blocks_remaining: String,
    /// Estimated Unix activation time while the milestone is still upcoming.
    pub estimated_at: Option<String>,
}

/// Crosslink activation progress and configured phase boundaries.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkActivationOverview {
    /// Phase containing the current best-chain height.
    pub current_phase: Option<CrosslinkPhase>,
    /// Progress through the current bounded phase, formatted with one decimal place.
    pub progress_percent: Option<String>,
    /// Staking, first-roster, and finality milestones in height order.
    pub milestones: Vec<CrosslinkActivationMilestone>,
}

/// Crosslink-specific network statistics used by explorer overview cards and activation UI.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkNetworkStats {
    /// Current finality state.
    pub finality: CrosslinkFinalityOverview,
    /// Current staking calendar and public balances.
    pub staking: CrosslinkStakingOverview,
    /// Compact all-time miner and trailing block totals.
    pub miners: CrosslinkMinersOverview,
    /// Current finalizer roster totals.
    pub finalizers: CrosslinkFinalizersOverview,
    /// Activation progress and milestones.
    pub activation: CrosslinkActivationOverview,
}
