//! Crosslink explorer overview response types.

use schemars::JsonSchema;
use serde::Serialize;

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

/// Current BFT roster totals.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
pub struct CrosslinkFinalizersOverview {
    /// Finalizers present in the current roster.
    pub roster_count: String,
    /// Members of the roster that have active voting slots.
    pub active_count: String,
    /// Consensus maximum number of active finalizers.
    pub active_limit: String,
    /// Voting power held by active roster members, in zatoshis.
    pub active_voting_power_zat: String,
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
