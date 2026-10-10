//! Signer presence in the certificates carried by a bounded PoW block window.

use std::collections::{BTreeSet, HashMap, HashSet};
use zebra_chain::block::{Hash, Height};

/// First canonical PoW header carrying a non-empty certificate for this decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CertificateInclusion {
    pub block_height: Height,
    pub block_hash: Hash,
    pub block_time: i64,
}

/// Matches BFT hashes, rather than heights which are insufficient to identify a decision.
#[derive(Debug)]
pub struct CertificateInclusions {
    pending: HashSet<[u8; 32]>,
    pub items: HashMap<[u8; 32], CertificateInclusion>,
}

impl CertificateInclusions {
    pub fn new(hashes: impl IntoIterator<Item = [u8; 32]>) -> Self {
        Self {
            pending: hashes.into_iter().collect(),
            items: HashMap::new(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.pending.is_empty()
    }

    /// Headers must arrive in canonical height order; repeated references never change the time.
    pub fn observe(
        &mut self,
        hash: [u8; 32],
        has_signatures: bool,
        inclusion: CertificateInclusion,
    ) {
        if has_signatures && inclusion.block_time > 0 && self.pending.remove(&hash) {
            self.items.insert(hash, inclusion);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PowVotingBlock {
    pub height: Height,
    pub hash: Hash,
    pub bft_height: Option<u64>,
    pub signers: BTreeSet<[u8; 32]>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PowVotingWindow {
    /// One canonical chain snapshot, oldest first. Null certificates still count.
    pub blocks: Vec<PowVotingBlock>,
}

pub struct PowVotingSummary {
    pub public_key: [u8; 32],
    pub sampled: usize,
    pub signed: usize,
    pub recent: Vec<(PowVotingBlock, bool)>,
}

impl PowVotingWindow {
    pub fn summaries(&self, keys: &[[u8; 32]]) -> Vec<PowVotingSummary> {
        keys.iter()
            .map(|key| PowVotingSummary {
                public_key: *key,
                sampled: self.blocks.len(),
                // Each PoW block counts separately, including repeated certificates.
                signed: self
                    .blocks
                    .iter()
                    .filter(|block| block.signers.contains(key))
                    .count(),
                recent: self
                    .blocks
                    .iter()
                    .rev()
                    .take(50)
                    .rev()
                    .map(|block| (block.clone(), block.signers.contains(key)))
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_times_use_the_first_matching_signed_header_not_repeated_references() {
        let mut index = CertificateInclusions::new([[7; 32], [8; 32]]);
        let first = CertificateInclusion {
            block_height: Height(36_292),
            block_hash: Hash([1; 32]),
            block_time: 1_791_623_457,
        };
        index.observe([7; 32], false, first.clone());
        index.observe([9; 32], true, first.clone());
        assert!(index.items.is_empty());
        index.observe([7; 32], true, first.clone());
        index.observe(
            [7; 32],
            true,
            CertificateInclusion {
                block_height: Height(37_067),
                block_hash: Hash([2; 32]),
                block_time: 1_791_643_000,
            },
        );
        assert_eq!(index.items.get(&[7; 32]), Some(&first));
        assert!(!index.is_complete());
        index.observe([8; 32], true, first);
        assert!(index.is_complete());
    }

    #[test]
    fn pow_voting_matches_270_of_500_without_deduplicating_certificates() {
        let blocks = (0..500)
            .map(|index| PowVotingBlock {
                height: Height(36_079 + index),
                hash: Hash([1; 32]),
                bft_height: (index >= 209).then_some(7),
                signers: if index >= 230 {
                    BTreeSet::from([[1; 32]])
                } else {
                    BTreeSet::new()
                },
            })
            .collect();
        let window = PowVotingWindow { blocks };
        let summaries = window.summaries(&[[1; 32], [2; 32]]);
        assert_eq!((summaries[0].signed, summaries[0].sampled), (270, 500));
        assert_eq!((summaries[1].signed, summaries[1].sampled), (0, 500));
        assert_eq!(summaries[0].recent.len(), 50);
        assert_eq!(
            summaries[0].recent.first().unwrap().0.height,
            Height(36_529)
        );
        assert_eq!(summaries[0].recent.last().unwrap().0.height, Height(36_578));
    }

    #[test]
    fn partial_or_empty_windows_do_not_invent_missing_blocks() {
        let window = PowVotingWindow {
            blocks: vec![PowVotingBlock {
                height: Height(0),
                hash: Hash([0; 32]),
                bft_height: None,
                signers: BTreeSet::new(),
            }],
        };
        let result = window.summaries(&[[1; 32]]);
        assert_eq!((result[0].signed, result[0].sampled), (0, 1));
        assert_eq!(result[0].recent.len(), 1);
        assert_eq!(
            PowVotingWindow::default().summaries(&[[1; 32]])[0].sampled,
            0
        );
    }
}
