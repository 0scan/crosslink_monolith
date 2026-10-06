//! Opaque all-time miner ranking pagination positions.

use std::str::FromStr;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use zebra_chain::{block::Hash, transparent::Address};

use crate::Error;

const BLOCK_COUNT_RANK_AND_HASH_BYTES: usize = 48;
const MAX_ENCODED_CURSOR_LENGTH: usize = 160;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MinerCursor {
    pub(super) address: Address,
    pub(super) block_count: u64,
    pub(super) rank: u64,
    pub(super) indexed_block_hash: Hash,
}

impl MinerCursor {
    pub(super) fn new(
        address: Address,
        block_count: u64,
        rank: u64,
        indexed_block_hash: Hash,
    ) -> Self {
        Self {
            address,
            block_count,
            rank,
            indexed_block_hash,
        }
    }

    pub(super) fn encode(self) -> String {
        let address = self.address.to_string();
        let address_length = u8::try_from(address.len())
            .expect("transparent address encodings are shorter than 256 bytes");
        let mut bytes = Vec::with_capacity(1 + address.len() + BLOCK_COUNT_RANK_AND_HASH_BYTES);
        bytes.push(address_length);
        bytes.extend_from_slice(address.as_bytes());
        bytes.extend_from_slice(&self.block_count.to_be_bytes());
        bytes.extend_from_slice(&self.rank.to_be_bytes());
        bytes.extend_from_slice(&self.indexed_block_hash.0);
        URL_SAFE_NO_PAD.encode(bytes)
    }

    pub(super) fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > MAX_ENCODED_CURSOR_LENGTH {
            return Err(Error::InvalidCursor(
                "top-miners cursor is too long".to_string(),
            ));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::InvalidCursor("cursor is not valid URL-safe base64".to_string()))?;
        let address_length = bytes
            .first()
            .copied()
            .map(usize::from)
            .ok_or_else(|| Error::InvalidCursor("top-miners cursor is empty".to_string()))?;
        let expected_length = 1_usize
            .checked_add(address_length)
            .and_then(|length| length.checked_add(BLOCK_COUNT_RANK_AND_HASH_BYTES))
            .ok_or_else(|| Error::InvalidCursor("top-miners cursor length overflow".to_string()))?;
        if bytes.len() != expected_length {
            return Err(Error::InvalidCursor(
                "top-miners cursor has an invalid length".to_string(),
            ));
        }

        let address_end = 1 + address_length;
        let address = std::str::from_utf8(&bytes[1..address_end])
            .map_err(|_| Error::InvalidCursor("cursor address is not UTF-8".to_string()))?;
        let address = Address::from_str(address)
            .map_err(|_| Error::InvalidCursor("cursor address is invalid".to_string()))?;
        let block_count_end = address_end + 8;
        let rank_end = block_count_end + 8;
        let block_count = u64::from_be_bytes(
            bytes[address_end..block_count_end]
                .try_into()
                .map_err(|_| Error::InvalidCursor("cursor block count is invalid".to_string()))?,
        );
        let rank = u64::from_be_bytes(
            bytes[block_count_end..rank_end]
                .try_into()
                .map_err(|_| Error::InvalidCursor("cursor rank is invalid".to_string()))?,
        );
        if block_count == 0 || rank == 0 {
            return Err(Error::InvalidCursor(
                "top-miners cursor counts must be positive".to_string(),
            ));
        }
        let indexed_block_hash =
            Hash(bytes[rank_end..].try_into().map_err(|_| {
                Error::InvalidCursor("cursor block hash must be 32 bytes".to_string())
            })?);

        Ok(Self {
            address,
            block_count,
            rank,
            indexed_block_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use zebra_chain::{block::Hash, parameters::NetworkKind, transparent::Address};

    use super::MinerCursor;

    #[test]
    fn cursor_round_trips_ranking_position_and_generation() {
        let cursor = MinerCursor::new(
            Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]),
            42,
            3,
            Hash([9; 32]),
        );

        assert_eq!(MinerCursor::decode(&cursor.encode()).unwrap(), cursor);
    }

    #[test]
    fn cursor_rejects_malformed_input() {
        assert!(MinerCursor::decode("not-a-valid-cursor").is_err());
    }
}
