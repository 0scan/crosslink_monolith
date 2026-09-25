//! Wire-level entry points for tests and fuzzing: frame a message as a peer sends it, and turn
//! wire bytes into the request a peer connection hands the inbound service.
//!
//! Decoding goes through the same [`Codec`] every peer connection uses, so what reaches the
//! inbound service has passed real message framing (magic, command, length, checksum) and real
//! deserialization, as if it had just arrived from the peer.

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};
use zebra_chain::{parameters::Network, transaction::UnminedTx};

use crate::{
    protocol::external::{Codec, Message},
    BoxError, PeerSocketAddr, Request,
};

/// The `tx` message a peer on `network` sends to push `transaction`, as bytes on the wire.
pub fn tx_message_bytes(network: &Network, transaction: UnminedTx) -> Result<Vec<u8>, BoxError> {
    let mut codec = Codec::builder().for_network(network).finish();
    let mut bytes = BytesMut::new();
    codec.encode(Message::Tx(transaction), &mut bytes)?;
    Ok(bytes.to_vec())
}

/// The request a peer connection hands the inbound service when `sender` sends `wire` unsolicited.
///
/// This mirrors `Connection::handle_message_as_request`: an unsolicited `tx` message becomes
/// [`Request::PushTransaction`] tagged with its sender, which is what makes the mempool apply its
/// per-peer queue cap. `wire` must hold exactly one message. Only `tx` is supported so far.
pub fn inbound_request_from_wire(
    network: &Network,
    wire: &[u8],
    sender: PeerSocketAddr,
) -> Result<Request, BoxError> {
    let mut codec = Codec::builder().for_network(network).finish();
    let mut bytes = BytesMut::from(wire);
    let message = codec.decode(&mut bytes)?.ok_or("incomplete wire message")?;
    if !bytes.is_empty() {
        return Err(format!("{} bytes after the message", bytes.len()).into());
    }
    match message {
        Message::Tx(transaction) => Ok(Request::PushTransaction(transaction, Some(sender))),
        other => Err(format!("unsupported inbound message: {}", other.command()).into()),
    }
}
