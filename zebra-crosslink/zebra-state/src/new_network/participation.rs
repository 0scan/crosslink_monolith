//! Certificate participation, rebuilt from persisted decisions on startup.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use tenderlink::SortedRosterMember;
use zcash_primitives::bft::{BftBlock, FatPointerToBftBlock, ACTIVE_ROSTER_MAX_N};
use zebra_chain::block::{Hash, Height};

/// A distinct certified BFT decision, rather than each PoW block carrying it.
#[derive(Clone, Debug)]
pub struct SignedBlock {
    pub bft_height: u32,
    /// Hash of the signed BFT decision, used to match its on-chain certificate.
    pub certificate_hash: [u8; 32],
    pub block_height: Height,
    pub block_hash: Hash,
    /// Time of the finalized PoW block, not a claimed signature timestamp.
    pub block_time: Option<i64>,
    /// When this node first accepted the certificate; unknown for legacy decisions.
    pub certificate_observed_at: Option<i64>,
}

#[derive(Debug)]
struct Decision {
    block: SignedBlock,
    eligible: BTreeSet<[u8; 32]>,
    signers: BTreeSet<[u8; 32]>,
}

/// Observational index only: it never participates in consensus validation.
#[derive(Debug, Default)]
pub struct ParticipationIndex {
    decisions: BTreeMap<u32, Decision>,
    signed_heights: HashMap<[u8; 32], BTreeSet<u32>>,
    /// The filtered, capped roster actually handed to Tenderlink.
    pub current_roster: Vec<([u8; 32], u64)>,
}

impl ParticipationIndex {
    pub fn set_roster(&mut self, roster: &[SortedRosterMember]) {
        self.current_roster = roster
            .iter()
            .take(ACTIVE_ROSTER_MAX_N)
            .map(|m| (m.pub_key.0, m.stake))
            .collect();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        block: &BftBlock,
        pointer: &FatPointerToBftBlock,
        roster: &[SortedRosterMember],
        block_height: Height,
        block_hash: Hash,
        block_time: Option<i64>,
        certificate_observed_at: Option<i64>,
    ) {
        // Bootstrap genesis has no voting roster or signatures.
        if block.height == 0
            || block.headers.is_empty()
            || self.decisions.contains_key(&block.height)
        {
            return;
        }
        let eligible: BTreeSet<_> = roster
            .iter()
            .take(ACTIVE_ROSTER_MAX_N)
            .map(|m| m.pub_key.0)
            .collect();
        let signers: BTreeSet<_> = pointer
            .signatures
            .iter()
            .map(|s| s.pub_key.0)
            .filter(|key| eligible.contains(key))
            .collect();
        self.insert(
            SignedBlock {
                bft_height: block.height,
                certificate_hash: pointer.points_at_block_hash().0,
                block_height,
                block_hash,
                block_time,
                certificate_observed_at,
            },
            eligible,
            signers,
        );
    }

    fn insert(
        &mut self,
        block: SignedBlock,
        eligible: BTreeSet<[u8; 32]>,
        signers: BTreeSet<[u8; 32]>,
    ) {
        if block.bft_height == 0 || self.decisions.contains_key(&block.bft_height) {
            return;
        }
        let signers: BTreeSet<_> = signers.intersection(&eligible).copied().collect();
        for key in &signers {
            self.signed_heights
                .entry(*key)
                .or_default()
                .insert(block.bft_height);
        }
        self.decisions.insert(
            block.bft_height,
            Decision {
                block,
                eligible,
                signers,
            },
        );
    }

    pub fn window(&self, keys: &[[u8; 32]], window: usize) -> ParticipationWindow {
        let decisions: Vec<_> = self.decisions.values().rev().take(window).collect();
        ParticipationWindow {
            from_height: decisions.last().map(|d| d.block.bft_height),
            to_height: decisions.first().map(|d| d.block.bft_height),
            observed_blocks: decisions.len(),
            items: keys
                .iter()
                .map(|key| {
                    let eligible = decisions
                        .iter()
                        .filter(|d| d.eligible.contains(key))
                        .count();
                    let signed = decisions.iter().filter(|d| d.signers.contains(key)).count();
                    ParticipationSummary {
                        public_key: *key,
                        recent: decisions
                            .iter()
                            .take(50)
                            .rev()
                            .map(|decision| RecentParticipation {
                                block: decision.block.clone(),
                                eligible: decision.eligible.contains(key),
                                signed: decision.signers.contains(key),
                            })
                            .collect(),
                        eligible,
                        signed,
                        last_signed: self
                            .signed_heights
                            .get(key)
                            .and_then(|heights| heights.last())
                            .and_then(|height| self.decisions.get(height))
                            .map(|d| d.block.clone()),
                    }
                })
                .collect(),
        }
    }

    /// Stable newest-first pages. A cursor excludes its own height, so new decisions
    /// cannot duplicate rows already returned.
    pub fn signed_blocks(
        &self,
        key: [u8; 32],
        before: Option<u32>,
        limit: usize,
    ) -> (Vec<SignedBlock>, bool) {
        let Some(heights) = self.signed_heights.get(&key) else {
            return (Vec::new(), false);
        };
        let mut matches = heights.range(..before.unwrap_or(u32::MAX)).rev();
        let items = matches
            .by_ref()
            .take(limit)
            .filter_map(|height| self.decisions.get(height).map(|d| d.block.clone()))
            .collect();
        (items, matches.next().is_some())
    }
}

#[derive(Debug)]
pub struct ParticipationWindow {
    pub from_height: Option<u32>,
    pub to_height: Option<u32>,
    pub observed_blocks: usize,
    pub items: Vec<ParticipationSummary>,
}

#[derive(Debug)]
pub struct RecentParticipation {
    pub block: SignedBlock,
    pub eligible: bool,
    pub signed: bool,
}

#[derive(Debug)]
pub struct ParticipationSummary {
    pub recent: Vec<RecentParticipation>,
    pub public_key: [u8; 32],
    pub eligible: usize,
    pub signed: usize,
    pub last_signed: Option<SignedBlock>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(index: &mut ParticipationIndex, height: u32, roster: &[u8], signers: &[u8]) {
        index.insert(
            SignedBlock {
                bft_height: height,
                certificate_hash: [1; 32],
                block_height: Height(height),
                block_hash: Hash([1; 32]),
                block_time: None,
                certificate_observed_at: None,
            },
            roster.iter().map(|key| [*key; 32]).collect(),
            signers.iter().map(|key| [*key; 32]).collect(),
        );
    }

    #[test]
    fn participation_counts_only_eligible_decisions_and_excludes_genesis() {
        let mut index = ParticipationIndex::default();
        record(&mut index, 0, &[], &[]);
        record(&mut index, 1, &[1], &[1, 1]);
        record(&mut index, 2, &[1, 2], &[1]);
        record(&mut index, 3, &[2], &[2]);
        let window = index.window(&[[1; 32], [2; 32], [3; 32]], 500);
        assert_eq!(window.observed_blocks, 3);
        assert_eq!(
            window.items[0]
                .recent
                .iter()
                .map(|d| d.block.bft_height)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(window.items[0].recent[0].signed);
        assert!(!window.items[0].recent[2].eligible);
        assert!(!window.items[1].recent[1].signed);
        assert!(window.items[1].recent[1].eligible);
        assert_eq!((window.items[0].signed, window.items[0].eligible), (2, 2));
        assert_eq!((window.items[1].signed, window.items[1].eligible), (1, 2));
        assert_eq!((window.items[2].signed, window.items[2].eligible), (0, 0));
        assert_eq!(index.window(&[[1; 32]], 1).items[0].eligible, 0);
        assert_eq!(
            index.window(&[[1; 32]], 1).items[0]
                .last_signed
                .as_ref()
                .unwrap()
                .bft_height,
            2
        );
    }

    #[test]
    fn certificate_time_is_preserved_and_legacy_time_stays_unknown() {
        let mut index = ParticipationIndex::default();
        index.insert(
            SignedBlock {
                bft_height: 1,
                certificate_hash: [1; 32],
                block_height: Height(34_840),
                block_hash: Hash([1; 32]),
                block_time: Some(1_791_586_956),
                certificate_observed_at: Some(1_791_623_457),
            },
            BTreeSet::from([[1; 32]]),
            BTreeSet::from([[1; 32]]),
        );
        let result = index.window(&[[1; 32]], 500);
        let block = result.items[0].last_signed.as_ref().unwrap();
        assert_eq!(block.certificate_observed_at, Some(1_791_623_457));
        assert_eq!(block.block_time, Some(1_791_586_956));
        record(&mut index, 2, &[1], &[1]);
        assert_eq!(
            index.window(&[[1; 32]], 500).items[0]
                .last_signed
                .as_ref()
                .unwrap()
                .certificate_observed_at,
            None
        );
    }

    #[test]
    fn recent_decisions_are_bounded_and_follow_the_requested_window() {
        let mut index = ParticipationIndex::default();
        for height in 1..=60 {
            record(&mut index, height, &[1], &[1]);
        }
        let window = index.window(&[[1; 32]], 500);
        assert_eq!(window.items[0].recent.len(), 50);
        assert_eq!(window.items[0].recent.first().unwrap().block.bft_height, 11);
        assert_eq!(window.items[0].recent.last().unwrap().block.bft_height, 60);
        assert_eq!(index.window(&[[1; 32]], 5).items[0].recent.len(), 5);
    }

    #[test]
    fn signed_pages_do_not_repeat_certificates_or_shift_after_new_decisions() {
        let mut index = ParticipationIndex::default();
        for height in 1..=5 {
            record(&mut index, height, &[1], &[1]);
        }
        record(&mut index, 5, &[1], &[1]);
        let (first, more) = index.signed_blocks([1; 32], None, 2);
        assert!(more);
        assert_eq!(
            first.iter().map(|b| b.bft_height).collect::<Vec<_>>(),
            vec![5, 4]
        );
        record(&mut index, 6, &[1], &[1]);
        let (next, more) = index.signed_blocks([1; 32], Some(4), 2);
        assert!(more);
        assert_eq!(
            next.iter().map(|b| b.bft_height).collect::<Vec<_>>(),
            vec![3, 2]
        );
        assert_eq!(index.signed_blocks([2; 32], None, 20).0.len(), 0);
    }
}
