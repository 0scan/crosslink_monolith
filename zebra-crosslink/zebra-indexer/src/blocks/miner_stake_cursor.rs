//! Opaque current miner-to-finalizer stake pagination positions.

use std::str::FromStr;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use zebra_chain::{block::Hash, transparent::Address};

use crate::Error;

const FIXED_CURSOR_BYTES: usize = 114;
const MAX_ENCODED_CURSOR_LENGTH: usize = 240;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MinerStakeCursor {
    pub(super) scope_finalizer: Option<[u8; 32]>,
    pub(super) miner_address: Address,
    pub(super) finalizer: [u8; 32],
    pub(super) current_stake_zat: u64,
    pub(super) rank: u64,
    pub(super) indexed_block_hash: Hash,
}

impl MinerStakeCursor {
    pub(super) fn encode(self) -> String {
        let address = self.miner_address.to_string();
        let address_length = u8::try_from(address.len())
            .expect("transparent address encodings are shorter than 256 bytes");
        let mut bytes = Vec::with_capacity(FIXED_CURSOR_BYTES + address.len());
        bytes.push(u8::from(self.scope_finalizer.is_some()));
        bytes.extend_from_slice(&self.scope_finalizer.unwrap_or([0; 32]));
        bytes.push(address_length);
        bytes.extend_from_slice(address.as_bytes());
        bytes.extend_from_slice(&self.finalizer);
        bytes.extend_from_slice(&self.current_stake_zat.to_be_bytes());
        bytes.extend_from_slice(&self.rank.to_be_bytes());
        bytes.extend_from_slice(&self.indexed_block_hash.0);
        URL_SAFE_NO_PAD.encode(bytes)
    }

    pub(super) fn decode(encoded: &str) -> Result<Self, Error> {
        if encoded.len() > MAX_ENCODED_CURSOR_LENGTH {
            return Err(Error::InvalidCursor(
                "miner-stake cursor is too long".to_string(),
            ));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| Error::InvalidCursor("cursor is not valid URL-safe base64".to_string()))?;
        if bytes.len() < FIXED_CURSOR_BYTES {
            return Err(Error::InvalidCursor(
                "miner-stake cursor is too short".to_string(),
            ));
        }
        let scope_finalizer = match bytes[0] {
            0 => None,
            1 => Some(
                bytes[1..33]
                    .try_into()
                    .expect("cursor scope finalizer is 32 bytes"),
            ),
            _ => {
                return Err(Error::InvalidCursor(
                    "miner-stake cursor has an invalid scope".to_string(),
                ))
            }
        };
        let address_length = usize::from(bytes[33]);
        let expected_length = FIXED_CURSOR_BYTES
            .checked_add(address_length)
            .ok_or_else(|| {
                Error::InvalidCursor("miner-stake cursor length overflow".to_string())
            })?;
        if bytes.len() != expected_length {
            return Err(Error::InvalidCursor(
                "miner-stake cursor has an invalid length".to_string(),
            ));
        }

        let address_start = 34;
        let address_end = address_start + address_length;
        let miner_address = std::str::from_utf8(&bytes[address_start..address_end])
            .map_err(|_| Error::InvalidCursor("cursor address is not UTF-8".to_string()))?;
        let miner_address = Address::from_str(miner_address)
            .map_err(|_| Error::InvalidCursor("cursor address is invalid".to_string()))?;
        let finalizer_end = address_end + 32;
        let stake_end = finalizer_end + 8;
        let rank_end = stake_end + 8;
        let finalizer = bytes[address_end..finalizer_end]
            .try_into()
            .expect("cursor finalizer is 32 bytes");
        let current_stake_zat = u64::from_be_bytes(
            bytes[finalizer_end..stake_end]
                .try_into()
                .expect("cursor stake is eight bytes"),
        );
        let rank = u64::from_be_bytes(
            bytes[stake_end..rank_end]
                .try_into()
                .expect("cursor rank is eight bytes"),
        );
        if current_stake_zat == 0 || rank == 0 {
            return Err(Error::InvalidCursor(
                "miner-stake cursor values must be positive".to_string(),
            ));
        }
        let indexed_block_hash = Hash(
            bytes[rank_end..]
                .try_into()
                .expect("cursor block hash is 32 bytes"),
        );

        Ok(Self {
            scope_finalizer,
            miner_address,
            finalizer,
            current_stake_zat,
            rank,
            indexed_block_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use zebra_chain::{block::Hash, parameters::NetworkKind, transparent::Address};

    use super::MinerStakeCursor;

    #[test]
    fn cursor_round_trips_global_and_finalizer_scopes() {
        for scope_finalizer in [None, Some([4; 32])] {
            let cursor = MinerStakeCursor {
                scope_finalizer,
                miner_address: Address::from_pub_key_hash(NetworkKind::Mainnet, [7; 20]),
                finalizer: [5; 32],
                current_stake_zat: 42,
                rank: 3,
                indexed_block_hash: Hash([9; 32]),
            };
            assert_eq!(MinerStakeCursor::decode(&cursor.encode()).unwrap(), cursor);
        }
    }
}
