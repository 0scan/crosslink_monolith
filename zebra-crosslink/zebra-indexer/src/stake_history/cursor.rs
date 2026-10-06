//! Opaque pagination cursors for staking-action history.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use zebra_chain::{block::Hash, transparent::Address};
use zebra_state::{IntoDisk, TransactionLocation};

use crate::{height_range::TransactionHeightRange, types::CrosslinkStakeAction, Error};

const CURSOR_VERSION: u8 = 1;
const FILTER_BYTES: usize = 89;
const CURSOR_BYTES: usize = 136;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StakeHistoryCursor {
    pub(super) location: TransactionLocation,
    pub(super) block_hash: Hash,
    filter: [u8; FILTER_BYTES],
    height_range: TransactionHeightRange,
}

impl StakeHistoryCursor {
    pub(super) fn new(
        location: TransactionLocation,
        block_hash: Hash,
        address: Option<Address>,
        finalizer: Option<[u8; 32]>,
        bond_key: Option<[u8; 32]>,
        action: Option<CrosslinkStakeAction>,
        height_range: TransactionHeightRange,
    ) -> Self {
        Self {
            location,
            block_hash,
            filter: filter_bytes(address, finalizer, bond_key, action),
            height_range,
        }
    }

    pub(super) fn matches(
        self,
        address: Option<Address>,
        finalizer: Option<[u8; 32]>,
        bond_key: Option<[u8; 32]>,
        action: Option<CrosslinkStakeAction>,
        height_range: TransactionHeightRange,
    ) -> bool {
        self.filter == filter_bytes(address, finalizer, bond_key, action)
            && self.height_range == height_range
    }

    pub(super) fn encode(self) -> String {
        let mut bytes = [0; CURSOR_BYTES];
        bytes[0] = CURSOR_VERSION;
        bytes[1..5].copy_from_slice(&self.location.height.0.to_be_bytes());
        bytes[5..7].copy_from_slice(&self.location.index.index().to_be_bytes());
        bytes[7..39].copy_from_slice(&self.block_hash.0);
        bytes[39..43].copy_from_slice(&self.height_range.from.0.to_be_bytes());
        bytes[43..47].copy_from_slice(&self.height_range.to.0.to_be_bytes());
        bytes[47..].copy_from_slice(&self.filter);
        URL_SAFE_NO_PAD.encode(bytes)
    }

    pub(super) fn decode(encoded: &str) -> Result<Self, Error> {
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::InvalidCursor("cursor is not valid URL-safe base64".to_string()))?;
        if bytes.len() != CURSOR_BYTES || bytes[0] != CURSOR_VERSION {
            return Err(Error::InvalidCursor(
                "stake-history cursor has an invalid version or length".to_string(),
            ));
        }

        Ok(Self {
            location: TransactionLocation::from_index(
                zebra_chain::block::Height(u32::from_be_bytes(
                    bytes[1..5].try_into().expect("cursor height is four bytes"),
                )),
                u16::from_be_bytes(
                    bytes[5..7]
                        .try_into()
                        .expect("cursor transaction index is two bytes"),
                ),
            ),
            block_hash: Hash(
                bytes[7..39]
                    .try_into()
                    .expect("cursor block hash is 32 bytes"),
            ),
            height_range: TransactionHeightRange {
                from: zebra_chain::block::Height(u32::from_be_bytes(
                    bytes[39..43]
                        .try_into()
                        .expect("cursor from height is four bytes"),
                )),
                to: zebra_chain::block::Height(u32::from_be_bytes(
                    bytes[43..47]
                        .try_into()
                        .expect("cursor to height is four bytes"),
                )),
            },
            filter: bytes[47..]
                .try_into()
                .expect("cursor filter has a fixed width"),
        })
    }
}

fn filter_bytes(
    address: Option<Address>,
    finalizer: Option<[u8; 32]>,
    bond_key: Option<[u8; 32]>,
    action: Option<CrosslinkStakeAction>,
) -> [u8; FILTER_BYTES] {
    let mut bytes = [0; FILTER_BYTES];
    bytes[0] = match action {
        None => 0,
        Some(CrosslinkStakeAction::Create) => 1,
        Some(CrosslinkStakeAction::BeginUnbonding) => 2,
        Some(CrosslinkStakeAction::Withdraw) => 3,
        Some(CrosslinkStakeAction::Retarget) => 4,
        Some(CrosslinkStakeAction::ConvertReward) => 5,
    };
    if let Some(address) = address {
        bytes[1] = 1;
        bytes[2..23].copy_from_slice(&address.as_bytes());
    }
    if let Some(finalizer) = finalizer {
        bytes[23] = 1;
        bytes[24..56].copy_from_slice(&finalizer);
    }
    if let Some(bond_key) = bond_key {
        bytes[56] = 1;
        bytes[57..89].copy_from_slice(&bond_key);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use zebra_chain::{block::Height, parameters::NetworkKind};

    use super::*;

    #[test]
    fn cursor_round_trips_and_is_bound_to_filters() {
        let address = Address::from_pub_key_hash(NetworkKind::Testnet, [3; 20]);
        let range = TransactionHeightRange::new(10..=100).unwrap();
        let cursor = StakeHistoryCursor::new(
            TransactionLocation::from_index(Height(42), 7),
            Hash([9; 32]),
            Some(address),
            Some([4; 32]),
            Some([5; 32]),
            Some(CrosslinkStakeAction::Retarget),
            range,
        );
        let decoded = StakeHistoryCursor::decode(&cursor.encode()).unwrap();
        assert_eq!(decoded, cursor);
        assert!(decoded.matches(
            Some(address),
            Some([4; 32]),
            Some([5; 32]),
            Some(CrosslinkStakeAction::Retarget),
            range,
        ));
        assert!(!decoded.matches(
            Some(address),
            Some([4; 32]),
            Some([6; 32]),
            Some(CrosslinkStakeAction::Retarget),
            range,
        ));
    }
}
