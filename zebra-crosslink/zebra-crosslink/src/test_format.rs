use static_assertions::*;
use std::{io::Write, mem::align_of, mem::size_of};
use zebra_chain::serialization::{ZcashDeserialize, ZcashSerialize, SerializationError};
use zebra_chain::transaction::{Transaction, UnminedTxId};
use zerocopy::*;
use zerocopy_derive::*;

use zcash_primitives::bft::*;

pub struct BftBlockAndFatPointerToItWrap(pub BftBlockAndFatPointerToIt);
impl ZcashDeserialize for BftBlockAndFatPointerToItWrap {
    fn zcash_deserialize<R: std::io::Read>(mut reader: R) -> Result<Self, SerializationError> { // SerializationError> {
        Ok(Self(BftBlockAndFatPointerToIt::zcash_deserialize(&mut reader)?))
    }
}
impl ZcashSerialize for BftBlockAndFatPointerToItWrap {
    fn zcash_serialize<W: std::io::Write>(&self, mut writer: W) -> Result<(), std::io::Error> {
        self.0.zcash_serialize(&mut writer)?;
        Ok(())
    }
}


#[repr(C)]
#[derive(Immutable, KnownLayout, IntoBytes, FromBytes)]
pub struct TFHdr {
    pub magic: [u8; 8],
    pub instrs_o: u64,
    pub instrs_n: u32,
    pub instr_size: u32, // used as stride
}

#[repr(C)]
#[derive(Clone, Copy, Immutable, IntoBytes, FromBytes)]
pub struct TFSlice {
    pub o: u64,
    pub size: u64,
}

impl TFSlice {
    pub fn as_val(self) -> [u64; 2] {
        [self.o, self.size]
    }

    pub fn as_byte_slice_in(self, bytes: &[u8]) -> &[u8] {
        &bytes[self.o as usize..(self.o + self.size) as usize]
    }
}

impl From<&[u64; 2]> for TFSlice {
    fn from(val: &[u64; 2]) -> TFSlice {
        TFSlice {
            o: val[0],
            size: val[1],
        }
    }
}

type TFInstrKind = u32;

#[repr(C)]
#[derive(Clone, Copy, Immutable, IntoBytes, FromBytes)]
pub struct TFInstr {
    pub kind: TFInstrKind,
    pub flags: u32,
    pub data: TFSlice,
    pub val: [u64; 2],
}

/// A value an EXPECT_* instruction should not check.
pub const TEST_STAKE_IGNORED: u64 = u64::MAX;

/// Bond status codes for EXPECT_BOND, as the state reports them, plus one for "no such bond".
pub const TF_BOND_ACTIVE: u64 = 0;
pub const TF_BOND_UNBONDING: u64 = 1;
pub const TF_BOND_WITHDRAWN: u64 = 2;
pub const TF_BOND_BURNED: u64 = 3;
pub const TF_BOND_ABSENT: u64 = 255;

static TF_INSTR_KIND_STRS: [&str; TFInstr::COUNT as usize] = {
    let mut strs = [""; TFInstr::COUNT as usize];
    strs[TFInstr::LOAD_POW as usize] = "LOAD_POW";
    strs[TFInstr::LOAD_POS as usize] = "LOAD_POS";
    strs[TFInstr::SET_PARAMS as usize] = "SET_PARAMS";
    strs[TFInstr::EXPECT_POW_CHAIN_LENGTH as usize] = "EXPECT_POW_CHAIN_LENGTH";
    strs[TFInstr::EXPECT_POS_CHAIN_LENGTH as usize] = "EXPECT_POS_CHAIN_LENGTH";
    strs[TFInstr::EXPECT_POW_BLOCK_FINALITY as usize] = "EXPECT_POW_BLOCK_FINALITY";
    strs[TFInstr::ROSTER_FORCE_INCLUDE as usize] = "ROSTER_FORCE_INCLUDE";
    strs[TFInstr::EXPECT_ROSTER_INCLUDES as usize] = "EXPECT_ROSTER_INCLUDES";
    strs[TFInstr::EXPECT_REJECTION_REASON as usize] = "EXPECT_REJECTION_REASON";
    strs[TFInstr::EXPECT_NODE_ALIVE as usize] = "EXPECT_NODE_ALIVE";
    strs[TFInstr::RECV_TX as usize] = "RECV_TX";
    strs[TFInstr::SUBMIT_TX as usize] = "SUBMIT_TX";
    strs[TFInstr::EXPECT_MEMPOOL_CONTAINS as usize] = "EXPECT_MEMPOOL_CONTAINS";
    strs[TFInstr::EXPECT_MEMPOOL_ABSENT as usize] = "EXPECT_MEMPOOL_ABSENT";
    strs[TFInstr::EXPECT_MEMPOOL_REJECTED as usize] = "EXPECT_MEMPOOL_REJECTED";
    strs[TFInstr::EXPECT_BOND as usize] = "EXPECT_BOND";
    strs[TFInstr::EXPECT_FINALIZER_BANK as usize] = "EXPECT_FINALIZER_BANK";
    strs[TFInstr::EXPECT_POOL_TOTALS as usize] = "EXPECT_POOL_TOTALS";
    strs[TFInstr::RECV_POW as usize] = "RECV_POW";
    strs[TFInstr::RECV_STP_PACKET as usize] = "RECV_STP_PACKET";
    strs[TFInstr::MINE_FROM_TEMPLATE as usize] = "MINE_FROM_TEMPLATE";

    const_assert!(TFInstr::COUNT == 21);
    strs
};

impl TFInstr {
    // NOTE: we want to deal with unknown values at the *application* layer, not the
    // (de)serialization layer.
    // TODO: there may be a crate that makes an enum feasible here
    pub const LOAD_POW: TFInstrKind = 0;
    pub const LOAD_POS: TFInstrKind = 1;
    pub const SET_PARAMS: TFInstrKind = 2;
    pub const EXPECT_POW_CHAIN_LENGTH: TFInstrKind = 3;
    pub const EXPECT_POS_CHAIN_LENGTH: TFInstrKind = 4;
    pub const EXPECT_POW_BLOCK_FINALITY: TFInstrKind = 5;
    pub const ROSTER_FORCE_INCLUDE: TFInstrKind = 6;
    pub const EXPECT_ROSTER_INCLUDES: TFInstrKind = 7;
    /// The previous instruction was rejected, and its reason contains this instruction's data
    /// (UTF-8). `SHOULD_FAIL` alone passes on any rejection, which is how a test passes for the
    /// wrong reason; this pins which rejection it was.
    pub const EXPECT_REJECTION_REASON: TFInstrKind = 8;
    /// The node still answers: the state service returns its tip within a bound, and
    /// re-submitting the tip block through the ingest path answers `Known`.
    pub const EXPECT_NODE_ALIVE: TFInstrKind = 9;
    /// A wire message (the data, header included) arriving from synthetic peer `val[0]`: decoded
    /// with the peer codec and routed through the inbound service exactly as a connection routes
    /// an unsolicited message. A `tx` message reaches the mempool as that peer's transaction.
    /// Checks only that it was delivered; verification is asynchronous, so follow it with an
    /// EXPECT_MEMPOOL_* instruction.
    pub const RECV_TX: TFInstrKind = 10;
    /// A transaction (the data) submitted locally, the doorway the wallet and RPC use. Checks the
    /// mempool's verdict, and its message is the rejection reason when rejected.
    pub const SUBMIT_TX: TFInstrKind = 11;
    /// The transaction (the data) becomes resident in the mempool within a bound. Fails early if
    /// it is rejected instead.
    pub const EXPECT_MEMPOOL_CONTAINS: TFInstrKind = 12;
    /// The transaction (the data) is not resident in the mempool, or stops being so within a
    /// bound: after it is mined, or evicted as a conflict. It holds at once for a transaction the
    /// mempool never had, so follow an EXPECT_MEMPOOL_CONTAINS with it to check an eviction.
    pub const EXPECT_MEMPOOL_ABSENT: TFInstrKind = 13;
    /// The transaction (the data) is in the mempool's rejected set within a bound.
    pub const EXPECT_MEMPOOL_REJECTED: TFInstrKind = 14;
    /// The bond whose key is the data has status `val[1]` (a `TF_BOND_*` code, `TF_BOND_ABSENT`
    /// for none) and amount `val[0]` in zatoshis, at the best chain tip. `TEST_STAKE_IGNORED`
    /// skips either.
    pub const EXPECT_BOND: TFInstrKind = 15;
    /// The reward bank of the finalizer whose public key is the data holds `val[0]` zatoshis.
    pub const EXPECT_FINALIZER_BANK: TFInstrKind = 16;
    /// The best chain tip's staking pools: the data is bonded, unbonded and finalizer rewards,
    /// three little-endian u64 zatoshi amounts, each skipped when `TEST_STAKE_IGNORED`.
    pub const EXPECT_POOL_TOTALS: TFInstrKind = 17;
    /// A PoW block (the data, serialized) served by synthetic STP peer `val[0]` the way a real
    /// peer serves one: the peer advertises it in a STATUS, waits for the node's BLOCK_REQ, and
    /// answers with BLOCK_CHUNK packets from the requested offset. Accepted and rejected as
    /// LOAD_POW is, and rejected too when the node kills the peer or never asks for the block.
    pub const RECV_POW: TFInstrKind = 18;
    /// An STP application packet (the data, type byte first) arriving from synthetic peer
    /// `val[0]`. Fails when the node kills the peer for it; any other handling passes.
    pub const RECV_STP_PACKET: TFInstrKind = 19;
    /// The node builds a block from its own getblocktemplate response, as its miner does, and
    /// the block is submitted through the LOAD_POW doorway. Accepted and rejected as LOAD_POW is;
    /// also rejected when no template or block could be made.
    pub const MINE_FROM_TEMPLATE: TFInstrKind = 20;
    pub const COUNT: TFInstrKind = 21;

    pub fn str_from_kind(kind: TFInstrKind) -> &'static str {
        let kind = kind as usize;
        if kind < TF_INSTR_KIND_STRS.len() {
            TF_INSTR_KIND_STRS[kind]
        } else {
            "<unknown>"
        }
    }

    pub fn string_from_instr(bytes: &[u8], instr: &TFInstr) -> String {
        let mut str = Self::str_from_kind(instr.kind).to_string();
        str += " (";

        match tf_read_instr(&bytes, instr) {
            Some(TestInstr::LoadPoW(block)) => {
                str += &format!(
                    "{} - {}, parent: {}",
                    block.coinbase_height().unwrap().0,
                    block.hash(),
                    block.header.previous_block_hash
                )
            }
            Some(TestInstr::LoadPoS((block, fat_ptr))) => {
                str += &format!(
                    "{}, snapshot: {}, hdrs: [{} .. {}]",
                    block.blake3_hash(),
                    block.snapshot_block_hash(),
                    BlockHash::from_header_data(&block.headers[0]),
                    BlockHash::from_header_data(block.headers.last().unwrap())
                )
            }
            Some(TestInstr::SetParams(params)) => {
                str += &format!(
                    "{} {:?}",
                    params.bc_confirmation_depth_sigma, params.bootstrap
                )
            }
            Some(TestInstr::ExpectPoWChainLength(h)) => str += &h.to_string(),
            Some(TestInstr::ExpectPoSChainLength(h)) => str += &h.to_string(),
            Some(TestInstr::ExpectPoWBlockFinality(hash, f)) => {
                str += &format!("{} => {:?}", hash, f)
            }
            Some(TestInstr::ExpectRosterIncludes(pub_key, stake)) => {
                str += &format!("{} => {}", PubKeyID(pub_key), stake)
            }
            Some(TestInstr::RosterForceInclude(pub_key, stake)) => {
                str += &format!("{} => {}", PubKeyID(pub_key), stake)
            }
            Some(TestInstr::ExpectRejectionReason(reason)) => str += &format!("{reason:?}"),
            Some(TestInstr::ExpectNodeAlive) => {}
            Some(TestInstr::RecvTx { wire, peer }) => {
                str += &format!("{} bytes from peer {peer}", wire.len())
            }
            Some(TestInstr::SubmitTx(tx))
            | Some(TestInstr::ExpectMempoolContains(tx))
            | Some(TestInstr::ExpectMempoolAbsent(tx))
            | Some(TestInstr::ExpectMempoolRejected(tx)) => str += &tx.hash().to_string(),
            Some(TestInstr::ExpectBond(key, status, amount)) => {
                str += &format!("{} status {status} amount {amount}", hex32(&key))
            }
            Some(TestInstr::ExpectFinalizerBank(pub_key, amount)) => {
                str += &format!("{} => {amount}", PubKeyID(pub_key))
            }
            Some(TestInstr::ExpectPoolTotals(pools)) => str += &format!("{pools:?}"),
            Some(TestInstr::RecvPoW { block, peer }) => {
                str += &match Block::zcash_deserialize(&block[..]) {
                    Ok(parsed) => format!("{} from peer {peer}", parsed.hash()),
                    Err(_) => format!("{} unparseable bytes from peer {peer}", block.len()),
                }
            }
            Some(TestInstr::MineFromTemplate) => {}
            Some(TestInstr::RecvStpPacket { packet, peer }) => {
                str += &format!("type {:?}, {} bytes from peer {peer}", packet.first(), packet.len())
            }
            None => {}
        }

        str += ")";

        if instr.flags != 0 {
            str += " [";
            if (instr.flags & SHOULD_FAIL) != 0 {
                str += " SHOULD_FAIL";
            }
            str += " ]";
        }

        str
    }

    pub fn data_slice<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        self.data.as_byte_slice_in(bytes)
    }
}

// Flags
pub const SHOULD_FAIL: u32 = 1 << 0;

pub struct TF {
    pub instrs: Vec<TFInstr>,
    pub data: Vec<u8>,
}

pub const TF_NOT_YET_FINALIZED: u64 = 0;
pub const TF_FINALIZED: u64 = 1;
pub const TF_CANT_BE_FINALIZED: u64 = 2;

pub fn finality_from_val(val: &[u64; 2]) -> Option<TFLBlockFinality> {
    if (val[0] == 0) {
        None
    } else {
        match (val[1]) {
            test_format::TF_NOT_YET_FINALIZED => Some(TFLBlockFinality::NotYetFinalized),
            test_format::TF_FINALIZED => Some(TFLBlockFinality::Finalized),
            test_format::TF_CANT_BE_FINALIZED => Some(TFLBlockFinality::CantBeFinalized),
            _ => panic!("unexpected finality value"),
        }
    }
}

pub fn val_from_finality(val: Option<TFLBlockFinality>) -> [u64; 2] {
    match (val) {
        Some(TFLBlockFinality::NotYetFinalized) => [1u64, test_format::TF_NOT_YET_FINALIZED],
        Some(TFLBlockFinality::Finalized) => [1u64, test_format::TF_FINALIZED],
        Some(TFLBlockFinality::CantBeFinalized) => [1u64, test_format::TF_CANT_BE_FINALIZED],
        None => [0u64; 2],
    }
}

impl TF {
    pub fn new(params: &ZcashCrosslinkParameters) -> TF {
        let mut tf = TF {
            instrs: Vec::new(),
            data: Vec::new(),
        };

        // Enforce that every parameter is written: adding a member fails to compile here.
        let ZcashCrosslinkParameters {
            bc_confirmation_depth_sigma,
            bootstrap,
            staking,
        } = *params;
        tf.push_instr_ex(
            TFInstr::SET_PARAMS,
            0,
            &params_to_bytes(bootstrap, staking),
            [bc_confirmation_depth_sigma, 0],
        );

        tf
    }

    pub fn push_serialize<Z: ZcashSerialize>(&mut self, z: &Z) -> TFSlice {
        let bgn = (size_of::<TFHdr>() + self.data.len()) as u64;
        z.zcash_serialize(&mut self.data);
        let end = (size_of::<TFHdr>() + self.data.len()) as u64;

        TFSlice {
            o: bgn,
            size: end - bgn,
        }
    }

    pub fn push_data(&mut self, bytes: &[u8]) -> TFSlice {
        let result = TFSlice {
            o: (size_of::<TFHdr>() + self.data.len()) as u64,
            size: bytes.len() as u64,
        };
        self.data.write_all(bytes);
        result
    }

    pub fn push_instr_ex(&mut self, kind: TFInstrKind, flags: u32, data: &[u8], val: [u64; 2]) {
        let data = self.push_data(data);
        self.instrs.push(TFInstr {
            kind,
            flags,
            data,
            val,
        });
    }

    pub fn push_instr(&mut self, kind: TFInstrKind, data: &[u8]) {
        self.push_instr_ex(kind, 0, data, [0; 2])
    }

    pub fn push_instr_val(&mut self, kind: TFInstrKind, val: [u64; 2]) {
        self.push_instr_ex(kind, 0, &[0; 0], val)
    }

    pub fn push_instr_serialize_ex<Z: ZcashSerialize>(
        &mut self,
        kind: TFInstrKind,
        flags: u32,
        data: &Z,
        val: [u64; 2],
    ) {
        let data = self.push_serialize(data);
        self.instrs.push(TFInstr {
            kind,
            flags,
            data,
            val,
        });
    }

    pub fn push_instr_serialize<Z: ZcashSerialize>(&mut self, kind: TFInstrKind, data: &Z) {
        self.push_instr_serialize_ex(kind, 0, data, [0; 2])
    }

    pub fn push_instr_load_pow(&mut self, data: &Block, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::LOAD_POW, flags, data, [0; 2])
    }
    pub fn push_instr_load_pow_bytes(&mut self, data: &[u8], flags: u32) {
        self.push_instr_ex(TFInstr::LOAD_POW, flags, data, [0; 2])
    }

    pub fn push_instr_load_pos(&mut self, data: &BftBlockAndFatPointerToItWrap, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::LOAD_POS, flags, data, [0; 2])
    }
    pub fn push_instr_load_pos_bytes(&mut self, data: &[u8], flags: u32) {
        self.push_instr_ex(TFInstr::LOAD_POS, flags, data, [0; 2])
    }

    pub fn push_instr_expect_pow_chain_length(&mut self, length: usize, flags: u32) {
        self.push_instr_ex(
            TFInstr::EXPECT_POW_CHAIN_LENGTH,
            flags,
            &[0; 0],
            [length as u64, 0],
        )
    }

    pub fn push_instr_expect_pos_chain_length(&mut self, length: usize, flags: u32) {
        self.push_instr_ex(
            TFInstr::EXPECT_POS_CHAIN_LENGTH,
            flags,
            &[0; 0],
            [length as u64, 0],
        )
    }

    pub fn push_instr_expect_pow_block_finality(
        &mut self,
        pow_hash: &ZebBlockHash,
        finality: Option<TFLBlockFinality>,
        flags: u32,
    ) {
        self.push_instr_ex(
            TFInstr::EXPECT_POW_BLOCK_FINALITY,
            flags,
            &pow_hash.0,
            test_format::val_from_finality(finality),
        )
    }

    pub fn push_instr_roster_force_include(&mut self, pub_key: [u8; 32], stake: u64, flags: u32) {
        self.push_instr_ex(TFInstr::ROSTER_FORCE_INCLUDE, flags, &pub_key, [stake, 0])
    }

    pub fn push_instr_expect_roster_includes(&mut self, pub_key: [u8; 32], stake: u64, flags: u32) {
        self.push_instr_ex(TFInstr::EXPECT_ROSTER_INCLUDES, flags, &pub_key, [stake, 0])
    }

    pub fn push_instr_expect_rejection_reason(&mut self, reason: &str, flags: u32) {
        self.push_instr_ex(TFInstr::EXPECT_REJECTION_REASON, flags, reason.as_bytes(), [0; 2])
    }

    pub fn push_instr_expect_node_alive(&mut self, flags: u32) {
        self.push_instr_ex(TFInstr::EXPECT_NODE_ALIVE, flags, &[0; 0], [0; 2])
    }

    /// A wire message delivered as if synthetic peer `peer` sent it. For a transaction, frame it
    /// with `zebra_network::wire::tx_message_bytes`.
    pub fn push_instr_recv_tx_wire(&mut self, wire: &[u8], peer: u64, flags: u32) {
        self.push_instr_ex(TFInstr::RECV_TX, flags, wire, [peer, 0])
    }

    pub fn push_instr_submit_tx(&mut self, tx: &Transaction, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::SUBMIT_TX, flags, tx, [0; 2])
    }

    pub fn push_instr_expect_mempool_contains(&mut self, tx: &Transaction, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::EXPECT_MEMPOOL_CONTAINS, flags, tx, [0; 2])
    }

    pub fn push_instr_expect_mempool_absent(&mut self, tx: &Transaction, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::EXPECT_MEMPOOL_ABSENT, flags, tx, [0; 2])
    }

    pub fn push_instr_expect_mempool_rejected(&mut self, tx: &Transaction, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::EXPECT_MEMPOOL_REJECTED, flags, tx, [0; 2])
    }

    /// `status` is a `TF_BOND_*` code; either argument may be `TEST_STAKE_IGNORED`.
    pub fn push_instr_expect_bond(&mut self, bond_key: [u8; 32], status: u64, amount: u64, flags: u32) {
        self.push_instr_ex(TFInstr::EXPECT_BOND, flags, &bond_key, [amount, status])
    }

    pub fn push_instr_expect_finalizer_bank(&mut self, pub_key: [u8; 32], amount: u64, flags: u32) {
        self.push_instr_ex(TFInstr::EXPECT_FINALIZER_BANK, flags, &pub_key, [amount, 0])
    }

    /// Bonded, unbonded and finalizer-reward pool totals; any may be `TEST_STAKE_IGNORED`.
    pub fn push_instr_expect_pool_totals(&mut self, bonded: u64, unbonded: u64, finalizer_rewards: u64, flags: u32) {
        let mut data = Vec::with_capacity(24);
        for amount in [bonded, unbonded, finalizer_rewards] {
            data.extend_from_slice(&amount.to_le_bytes());
        }
        self.push_instr_ex(TFInstr::EXPECT_POOL_TOTALS, flags, &data, [0; 2])
    }

    /// A block served over the STP block-sync exchange by synthetic peer `peer`.
    pub fn push_instr_recv_pow(&mut self, block: &Block, peer: u64, flags: u32) {
        self.push_instr_serialize_ex(TFInstr::RECV_POW, flags, block, [peer, 0])
    }

    /// Block bytes served as-is, for encodings `Block` would not produce.
    pub fn push_instr_recv_pow_bytes(&mut self, block: &[u8], peer: u64, flags: u32) {
        self.push_instr_ex(TFInstr::RECV_POW, flags, block, [peer, 0])
    }

    pub fn push_instr_recv_stp_packet(&mut self, packet: &[u8], peer: u64, flags: u32) {
        self.push_instr_ex(TFInstr::RECV_STP_PACKET, flags, packet, [peer, 0])
    }

    pub fn push_instr_mine_from_template(&mut self, flags: u32) {
        self.push_instr_ex(TFInstr::MINE_FROM_TEMPLATE, flags, &[0; 0], [0; 2])
    }

    /// A block the node must reject for `reason`, and must survive rejecting: the load, the
    /// reason check, and the liveness check, as three instructions.
    pub fn push_instr_load_pow_rejected(&mut self, block: &Block, reason: &str) {
        self.push_instr_load_pow(block, SHOULD_FAIL);
        self.push_instr_expect_rejection_reason(reason, 0);
        self.push_instr_expect_node_alive(0);
    }

    fn is_a_power_of_2(v: usize) -> bool {
        v != 0 && ((v & (v - 1)) == 0)
    }

    fn align_up(v: usize, mut align: usize) -> usize {
        assert!(Self::is_a_power_of_2(align));
        align -= 1;
        (v + align) & !align
    }

    pub fn write<W: std::io::Write>(&self, writer: &mut W) -> bool {
        let instrs_o_unaligned = size_of::<TFHdr>() + self.data.len();
        let instrs_o = Self::align_up(instrs_o_unaligned, align_of::<TFInstr>());
        let hdr = TFHdr {
            magic: "ZECCLTF0".as_bytes().try_into().unwrap(),
            instrs_o: instrs_o as u64,
            instrs_n: self.instrs.len() as u32,
            instr_size: size_of::<TFInstr>() as u32,
        };
        writer
            .write_all(hdr.as_bytes())
            .expect("writing shouldn't fail");
        writer
            .write_all(&self.data)
            .expect("writing shouldn't fail");

        if instrs_o > instrs_o_unaligned {
            const ALIGN_0S: [u8; align_of::<TFInstr>()] = [0u8; align_of::<TFInstr>()];
            let align_size = instrs_o - instrs_o_unaligned;
            let align_bytes = &ALIGN_0S[..align_size];
            writer.write_all(align_bytes);
        }
        writer
            .write_all(self.instrs.as_bytes())
            .expect("writing shouldn't fail");

        true
    }

    pub fn write_to_file(&self, path: &std::path::Path) -> bool {
        if let Ok(mut file) = std::fs::File::create(path) {
            self.write(&mut file)
        } else {
            false
        }
    }

    pub fn write_to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.write(&mut bytes);
        bytes
    }

    // Simple version, all in one go... for large files we'll want to break this up; get hdr &
    // get/stream instrs, then read data as needed
    pub fn read_from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let tf_hdr = match TFHdr::ref_from_prefix(&bytes[0..]) {
            Ok((hdr, _)) => hdr,
            Err(err) => return Err(err.to_string()),
        };

        let read_instrs = <[TFInstr]>::ref_from_prefix_with_elems(
            &bytes[tf_hdr.instrs_o as usize..],
            tf_hdr.instrs_n as usize,
        );

        let instrs = match read_instrs {
            Ok((instrs, _)) => instrs,
            Err(err) => return Err(err.to_string()),
        };

        let data = &bytes[size_of::<TFHdr>()..tf_hdr.instrs_o as usize];

        // TODO: just use slices, don't copy to vectors
        let tf = TF {
            instrs: instrs.to_vec(),
            data: data.to_vec(),
        };

        Ok(tf)
    }

    pub fn read_from_file(path: &std::path::Path) -> Result<(Vec<u8>, Self), String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => return Err(err.to_string()),
        };

        Self::read_from_bytes(&bytes).map(|tf| (bytes, tf))
    }
}

// TODO: macro for a stringified condition
fn test_check(flags: u32, condition: bool, message: &str) {
    *TEST_LAST_CHECK.lock().unwrap() = Some((condition, message.to_string()));
    let should_succeed = (flags & SHOULD_FAIL) == 0;
    const SUCCESS_STRS: [&str; 2] = ["fail", "succeed"];

    if condition != should_succeed {
        let test_instr_i = *TEST_INSTR_C.lock().unwrap();
        TEST_FAILED_INSTR_IDXS.lock().unwrap().push((test_instr_i, message.to_string()));

        match *TEST_CHECK_ASSERT.lock().unwrap() {
            0 => {},
            1 => error!(
                "test check should {} but actually {}ed, message:\n{}",
                SUCCESS_STRS[should_succeed as usize],
                SUCCESS_STRS[!should_succeed as usize],
                message
            ),
            _ => panic!(
                "test check should {} but actually {}ed (and TEST_CHECK_ASSERT enabled), message:\n{}",
                SUCCESS_STRS[should_succeed as usize],
                SUCCESS_STRS[!should_succeed as usize],
                message
            ),
        }
    }
}

use crate::*;

/// Parameters for scenarios that feed BFT blocks in directly, which is every scenario that is not
/// testing the bootstrap itself. A file with no `SET_PARAMS` is read as these: every scenario was
/// written that way before the bootstrap became a parameter.
pub const HARNESS_PARAMETERS: ZcashCrosslinkParameters = ZcashCrosslinkParameters {
    bootstrap: BftBootstrap::Supplied,
    // Sigma is pinned here rather than inherited from `PROTOTYPE_PARAMETERS`. The
    // scenes in the test suite are hand-built at specific heights: a BFT block carries exactly
    // sigma headers, and the PoW block that cites it has to sit at least sigma + 1 above the
    // block that certificate finalizes. Inheriting sigma would silently invalidate every one of
    // those scenes the moment the network parameter moved, which is not what changing a network
    // parameter should mean. The rules under test do not depend on sigma's value; the live
    // network's value is exercised on a testnet, not here.
    bc_confirmation_depth_sigma: 3,
    // Pinned for the same reason as sigma: the staking scenes are built at heights chosen for
    // this calendar. A scenario that wants a short one uses `short_staking`.
    staking: PROTOTYPE_STAKING,
};

/// The shortest staking calendar a scenario on `params` can use and still mean what it would on
/// a real network, for
/// `ZcashCrosslinkParameters { staking: short_staking(&params), ..params }`.
///
/// `StakingParameters::is_valid` is only what consensus needs to run. This adds what a scenario
/// needs to stay realistic:
/// - a day window of 3, so a test can act first inside, last inside and first outside it;
/// - an action delay one past the window, with room left in the period, so a withdrawal can
///   land early or late in a later window;
/// - a period at least `2 * (sigma + FINALITY_LIVENESS_ALLOWANCE + 1)`, so roster and stake
///   changes turn over slower than finality can reflect them, as they do in the prototype calendar.
///
/// A scenario that needs more, such as an edge further into the window, sets its own calendar.
///
/// With `CROSSLINK_TEST_MODE=conformance` this returns the prototype calendar instead, so every
/// scenario written against it also runs at real-network values. That run is the check on
/// anything the short calendar's margins miss, which is why scenarios derive their heights from
/// the calendar rather than hard-coding them.
pub fn short_staking(params: &ZcashCrosslinkParameters) -> StakingParameters {
    if conformance_mode() {
        return PROTOTYPE_STAKING;
    }
    short_calendar(params)
}

/// Whether this run is a conformance run (`CROSSLINK_TEST_MODE=conformance`): real-network
/// calendar values, generated fresh.
pub fn conformance_mode() -> bool {
    std::env::var("CROSSLINK_TEST_MODE").is_ok_and(|mode| mode == "conformance")
}

fn short_calendar(params: &ZcashCrosslinkParameters) -> StakingParameters {
    let day_window = 3;
    let action_delay = day_window + 1;
    let finality_gap = params.bc_confirmation_depth_sigma + FINALITY_LIVENESS_ALLOWANCE;
    let finality_floor = u32::try_from(2 * (finality_gap + 1)).expect("sigma is a small test value");
    let staking = StakingParameters {
        period: finality_floor.max(day_window + action_delay),
        day_window,
        action_delay,
    };
    assert!(staking.is_valid(), "short_staking built an invalid calendar: {staking:?}");
    staking
}

// `SET_PARAMS` carries sigma in `val[0]`, and in its data the bootstrap followed by the staking
// calendar (period, day window, action delay, each a little-endian u32). The calendar is written
// only when it isn't the prototype's, and data that ends after the bootstrap is read as the
// prototype calendar, so files from before the calendar was a parameter, and every file that
// keeps the prototype calendar, stay byte-identical.
// `val[1]` is written as zero and never read: it used to carry the Book's `L`, which Zebra
// Crosslink does not have.
const TF_BOOTSTRAP_SUPPLIED: u8 = 0;
const TF_BOOTSTRAP_FROM_CHAIN: u8 = 1;

fn params_to_bytes(bootstrap: BftBootstrap, staking: StakingParameters) -> Vec<u8> {
    let mut bytes = match bootstrap {
        BftBootstrap::Supplied => vec![TF_BOOTSTRAP_SUPPLIED],
        BftBootstrap::FromChain { staking_height, roster_height, activation_height } => {
            let mut bytes = vec![TF_BOOTSTRAP_FROM_CHAIN];
            bytes.extend_from_slice(&staking_height.to_le_bytes());
            bytes.extend_from_slice(&roster_height.to_le_bytes());
            bytes.extend_from_slice(&activation_height.to_le_bytes());
            bytes
        }
    };
    if staking != PROTOTYPE_STAKING {
        for value in [staking.period, staking.day_window, staking.action_delay] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn params_from_bytes(bytes: &[u8]) -> Option<(BftBootstrap, StakingParameters)> {
    let (bootstrap, rest) = match bytes {
        [TF_BOOTSTRAP_SUPPLIED, rest @ ..] => (BftBootstrap::Supplied, rest),
        [TF_BOOTSTRAP_FROM_CHAIN, s0, s1, s2, s3, r0, r1, r2, r3, a0, a1, a2, a3, rest @ ..] => (
            BftBootstrap::FromChain {
                staking_height: u32::from_le_bytes([*s0, *s1, *s2, *s3]),
                roster_height: u32::from_le_bytes([*r0, *r1, *r2, *r3]),
                activation_height: u32::from_le_bytes([*a0, *a1, *a2, *a3]),
            },
            rest,
        ),
        _ => return None,
    };
    let word = |i: usize| u32::from_le_bytes(rest[4 * i..4 * i + 4].try_into().expect("four bytes"));
    let staking = match rest.len() {
        0 => PROTOTYPE_STAKING,
        12 => StakingParameters { period: word(0), day_window: word(1), action_delay: word(2) },
        _ => return None,
    };
    Some((bootstrap, staking))
}

/// The Crosslink parameters a test file's node must run with: its leading `SET_PARAMS`, or
/// [`HARNESS_PARAMETERS`] if it has none. They are consensus parameters, so the harness builds the
/// network with them before the node boots instead of applying them when the instruction runs.
pub fn crosslink_parameters_for_test(bytes: &[u8]) -> ZcashCrosslinkParameters {
    let Ok(tf) = TF::read_from_bytes(bytes) else {
        return HARNESS_PARAMETERS;
    };
    let Some(first) = tf.instrs.first() else {
        return HARNESS_PARAMETERS;
    };
    if first.kind != TFInstr::SET_PARAMS {
        return HARNESS_PARAMETERS;
    }
    // A malformed SET_PARAMS falls back here, then fails loudly when the instruction is read.
    if let Some(TestInstr::SetParams(params)) = tf_read_instr(bytes, first) {
        params
    } else {
        HARNESS_PARAMETERS
    }
}

pub(crate) fn tf_read_instr(bytes: &[u8], instr: &TFInstr) -> Option<TestInstr> {
    const_assert!(TFInstr::COUNT == 21);
    match instr.kind {
        TFInstr::LOAD_POW => {
            let block = Block::zcash_deserialize(instr.data_slice(bytes)).ok()?;
            Some(TestInstr::LoadPoW(block))
        }

        TFInstr::LOAD_POS => {
            let block_and_fat_ptr =
                BftBlockAndFatPointerToItWrap::zcash_deserialize(instr.data_slice(bytes)).ok()?;
            Some(TestInstr::LoadPoS((
                block_and_fat_ptr.0.block,
                block_and_fat_ptr.0.fat_ptr,
            )))
        }

        TFInstr::SET_PARAMS => {
            let (bootstrap, staking) = params_from_bytes(instr.data_slice(bytes))?;
            Some(TestInstr::SetParams(ZcashCrosslinkParameters {
                bc_confirmation_depth_sigma: instr.val[0],
                bootstrap,
                staking,
            }))
        }

        TFInstr::EXPECT_POW_CHAIN_LENGTH => {
            Some(TestInstr::ExpectPoWChainLength(instr.val[0] as u32))
        }
        TFInstr::EXPECT_POS_CHAIN_LENGTH => Some(TestInstr::ExpectPoSChainLength(instr.val[0])),

        TFInstr::EXPECT_POW_BLOCK_FINALITY => Some(TestInstr::ExpectPoWBlockFinality(
            ZebBlockHash(
                instr
                    .data_slice(bytes)
                    .try_into()
                    .expect("should be 32 bytes for hash"),
            ),
            finality_from_val(&instr.val),
        )),

        TFInstr::ROSTER_FORCE_INCLUDE => Some(TestInstr::RosterForceInclude(
            instr.data_slice(bytes).try_into().expect("32-byte array"),
            instr.val[0],
        )),
        TFInstr::EXPECT_ROSTER_INCLUDES => Some(TestInstr::ExpectRosterIncludes(
            instr.data_slice(bytes).try_into().expect("32-byte array"),
            instr.val[0],
        )),

        TFInstr::EXPECT_REJECTION_REASON => Some(TestInstr::ExpectRejectionReason(
            String::from_utf8(instr.data_slice(bytes).to_vec()).ok()?,
        )),
        TFInstr::EXPECT_NODE_ALIVE => Some(TestInstr::ExpectNodeAlive),
        TFInstr::RECV_TX => Some(TestInstr::RecvTx {
            wire: instr.data_slice(bytes).to_vec(),
            peer: instr.val[0],
        }),
        TFInstr::SUBMIT_TX => Some(TestInstr::SubmitTx(
            Transaction::zcash_deserialize(instr.data_slice(bytes)).ok()?,
        )),
        TFInstr::EXPECT_MEMPOOL_CONTAINS => Some(TestInstr::ExpectMempoolContains(
            Transaction::zcash_deserialize(instr.data_slice(bytes)).ok()?,
        )),
        TFInstr::EXPECT_MEMPOOL_ABSENT => Some(TestInstr::ExpectMempoolAbsent(
            Transaction::zcash_deserialize(instr.data_slice(bytes)).ok()?,
        )),
        TFInstr::EXPECT_MEMPOOL_REJECTED => Some(TestInstr::ExpectMempoolRejected(
            Transaction::zcash_deserialize(instr.data_slice(bytes)).ok()?,
        )),
        TFInstr::EXPECT_BOND => Some(TestInstr::ExpectBond(
            instr.data_slice(bytes).try_into().ok()?,
            instr.val[1],
            instr.val[0],
        )),
        TFInstr::EXPECT_FINALIZER_BANK => Some(TestInstr::ExpectFinalizerBank(
            instr.data_slice(bytes).try_into().ok()?,
            instr.val[0],
        )),
        TFInstr::EXPECT_POOL_TOTALS => {
            let data: [u8; 24] = instr.data_slice(bytes).try_into().ok()?;
            let amount = |i: usize| u64::from_le_bytes(data[8 * i..8 * i + 8].try_into().expect("eight bytes"));
            Some(TestInstr::ExpectPoolTotals([amount(0), amount(1), amount(2)]))
        }
        TFInstr::RECV_POW => Some(TestInstr::RecvPoW {
            block: instr.data_slice(bytes).to_vec(),
            peer: instr.val[0],
        }),
        TFInstr::MINE_FROM_TEMPLATE => Some(TestInstr::MineFromTemplate),
        TFInstr::RECV_STP_PACKET => Some(TestInstr::RecvStpPacket {
            packet: instr.data_slice(bytes).to_vec(),
            peer: instr.val[0],
        }),

        _ => {
            panic!("Unrecognized instruction {}", instr.kind);
            None
        }
    }
}

#[derive(Clone)]
pub(crate) enum TestInstr {
    LoadPoW(Block),
    LoadPoS((BftBlock, FatPointerToBftBlock)),
    SetParams(ZcashCrosslinkParameters),
    ExpectPoWChainLength(u32),
    ExpectPoSChainLength(u64),
    ExpectPoWBlockFinality(ZebBlockHash, Option<TFLBlockFinality>),
    ExpectRejectionReason(String),
    ExpectNodeAlive,
    RecvTx { wire: Vec<u8>, peer: u64 },
    SubmitTx(Transaction),
    ExpectMempoolContains(Transaction),
    ExpectMempoolAbsent(Transaction),
    ExpectMempoolRejected(Transaction),
    /// (bond key, expected status, expected amount)
    ExpectBond([u8; 32], u64, u64),
    ExpectFinalizerBank([u8; 32], u64),
    /// [bonded, unbonded, finalizer rewards]
    ExpectPoolTotals([u64; 3]),
    /// Raw bytes: the peer serves exactly what the file holds, parseable or not.
    RecvPoW { block: Vec<u8>, peer: u64 },
    RecvStpPacket { packet: Vec<u8>, peer: u64 },
    MineFromTemplate,
    RosterForceInclude([u8; 32], u64),   // public address
    ExpectRosterIncludes([u8; 32], u64), // public address
}

pub(crate) async fn handle_instr(
    internal_handle: &TFLServiceHandle,
    bytes: &[u8],
    instr: TestInstr,
    flags: u32,
    instr_i: usize,
) {
    match instr {
        TestInstr::LoadPoW(block) => {
            // let path = format!("../crosslink-test-data/test_pow_block_{}.bin", instr_i);
            // info!("writing binary at {}", path);
            // let mut file = std::fs::File::create(&path).expect("valid file");
            // file.write_all(instr.data_slice(bytes));

            let (force_feed_ok, msg) = ingest_pow(Arc::new(block)).await;
            test_check(flags, force_feed_ok, &msg);
        }

        TestInstr::MineFromTemplate => {
            let (accepted, message) = match internal_handle.call.block_from_template.get() {
                None => (false, "MINE_FROM_TEMPLATE: the node has no RPC implementation to take a template from".to_string()),
                Some(block_from_template) => match block_from_template().await {
                    Err(err) => (false, format!("MINE_FROM_TEMPLATE: {err}")),
                    Ok(block) => {
                        let label = format!(
                            "MINE_FROM_TEMPLATE {} @ {:?}, {} transaction(s)",
                            block.hash(),
                            block.coinbase_height().map(|height| height.0),
                            block.transactions.len()
                        );
                        let (accepted, verdict) = ingest_pow(Arc::new(block)).await;
                        (accepted, format!("{label}: {verdict}"))
                    }
                },
            };
            test_check(flags, accepted, &message);
        }

        TestInstr::LoadPoS((block, fat_ptr)) => {
            // let path = format!("../crosslink-test-data/test_pos_block_{}.bin", instr_i);
            // info!("writing binary at {}", path);
            // let mut file = std::fs::File::create(&path).expect("valid file");
            // file.write_all(instr.data_slice(bytes)).expect("write success");

            let (force_feed_ok, msg) = match zebra_state::new_network::bft::force_feed_bft_block(Arc::new(block), fat_ptr).await {
                Ok(()) => (true, "PoS force feed ok".to_string()),
                Err(msg) => (false, msg),
            };
            test_check(flags, force_feed_ok, &msg);
        }

        TestInstr::SetParams(params) => {
            debug_assert!(instr_i == 0, "should only be set at the beginning");
            // Consensus parameters are fixed when the network is built, before the node boots (see
            // `crosslink_parameters_for_test`), so all that remains is confirming the node agrees.
            test_check(
                flags,
                params == internal_handle.params,
                &format!("SET_PARAMS: file declares {:?}, node runs {:?}", params, internal_handle.params),
            );
        }

        TestInstr::ExpectPoWChainLength(h) => {
            // Bounded, and every answer is recorded: an unbounded wait hangs the test if the state
            // service stops answering, and skipping the check on a missing tip passed silently.
            let (holds, message) = match tokio::time::timeout(
                NODE_ANSWER_WAIT,
                (internal_handle.call.state)(StateRequest::Tip),
            )
            .await
            {
                Ok(Ok(StateResponse::Tip(Some((height, _))))) => {
                    let actual = height.0 + 1;
                    (h == actual, format!("PoW chain length: expected {h}, actually {actual}"))
                }
                Ok(Ok(StateResponse::Tip(None))) => {
                    (false, format!("PoW chain length: expected {h}, but the node has no tip"))
                }
                Ok(Ok(other)) => (false, format!("PoW chain length: the tip request answered {other:?}")),
                Ok(Err(err)) => (false, format!("PoW chain length: the tip request failed: {err}")),
                Err(_) => (false, format!("PoW chain length: no answer to the tip request within {NODE_ANSWER_WAIT:?}")),
            };
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectRejectionReason(reason) => {
            let previous = TEST_PREV_OUTCOME.lock().unwrap().clone();
            let (holds, message) = match previous {
                Some((false, rejection)) => (
                    rejection.contains(&reason),
                    format!("rejection reason: expected it to contain {reason:?}, actually {rejection:?}"),
                ),
                Some((true, accepted)) => (
                    false,
                    format!("rejection reason: expected a rejection containing {reason:?}, but the previous instruction was accepted: {accepted:?}"),
                ),
                None => (
                    false,
                    format!("rejection reason: expected a rejection containing {reason:?}, but the previous instruction made no check"),
                ),
            };
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectNodeAlive => {
            let (alive, message) = node_alive(internal_handle).await;
            test_check(flags, alive, &message);
        }

        TestInstr::RecvTx { wire, peer } => {
            let sender = synthetic_peer(peer);
            let delivery = tokio::time::timeout(NODE_ANSWER_WAIT, (internal_handle.call.inbound_wire)(sender, wire)).await;
            let (delivered, message) = match delivery {
                Ok(Ok(())) => (true, format!("RECV_TX: delivered from peer {sender}")),
                Ok(Err(err)) => (false, format!("RECV_TX: the message from peer {sender} was refused: {err}")),
                Err(_) => (false, format!("RECV_TX: no answer delivering from peer {sender} within {NODE_ANSWER_WAIT:?}")),
            };
            test_check(flags, delivered, &message);
        }

        TestInstr::SubmitTx(transaction) => {
            let (accepted, message) = submit_tx(internal_handle, transaction).await;
            test_check(flags, accepted, &message);
        }

        TestInstr::RecvPoW { block, peer } => {
            let mut stp = take_stp_peer(peer);
            let (accepted, message) = serve_block(internal_handle, &mut stp, &block).await;
            put_stp_peer(stp);
            test_check(flags, accepted, &message);
        }

        TestInstr::RecvStpPacket { packet, peer } => {
            let mut stp = take_stp_peer(peer);
            let (survived, message) = deliver_stp_packet(&mut stp, packet).await;
            put_stp_peer(stp);
            test_check(flags, survived, &message);
        }

        TestInstr::ExpectMempoolContains(transaction) => {
            let (holds, message) = await_mempool(internal_handle, &transaction, MempoolExpect::Resident).await;
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectMempoolAbsent(transaction) => {
            let (holds, message) = await_mempool(internal_handle, &transaction, MempoolExpect::Absent).await;
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectMempoolRejected(transaction) => {
            let (holds, message) = await_mempool(internal_handle, &transaction, MempoolExpect::Rejected).await;
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectBond(key, status, amount) => {
            let answer = tokio::time::timeout(
                NODE_ANSWER_WAIT,
                (internal_handle.call.state)(StateRequest::BondInfo(key)),
            )
            .await;
            let (holds, message) = match answer {
                Ok(Ok(StateResponse::BondInfo(info))) => {
                    let actual_status = info.as_ref().map_or(TF_BOND_ABSENT, |info| u64::from(info.status));
                    let actual_amount = info.as_ref().map(|info| u64::from(info.amount));
                    let status_holds = status == TEST_STAKE_IGNORED || status == actual_status;
                    let amount_holds = amount == TEST_STAKE_IGNORED || actual_amount == Some(amount);
                    let actual = match &info {
                        Some(info) => format!(
                            "status {} amount {} (last action at {}, target {})",
                            info.status,
                            u64::from(info.amount),
                            info.last_action_height,
                            PubKeyID(info.target_finalizer),
                        ),
                        None => "no such bond".to_string(),
                    };
                    (
                        status_holds && amount_holds,
                        format!("bond {}: expected status {} amount {}, actually {actual}", hex32(&key), stake_expectation(status), stake_expectation(amount)),
                    )
                }
                Ok(Ok(other)) => (false, format!("bond {}: the bond request answered {other:?}", hex32(&key))),
                Ok(Err(err)) => (false, format!("bond {}: the bond request failed: {err}", hex32(&key))),
                Err(_) => (false, format!("bond {}: no answer within {NODE_ANSWER_WAIT:?}", hex32(&key))),
            };
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectFinalizerBank(pub_key, amount) => {
            let answer = tokio::time::timeout(
                NODE_ANSWER_WAIT,
                (internal_handle.call.state)(StateRequest::FinalizerRewardBalance(pub_key)),
            )
            .await;
            let finalizer = PubKeyID(pub_key);
            let (holds, message) = match answer {
                Ok(Ok(StateResponse::FinalizerRewardBalance(bank))) => {
                    (bank == amount, format!("finalizer {finalizer} bank: expected {amount}, actually {bank}"))
                }
                Ok(Ok(other)) => (false, format!("finalizer {finalizer} bank: the request answered {other:?}")),
                Ok(Err(err)) => (false, format!("finalizer {finalizer} bank: the request failed: {err}")),
                Err(_) => (false, format!("finalizer {finalizer} bank: no answer within {NODE_ANSWER_WAIT:?}")),
            };
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectPoolTotals(expected) => {
            let answer = tokio::time::timeout(
                NODE_ANSWER_WAIT,
                (internal_handle.call.read_state)(zebra_state::ReadRequest::TipPoolValues),
            )
            .await;
            let (holds, message) = match answer {
                Ok(Ok(zebra_state::ReadResponse::TipPoolValues { tip_height, value_balance, .. })) => {
                    let actual = [
                        u64::from(value_balance.staking_bonded_amount()),
                        u64::from(value_balance.staking_unbonded_amount()),
                        u64::from(value_balance.finalizer_rewards_amount()),
                    ];
                    let holds = expected.iter().zip(actual).all(|(&e, a)| e == TEST_STAKE_IGNORED || e == a);
                    (
                        holds,
                        format!(
                            "pool totals at height {} [bonded, unbonded, finalizer rewards]: expected [{}], actually {actual:?}",
                            tip_height.0,
                            expected.map(stake_expectation).join(", ")
                        ),
                    )
                }
                Ok(Ok(other)) => (false, format!("pool totals: the request answered {other:?}")),
                Ok(Err(err)) => (false, format!("pool totals: the request failed: {err}")),
                Err(_) => (false, format!("pool totals: no answer within {NODE_ANSWER_WAIT:?}")),
            };
            test_check(flags, holds, &message);
        }

        TestInstr::ExpectPoSChainLength(h) => {
            let expect = h as usize;
            let actual = zebra_state::new_network::bft::bft_chain().read().unwrap().blocks.len();
            test_check(
                flags,
                expect == actual,
                &format!("PoS chain length: expected {}, actually {}", expect, actual),
            ); // TODO: maybe assert in test but recoverable error in-GUI
        }

        TestInstr::ExpectPoWBlockFinality(hash, f) => {
            let expect = f;
            let height = block_height_from_hash(&internal_handle.call.clone(), hash).await;
            // The state service answers `fin` and the best chain in one read (FINALITY.md
            // §7.2), so the harness asks it exactly as the RPC does.
            let actual = match (internal_handle.call.read_state)(
                zebra_state::ReadRequest::CrosslinkBlockFinality(hash),
            )
            .await
            {
                Ok(zebra_state::ReadResponse::CrosslinkBlockFinality(finality)) => Some(finality),
                _ => None,
            };
            test_check(
                flags,
                expect == actual,
                &format!(
                    "PoW block finality at hash={}, height={:?}: expected {:?}, actually {:?}",
                    hash, height, expect, actual
                ),
            ); // TODO: maybe assert in test but recoverable error in-GUI
        }

        TestInstr::ExpectRosterIncludes(pub_key, stake) => {
            let key = PubKeyID(pub_key);
            let finalizer = zebra_state::new_network::bft::bft_chain()
                .read()
                .unwrap()
                .roster
                .iter()
                .find(|x| PubKeyID(x.pub_key) == key)
                .cloned();

            if let Some(finalizer) = finalizer {
                test_check(
                    flags,
                    stake == TEST_STAKE_IGNORED || stake == finalizer.voting_power,
                    &format!(
                        "Finalizer stake: expected {}, actually {}",
                        stake, finalizer.voting_power
                    ),
                );
            } else {
                test_check(
                    flags,
                    false,
                    &format!("Finalizer found: {:?}", key),
                );
            }
        }

        TestInstr::RosterForceInclude(pub_key, stake) => {
            zebra_state::new_network::bft::bft_chain()
                .write()
                .unwrap()
                .roster
                .push(RosterMember { pub_key, voting_power: stake, txids: Vec::new(), finalizer_address: None });
        }
    }
}

/// How long an instruction waits for the node to answer before recording that it didn't.
const NODE_ANSWER_WAIT: Duration = Duration::from_secs(30);

/// Whether the node still answers: its state service returns the tip within a bound, and
/// re-submitting the tip block through the ingest path -- the doorway every LOAD_POW uses --
/// answers `Known`, which only a live sync loop can do.
async fn node_alive(internal_handle: &TFLServiceHandle) -> (bool, String) {
    let tip = tokio::time::timeout(NODE_ANSWER_WAIT, (internal_handle.call.state)(StateRequest::Tip)).await;
    let (height, hash) = match tip {
        Ok(Ok(StateResponse::Tip(Some(tip)))) => tip,
        Ok(Ok(other)) => return (false, format!("node alive: the tip request answered {other:?}")),
        Ok(Err(err)) => return (false, format!("node alive: the tip request failed: {err}")),
        Err(_) => return (false, format!("node alive: no answer to the tip request within {NODE_ANSWER_WAIT:?}")),
    };
    let block = tokio::time::timeout(
        NODE_ANSWER_WAIT,
        (internal_handle.call.state)(StateRequest::Block(hash.into())),
    )
    .await;
    let block = match block {
        Ok(Ok(StateResponse::Block(Some(block)))) => block,
        other => return (false, format!("node alive: could not read the tip block {hash} at height {}: {other:?}", height.0)),
    };
    match zebra_state::new_network::submit_block_to_new_network(block, NODE_ANSWER_WAIT).await {
        Ok(zebra_state::new_network::IngestOutcome::Known { .. }) => {
            (true, format!("node alive: re-submitting tip {hash} at height {} answered Known", height.0))
        }
        Ok(other) => (false, format!("node alive: re-submitting the tip answered {other:?}, not Known")),
        Err(err) => (false, format!("node alive: the ingest loop did not answer re-submitting the tip: {err}")),
    }
}

fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A synthetic STP peer and what the harness knows about it. A killed peer stays killed for the
/// rest of the test, as a real one would.
struct HarnessStpPeer {
    index: u64,
    link: zebra_state::new_network::SyntheticPeer,
    killed: Option<String>,
}

/// Peers wait here between instructions; an instruction takes its peer out, so no lock is held
/// across an await.
static TEST_STP_PEERS: Mutex<Vec<HarnessStpPeer>> = Mutex::new(Vec::new());

/// How long a packet gets to provoke a kill. The sync loop ticks every 100 ms, and nothing
/// acknowledges a packet, so this is a bound rather than a handshake.
const STP_PACKET_SETTLE: Duration = Duration::from_secs(1);

/// How long to watch a peer advertise a block the node already has. The node never requests it,
/// so there is no verdict to wait for, only a kill to rule out.
const KNOWN_BLOCK_SETTLE: Duration = Duration::from_secs(2);

/// Through new_network's ingest queue, the doorway submitblock uses, so blocks take the
/// production admission path rather than a parallel one.
async fn ingest_pow(block: Arc<Block>) -> (bool, String) {
    use zebra_state::new_network::IngestOutcome;
    match zebra_state::new_network::submit_block_to_new_network(block, NODE_ANSWER_WAIT).await {
        Ok(IngestOutcome::Committed(_)) => (true, "PoW ingest ok".to_string()),
        Ok(IngestOutcome::Known { .. }) => (true, "PoW already known".to_string()),
        Ok(IngestOutcome::Failed { reason, .. }) => (false, reason),
        Err(msg) => (false, msg),
    }
}

fn take_stp_peer(index: u64) -> HarnessStpPeer {
    let mut peers = TEST_STP_PEERS.lock().unwrap();
    if let Some(i) = peers.iter().position(|p| p.index == index) {
        return peers.swap_remove(i);
    }
    let link_index = u16::try_from(index).expect("synthetic peer indices fit in 16 bits");
    HarnessStpPeer { index, link: zebra_state::new_network::attach_synthetic_peer(link_index), killed: None }
}

fn put_stp_peer(peer: HarnessStpPeer) {
    TEST_STP_PEERS.lock().unwrap().push(peer);
}

/// Forgets whatever the peer received since its last instruction, keeping only a kill.
fn drain_stp_peer(stp: &mut HarnessStpPeer) {
    while stp.link.outbound.try_recv().is_ok() {}
    while let Ok(event) = stp.link.events.try_recv() {
        if let zebra_state::new_network::SyntheticPeerEvent::Killed(reason) = event {
            stp.killed = Some(reason);
        }
    }
}

async fn deliver_stp_packet(stp: &mut HarnessStpPeer, packet: Vec<u8>) -> (bool, String) {
    use zebra_state::new_network::SyntheticPeerEvent;
    let index = stp.index;
    drain_stp_peer(stp);
    if let Some(reason) = &stp.killed {
        return (false, format!("RECV_STP_PACKET: peer {index} was already killed: {reason}"));
    }
    let _ = stp.link.inbound.send(packet);

    let deadline = tokio::time::Instant::now() + STP_PACKET_SETTLE;
    loop {
        tokio::select! {
            event = stp.link.events.recv() => match event {
                Some(SyntheticPeerEvent::Killed(reason)) => {
                    stp.killed = Some(reason.clone());
                    return (false, format!("RECV_STP_PACKET: the node killed peer {index}: {reason}"));
                }
                Some(_) => {}
                None => return (false, "RECV_STP_PACKET: the sync loop is gone".to_string()),
            },
            _ = tokio::time::sleep_until(deadline) => {
                return (true, format!("RECV_STP_PACKET: peer {index} still connected after {STP_PACKET_SETTLE:?}"));
            }
        }
    }
}

/// Plays a peer that has `wire_block`: advertises it, then answers the node's requests for it.
async fn serve_block(internal_handle: &TFLServiceHandle, stp: &mut HarnessStpPeer, wire_block: &[u8]) -> (bool, String) {
    use zebra_state::new_network::{
        block_chunk_packets, parse_block_request, status_packet, IngestOutcome, NearTipChains, ShadowBlock,
        SyntheticPeerEvent,
    };
    let index = stp.index;
    let Ok(block) = Block::zcash_deserialize(wire_block) else {
        return (false, "RECV_POW: the block does not parse, so no STATUS can advertise it; send its packets with RECV_STP_PACKET".to_string());
    };
    let hash = block.hash();
    let Some(height) = block.coinbase_height().map(|height| height.0) else {
        return (false, format!("RECV_POW: block {hash} has no coinbase height, so no STATUS can advertise it"));
    };

    drain_stp_peer(stp);
    if let Some(reason) = &stp.killed {
        return (false, format!("RECV_POW: peer {index} was already killed: {reason}"));
    }

    let already_known = matches!(
        tokio::time::timeout(NODE_ANSWER_WAIT, (internal_handle.call.state)(StateRequest::KnownBlock(hash))).await,
        Ok(Ok(StateResponse::KnownBlock(Some(_))))
    );

    let mut chains = NearTipChains::default();
    chains.push_chain_unchecked(vec![ShadowBlock { this_hash: hash, parent_hash: block.header.previous_block_hash, this_height: height }]);
    let _ = stp.link.inbound.send(status_packet(&[], &chains, None));

    let deadline = tokio::time::Instant::now() + if already_known { KNOWN_BLOCK_SETTLE } else { NODE_ANSWER_WAIT };
    let (mut requests, mut chunks) = (0usize, 0usize);
    loop {
        tokio::select! {
            packet = stp.link.outbound.recv() => {
                let Some(packet) = packet else {
                    return (false, "RECV_POW: the sync loop is gone".to_string());
                };
                let Some((req_height, req_hash, offset)) = parse_block_request(&packet) else {
                    continue; // STATUS, address gossip, hole punching: a real peer would handle these, but they don't bear on this block
                };
                let asks_for_this = req_hash == hash || (req_hash == ZebBlockHash([0; 32]) && req_height == height);
                if asks_for_this && (offset as usize) < wire_block.len() {
                    requests += 1;
                    for chunk in block_chunk_packets(wire_block, height, hash, offset as usize) {
                        chunks += 1;
                        let _ = stp.link.inbound.send(chunk);
                    }
                }
            }
            event = stp.link.events.recv() => match event {
                Some(SyntheticPeerEvent::Killed(reason)) => {
                    stp.killed = Some(reason.clone());
                    return (false, format!("RECV_POW: the node killed peer {index}: {reason}"));
                }
                // As LOAD_POW reports it: a deferral is a failed ingest whose block stays queued.
                Some(SyntheticPeerEvent::Deferred { hash: deferred, reason }) if deferred == hash => return (false, reason),
                Some(SyntheticPeerEvent::Outcome { hash: decided, outcome }) if decided == hash => {
                    return match outcome {
                        IngestOutcome::Committed(_) => (true, "PoW ingest ok".to_string()),
                        IngestOutcome::Known { .. } => (true, "PoW already known".to_string()),
                        IngestOutcome::Failed { reason, .. } => (false, reason),
                    };
                }
                Some(_) => {}
                None => return (false, "RECV_POW: the sync loop is gone".to_string()),
            },
            _ = tokio::time::sleep_until(deadline) => {
                if already_known && requests == 0 {
                    return (true, "PoW already known".to_string());
                }
                return (
                    false,
                    format!("RECV_POW: no verdict on {hash} @ {height} within {NODE_ANSWER_WAIT:?}; the node sent {requests} request(s) for it, answered with {chunks} chunk(s)"),
                );
            }
        }
    }
}

fn stake_expectation(value: u64) -> String {
    if value == TEST_STAKE_IGNORED { "*".to_string() } else { value.to_string() }
}

/// The address synthetic peer `index` sends from. Distinct peers get distinct addresses, so each
/// meets the mempool's per-peer limits separately.
fn synthetic_peer(index: u64) -> std::net::SocketAddr {
    std::net::SocketAddr::from(([127, 1, (index >> 8) as u8, index as u8], 18233))
}

/// Submits `transaction` locally, as the wallet and RPC do, and returns the mempool's verdict:
/// accepted, or rejected with its reason.
async fn submit_tx(internal_handle: &TFLServiceHandle, transaction: Transaction) -> (bool, String) {
    use zebra_node_services::mempool::{Gossip, Request, Response};

    let txid = transaction.hash();
    let queued = tokio::time::timeout(
        NODE_ANSWER_WAIT,
        (internal_handle.call.mempool)(Request::Queue(vec![Gossip::Tx(transaction.into())])),
    )
    .await;
    let verdict = match queued {
        Ok(Ok(Response::Queued(mut results))) if results.len() == 1 => match results.remove(0) {
            Ok(verdict) => verdict,
            Err(err) => return (false, format!("SUBMIT_TX {txid}: the mempool refused to queue it: {err}")),
        },
        Ok(Ok(other)) => return (false, format!("SUBMIT_TX {txid}: the mempool answered {other:?}")),
        Ok(Err(err)) => return (false, format!("SUBMIT_TX {txid}: the queue request failed: {err}")),
        Err(_) => return (false, format!("SUBMIT_TX {txid}: no answer from the mempool within {NODE_ANSWER_WAIT:?}")),
    };
    match tokio::time::timeout(NODE_ANSWER_WAIT, verdict).await {
        Ok(Ok(Ok(()))) => (true, format!("SUBMIT_TX {txid}: accepted into the mempool")),
        Ok(Ok(Err(err))) => (false, format!("SUBMIT_TX {txid}: rejected: {err}")),
        Ok(Err(_)) => (false, format!("SUBMIT_TX {txid}: the mempool dropped the verdict")),
        Err(_) => (false, format!("SUBMIT_TX {txid}: no verdict within {NODE_ANSWER_WAIT:?}")),
    }
}

#[derive(Clone, Copy, Debug)]
enum MempoolExpect {
    Resident,
    Absent,
    Rejected,
}

/// Waits, within a bound, for `transaction` to reach the state `expect` names in the mempool.
/// Peer-sent transactions are downloaded and verified asynchronously, so a single read would race
/// the verification.
async fn await_mempool(
    internal_handle: &TFLServiceHandle,
    transaction: &Transaction,
    expect: MempoolExpect,
) -> (bool, String) {
    use zebra_node_services::mempool::{Request, Response};

    let id = UnminedTxId::from(transaction);
    let txid = transaction.hash();
    let ask = |request: Request| {
        tokio::time::timeout(NODE_ANSWER_WAIT, (internal_handle.call.mempool)(request))
    };
    let deadline = tokio::time::Instant::now() + NODE_ANSWER_WAIT;
    loop {
        let resident = match ask(Request::TransactionIds).await {
            Ok(Ok(Response::TransactionIds(ids))) => ids.contains(&id),
            other => return (false, format!("mempool {txid}: the transaction ids request answered {other:?}")),
        };
        let rejected = match ask(Request::RejectedTransactionIds([id].into_iter().collect())).await {
            Ok(Ok(Response::RejectedTransactionIds(ids))) => !ids.is_empty(),
            other => return (false, format!("mempool {txid}: the rejected ids request answered {other:?}")),
        };
        let state = format!("resident {resident}, rejected {rejected}");
        let reached = match expect {
            MempoolExpect::Resident => resident,
            MempoolExpect::Absent => !resident,
            MempoolExpect::Rejected => rejected,
        };
        if reached {
            return (true, format!("mempool {txid}: {expect:?} ({state})"));
        }
        // A rejected transaction won't become resident; say so now rather than at the deadline.
        if matches!(expect, MempoolExpect::Resident) && rejected {
            return (false, format!("mempool {txid}: expected {expect:?}, but it was rejected"));
        }
        if tokio::time::Instant::now() >= deadline {
            return (
                false,
                format!(
                    "mempool {txid}: expected {expect:?}, still {state} after {NODE_ANSWER_WAIT:?} \
                     (a disabled mempool drops peer transactions without a word)"
                ),
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn read_instrs(internal_handle: TFLServiceHandle, bytes: &[u8], instrs: &[TFInstr]) {
    // A failed deserialize is a hard error for a normal test but an expected input rejection
    // for the fuzzer; `uhh_option` decides which via TEST_ON_FAIL (PANIC vs recover).
    let on_fail = *TEST_ON_FAIL.lock().unwrap();
    for instr_i in 0..instrs.len() {
        // info!(
        //     "Loading instruction {}: {} ({})",
        //     instr_i,
        //     TFInstr::string_from_instr(bytes, &instrs[instr_i]),
        //     instrs[instr_i].kind
        // );

        if let Some(instr) = uhh_option(tf_read_instr(bytes, &instrs[instr_i]), on_fail) {
            let height = match &instr {
                TestInstr::LoadPoW(block) => block.coinbase_height().map(|height| height.0),
                TestInstr::RecvPoW { block, .. } => Block::zcash_deserialize(&block[..])
                    .ok()
                    .and_then(|block| block.coinbase_height())
                    .map(|height| height.0),
                _ => None,
            };
            let failed_before = TEST_FAILED_INSTR_IDXS.lock().unwrap().len();
            *TEST_LAST_CHECK.lock().unwrap() = None;
            let start = std::time::Instant::now();

            handle_instr(
                &internal_handle,
                bytes,
                instr,
                instrs[instr_i].flags,
                instr_i,
            )
            .await;

            let outcome = TEST_LAST_CHECK.lock().unwrap().take();
            *TEST_PREV_OUTCOME.lock().unwrap() = outcome.clone();
            let failed = TEST_FAILED_INSTR_IDXS.lock().unwrap().len() > failed_before;
            let test = *TEST_NAME.lock().unwrap();
            crate::test_timing::record_instr(
                test,
                instr_i,
                TFInstr::str_from_kind(instrs[instr_i].kind),
                instrs[instr_i].flags & SHOULD_FAIL != 0,
                start,
                outcome,
                failed,
                height,
                instrs[instr_i].data_slice(bytes).len(),
            );
        } else {
            // An instruction that didn't parse (only reachable when the fuzzer recovers) made
            // no check, so an instruction about it must not see the one before it.
            *TEST_PREV_OUTCOME.lock().unwrap() = None;
        }

        *TEST_INSTR_C.lock().unwrap() = instr_i + 1; // accounts for end
    }
}

pub(crate) async fn instr_reader(internal_handle: TFLServiceHandle) {
    use zebra_chain::serialization::{ZcashDeserialize, ZcashSerialize};
    let call = internal_handle.call.clone();
    println!("waiting for tip before starting the test...");
    let before_time = Instant::now();
    loop {
        if let Ok(StateResponse::Tip(Some(_))) = (call.state)(StateRequest::Tip).await {
            break;
        } else {
            // warn!("Failed to read tip");
            if before_time.elapsed().as_secs() > 30 {
                panic!("Timeout waiting for test to start.");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    println!("Starting test!");

    if let Some(path) = TEST_INSTR_PATH.lock().unwrap().clone() {
        *TEST_INSTR_BYTES.lock().unwrap() = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => panic!("Invalid test file: {:?}: {}", path, err), // TODO: specifics
        };
    }

    let bytes = TEST_INSTR_BYTES.lock().unwrap().clone();

    // Normal tests PANIC on an unparseable envelope; the fuzzer recovers (TEST_ON_FAIL).
    let on_fail = *TEST_ON_FAIL.lock().unwrap();
    let tf = match uhh(TF::read_from_bytes(&bytes), on_fail) {
        Ok(tf) => tf,
        Err(_) => return, // uhh already panicked (PANIC) or logged (fuzzer)
    };

    *TEST_INSTRS.lock().unwrap() = tf.instrs.clone();

    let params = internal_handle.params;
    read_instrs(internal_handle, &bytes, &tf.instrs).await;

    // Copy everything out and release every lock BEFORE asserting. The panic hook runs before
    // unwinding, while anything the failing statement holds is still held, and it calls
    // dump_test_instrs, which locks TEST_FAILED_INSTR_IDXS and TEST_INSTR_C; the shutdown path
    // below does the same. std Mutex is not reentrant, so a guard alive at either point deadlocks
    // this thread: a failing test hangs in the hook instead of aborting, a passing one at exit.
    let failed_instrs = TEST_FAILED_INSTR_IDXS.lock().unwrap().clone();
    let completed_instrs = *TEST_INSTR_C.lock().unwrap();
    let test = *TEST_NAME.lock().unwrap();

    // Before the asserts, so a failing test still gets its timing row.
    let passed = failed_instrs.is_empty() && completed_instrs == tf.instrs.len();
    crate::test_timing::record_test_end(test, &params, passed);

    assert_eq!(completed_instrs, tf.instrs.len(), "didn't complete test {test}");
    // The (instruction index, message) pairs make a red test self-describing.
    assert!(failed_instrs.is_empty(), "failed test {test}: {failed_instrs:?}");
    println!("Test done, shutting down");
    // #[cfg(feature = "viz_gui")]
    // tokio::time::sleep(Duration::from_secs(120)).await;

    TEST_SHUTDOWN_FN.lock().unwrap()();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHORT: StakingParameters = StakingParameters { period: 10, day_window: 5, action_delay: 6 };

    #[test]
    fn set_params_round_trips_the_staking_calendar() {
        for bootstrap in [BftBootstrap::Supplied, BftBootstrap::FromChain { staking_height: 0, roster_height: 5, activation_height: 300 }] {
            for staking in [PROTOTYPE_STAKING, SHORT] {
                assert_eq!(params_from_bytes(&params_to_bytes(bootstrap, staking)), Some((bootstrap, staking)));
            }
        }
    }

    /// Files written before the calendar was a parameter end after the bootstrap, and must keep
    /// meaning what they meant: the prototype calendar. Writing the prototype calendar produces
    /// those same bytes, so tracked scene files don't change.
    #[test]
    fn set_params_without_a_calendar_is_the_prototype_calendar() {
        assert_eq!(params_from_bytes(&[TF_BOOTSTRAP_SUPPLIED]), Some((BftBootstrap::Supplied, PROTOTYPE_STAKING)));
        assert_eq!(params_to_bytes(BftBootstrap::Supplied, PROTOTYPE_STAKING), vec![TF_BOOTSTRAP_SUPPLIED]);
    }

    #[test]
    fn short_staking_follows_sigma_and_keeps_its_margins() {
        // Harness sigma 3 and liveness allowance 3 give a period of 2 * (3 + 3 + 1) = 14.
        assert_eq!(short_calendar(&HARNESS_PARAMETERS), StakingParameters { period: 14, day_window: 3, action_delay: 4 });
        // The prototype's sigma of 4 gives 16.
        assert_eq!(short_calendar(&PROTOTYPE_PARAMETERS).period, 16);

        let staking = short_calendar(&HARNESS_PARAMETERS);
        // Room for a withdrawal late in a later window: the delay leaves the rest of the period.
        assert!(staking.action_delay <= staking.period - staking.day_window);
    }

    #[test]
    fn set_params_with_a_partial_calendar_is_rejected() {
        let mut bytes = params_to_bytes(BftBootstrap::Supplied, SHORT);
        bytes.pop();
        assert_eq!(params_from_bytes(&bytes), None);
    }
}
