// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

//! Reads a worker's WebSocket frames: the worker-to-engine half of its
//! connection.
//!
//! tungstenite ends a connection whose peer sends a frame or message over its
//! read limits: the failed read fuses the stream, so one oversized call from a
//! worker would also fail every other call in flight on its connection.
//!
//! This reader keeps every message up to its limit, whether it arrives as one
//! frame or as fragments. A larger message is read to its end and dropped
//! without being buffered: only its first and last [`EDGE_BYTES`] are kept, so
//! the engine can tell which request it was and answer it (see
//! [`Oversized::envelope`]). The connection stays open.
//!
//! Frame headers are parsed by tungstenite's [`FrameHeader`], and frames are
//! checked as tungstenite checks a client's: masked, no reserved bits, control
//! frames final and at most 125 bytes, continuation frames only inside a
//! message, and valid UTF-8 in a text message that is kept. A violation is an
//! error that ends the connection.

use std::io::Cursor;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_tungstenite::tungstenite::{
    Bytes, Error, Message, Utf8Bytes,
    error::ProtocolError,
    protocol::{
        CloseFrame,
        frame::{
            FrameHeader,
            coding::{Control, Data, OpCode},
        },
    },
};
use uuid::Uuid;

/// Bytes kept from each end of a message over the limit.
pub(crate) const EDGE_BYTES: usize = 64 * 1024;
/// How much the reader asks the socket for at a time.
const READ_CHUNK: usize = 128 * 1024;
/// Largest control frame payload (RFC 6455 section 5.5).
const MAX_CONTROL_PAYLOAD: u64 = 125;

/// What [`InboundReader::next`] reads.
#[derive(Debug)]
pub(crate) enum Inbound {
    /// A data message within the limit, or a control frame.
    Message(Message),
    /// A data message over the limit, read to its end and dropped.
    Oversized(Oversized),
}

/// What is kept of a data message over the limit.
#[derive(Debug)]
pub(crate) struct Oversized {
    /// Payload size of the whole message, in bytes.
    pub(crate) size: u64,
    /// The first [`EDGE_BYTES`] of the payload.
    pub(crate) head: Vec<u8>,
    /// The last [`EDGE_BYTES`] of the payload (none of them in `head`).
    pub(crate) tail: Vec<u8>,
}

/// Top-level fields of an oversized JSON envelope.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct EnvelopeFields {
    pub(crate) message_type: Option<String>,
    pub(crate) invocation_id: Option<Uuid>,
    pub(crate) function_id: Option<String>,
}

impl Oversized {
    /// Recovers the envelope's `type`, `invocation_id` and `function_id`.
    ///
    /// SDKs order fields differently: Node, the browser SDK and Rust write
    /// `type` and both ids before `data` or `result`, Python writes
    /// `invocation_id` after `data` and `type` last, and Go sorts keys, so
    /// `data` and `error` come before both ids. So the head is read forward and
    /// the tail backward, each only at the top level of the object; a field
    /// found in the head wins.
    pub(crate) fn envelope(&self) -> EnvelopeFields {
        let mut fields = EnvelopeFields::default();
        for (key, value) in head_members(&self.head)
            .into_iter()
            .chain(tail_members(&self.tail))
        {
            match key.as_str() {
                "type" if fields.message_type.is_none() => fields.message_type = Some(value),
                "invocation_id" if fields.invocation_id.is_none() => {
                    fields.invocation_id = Uuid::parse_str(&value).ok();
                }
                "function_id" if fields.function_id.is_none() => fields.function_id = Some(value),
                _ => {}
            }
        }
        fields
    }
}

/// Reads a worker's frames, keeping messages up to `limit` bytes.
pub(crate) struct InboundReader<R> {
    io: R,
    /// Bytes read from the socket: at most a partial frame header, plus one
    /// read. Those before `pos` are consumed.
    buffer: Vec<u8>,
    pos: usize,
    limit: usize,
    frame: Option<FrameState>,
    message: Option<MessageState>,
    closed: bool,
    ended: bool,
    #[cfg(test)]
    peak_buffered: usize,
}

/// The frame being read.
struct FrameState {
    opcode: OpCode,
    is_final: bool,
    mask: [u8; 4],
    len: u64,
    /// Payload bytes read so far.
    read: u64,
    /// The payload of a control frame.
    control: Vec<u8>,
}

/// The data message being read, from its first frame to its final one.
struct MessageState {
    text: bool,
    /// Payload size of its frames so far, counting the whole current frame.
    size: u64,
    body: Body,
}

enum Body {
    Kept(Vec<u8>),
    Skipped { head: Vec<u8>, tail: Vec<u8> },
}

impl<R> InboundReader<R>
where
    R: AsyncRead + Unpin,
{
    pub(crate) fn new(io: R, limit: usize) -> Self {
        Self {
            io,
            buffer: Vec::with_capacity(READ_CHUNK),
            pos: 0,
            limit,
            frame: None,
            message: None,
            closed: false,
            ended: false,
            #[cfg(test)]
            peak_buffered: 0,
        }
    }

    /// The next message or control frame, `None` once the connection has
    /// ended. Cancel safe: all progress is kept in `self`, so a dropped call
    /// loses nothing.
    pub(crate) async fn next(&mut self) -> Option<Result<Inbound, Error>> {
        if self.ended {
            return None;
        }
        loop {
            let progress = self.advance();
            #[cfg(test)]
            {
                self.peak_buffered = self.peak_buffered.max(self.buffered());
            }
            match progress {
                Ok(Some(inbound)) => {
                    if matches!(inbound, Inbound::Message(Message::Close(_))) {
                        self.closed = true;
                    }
                    return Some(Ok(inbound));
                }
                Ok(None) => {}
                Err(error) => {
                    self.ended = true;
                    return Some(Err(error));
                }
            }
            // Everything buffered is consumed but a partial frame header, so
            // this moves at most a few bytes.
            self.buffer.drain(..self.pos);
            self.pos = 0;
            self.buffer.reserve(READ_CHUNK);
            match self.io.read_buf(&mut self.buffer).await {
                Ok(0) => {
                    self.ended = true;
                    let clean = self.closed && self.frame.is_none() && self.buffer.is_empty();
                    return (!clean).then_some(Err(Error::Protocol(
                        ProtocolError::ResetWithoutClosingHandshake,
                    )));
                }
                Ok(_) => {}
                Err(error) => {
                    self.ended = true;
                    return Some(Err(Error::Io(error)));
                }
            }
        }
    }

    /// Consumes buffered bytes until a message or control frame is complete
    /// (`Some`), or more bytes are needed (`None`).
    fn advance(&mut self) -> Result<Option<Inbound>, Error> {
        loop {
            if self.frame.is_none() {
                let mut cursor = Cursor::new(&self.buffer[self.pos..]);
                let Some((header, len)) = FrameHeader::parse(&mut cursor)? else {
                    return Ok(None);
                };
                self.pos += cursor.position() as usize;
                self.frame = Some(self.start_frame(header, len)?);
            }

            let Self {
                frame,
                message,
                buffer,
                pos,
                limit,
                ..
            } = self;
            let frame = frame.as_mut().expect("a frame is being read");
            let available = usize::try_from(frame.len - frame.read)
                .unwrap_or(usize::MAX)
                .min(buffer.len() - *pos);
            if available > 0 {
                consume(
                    frame,
                    message.as_mut(),
                    &mut buffer[*pos..*pos + available],
                    *limit,
                );
                *pos += available;
            }
            if frame.read < frame.len {
                return Ok(None);
            }

            let frame = self.frame.take().expect("a frame is being read");
            if let Some(inbound) = self.finish_frame(frame)? {
                return Ok(Some(inbound));
            }
        }
    }

    fn start_frame(&mut self, header: FrameHeader, len: u64) -> Result<FrameState, Error> {
        if header.rsv1 || header.rsv2 || header.rsv3 {
            return Err(Error::Protocol(ProtocolError::NonZeroReservedBits));
        }
        let Some(mask) = header.mask else {
            return Err(Error::Protocol(ProtocolError::UnmaskedFrameFromClient));
        };
        match header.opcode {
            OpCode::Control(_) => {
                if !header.is_final {
                    return Err(Error::Protocol(ProtocolError::FragmentedControlFrame));
                }
                if len > MAX_CONTROL_PAYLOAD {
                    return Err(Error::Protocol(ProtocolError::ControlFrameTooBig));
                }
            }
            OpCode::Data(data) => {
                let starts = data != Data::Continue;
                match (starts, self.message.is_some()) {
                    (false, false) => {
                        return Err(Error::Protocol(ProtocolError::UnexpectedContinueFrame));
                    }
                    (true, true) => {
                        return Err(Error::Protocol(ProtocolError::ExpectedFragment(data)));
                    }
                    (true, false) => {
                        self.message = Some(MessageState {
                            text: data == Data::Text,
                            size: 0,
                            body: Body::Kept(Vec::new()),
                        });
                    }
                    (false, true) => {}
                }
                let message = self.message.as_mut().expect("a message is being read");
                message.size = message.size.saturating_add(len);
                if message.size > self.limit as u64 {
                    message.skip();
                }
            }
        }
        Ok(FrameState {
            opcode: header.opcode,
            is_final: header.is_final,
            mask,
            len,
            read: 0,
            control: Vec::new(),
        })
    }

    fn finish_frame(&mut self, frame: FrameState) -> Result<Option<Inbound>, Error> {
        let message = match frame.opcode {
            OpCode::Control(Control::Ping) => Message::Ping(Bytes::from(frame.control)),
            OpCode::Control(Control::Pong) => Message::Pong(Bytes::from(frame.control)),
            OpCode::Control(Control::Close) => Message::Close(close_frame(frame.control)?),
            OpCode::Control(Control::Reserved(code)) | OpCode::Data(Data::Reserved(code)) => {
                return Err(Error::Protocol(ProtocolError::InvalidOpcode(code)));
            }
            OpCode::Data(_) if !frame.is_final => return Ok(None),
            OpCode::Data(_) => {
                let message = self.message.take().expect("a message is being read");
                return message.finish().map(Some);
            }
        };
        Ok(Some(Inbound::Message(message)))
    }

    /// Bytes this reader holds, by capacity.
    #[cfg(test)]
    fn buffered(&self) -> usize {
        let message = match self.message.as_ref().map(|message| &message.body) {
            Some(Body::Kept(data)) => data.capacity(),
            Some(Body::Skipped { head, tail }) => head.capacity() + tail.capacity(),
            None => 0,
        };
        let control = self
            .frame
            .as_ref()
            .map_or(0, |frame| frame.control.capacity());
        self.buffer.capacity() + message + control
    }
}

impl MessageState {
    /// Over the limit: from now on, only the edges of the payload are kept.
    fn skip(&mut self) {
        let Body::Kept(data) = &mut self.body else {
            return;
        };
        let data = std::mem::take(data);
        let mut head = Vec::with_capacity(EDGE_BYTES);
        head.extend_from_slice(&data[..data.len().min(EDGE_BYTES)]);
        let rest = &data[head.len()..];
        let mut tail = Vec::with_capacity(2 * EDGE_BYTES);
        tail.extend_from_slice(&rest[rest.len().saturating_sub(EDGE_BYTES)..]);
        self.body = Body::Skipped { head, tail };
    }

    fn finish(self) -> Result<Inbound, Error> {
        match self.body {
            Body::Kept(data) if self.text => Utf8Bytes::try_from(data)
                .map(|text| Inbound::Message(Message::Text(text)))
                .map_err(|error| Error::Utf8(error.to_string())),
            Body::Kept(data) => Ok(Inbound::Message(Message::Binary(Bytes::from(data)))),
            Body::Skipped { head, mut tail } => {
                tail.drain(..tail.len().saturating_sub(EDGE_BYTES));
                Ok(Inbound::Oversized(Oversized {
                    size: self.size,
                    head,
                    tail,
                }))
            }
        }
    }
}

/// Takes `chunk`, the next payload bytes of `frame`, still masked.
fn consume(
    frame: &mut FrameState,
    message: Option<&mut MessageState>,
    chunk: &mut [u8],
    limit: usize,
) {
    let offset = frame.read;
    frame.read += chunk.len() as u64;
    let message = match frame.opcode {
        OpCode::Data(_) => message.expect("a data frame belongs to a message"),
        OpCode::Control(_) => {
            unmask(chunk, frame.mask, offset);
            frame.control.extend_from_slice(chunk);
            return;
        }
    };
    match &mut message.body {
        Body::Kept(data) => {
            let needed = data.len() + chunk.len();
            if needed > data.capacity() {
                // The size of a final frame's message is known: allocate it
                // once. Fragments grow by doubling, never past the limit.
                let target = if frame.is_final {
                    message.size as usize
                } else {
                    (data.capacity() * 2).clamp(needed, limit.max(needed))
                };
                data.reserve_exact(target - data.len());
            }
            let start = data.len();
            data.extend_from_slice(chunk);
            unmask(&mut data[start..], frame.mask, offset);
        }
        Body::Skipped { head, tail } => {
            let mut chunk = chunk;
            let mut offset = offset;
            if head.len() < EDGE_BYTES {
                let take = (EDGE_BYTES - head.len()).min(chunk.len());
                let (front, rest) = chunk.split_at_mut(take);
                unmask(front, frame.mask, offset);
                head.extend_from_slice(front);
                chunk = rest;
                offset += take as u64;
            }
            // At least EDGE_BYTES of the same frame follow any earlier byte,
            // so only a frame's last EDGE_BYTES can end up in the tail.
            let tail_from = frame.len.saturating_sub(EDGE_BYTES as u64);
            let skip = tail_from.saturating_sub(offset).min(chunk.len() as u64) as usize;
            let rest = &mut chunk[skip..];
            if !rest.is_empty() {
                unmask(rest, frame.mask, offset + skip as u64);
                tail.extend_from_slice(rest);
                if tail.len() > 2 * EDGE_BYTES {
                    tail.drain(..tail.len() - EDGE_BYTES);
                }
            }
        }
    }
}

/// Unmasks `buf`, the payload bytes from `offset` on (RFC 6455 section 5.3).
fn unmask(buf: &mut [u8], mask: [u8; 4], offset: u64) {
    let shift = (offset % 4) as usize;
    let key: [u8; 4] = std::array::from_fn(|index| mask[(shift + index) % 4]);
    let word = u32::from_ne_bytes(key);
    let (words, rest) = buf.as_chunks_mut::<4>();
    for chunk in words {
        *chunk = (u32::from_ne_bytes(*chunk) ^ word).to_ne_bytes();
    }
    for (byte, key) in rest.iter_mut().zip(key) {
        *byte ^= key;
    }
}

fn close_frame(payload: Vec<u8>) -> Result<Option<CloseFrame>, Error> {
    match payload.len() {
        0 => Ok(None),
        1 => Err(Error::Protocol(ProtocolError::InvalidCloseSequence)),
        _ => {
            let code = u16::from_be_bytes([payload[0], payload[1]]).into();
            let reason = Utf8Bytes::try_from(Bytes::from(payload).slice(2..))
                .map_err(|error| Error::Utf8(error.to_string()))?;
            Ok(Some(CloseFrame { code, reason }))
        }
    }
}

fn is_json_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// A JSON string literal, quotes included, as a `String`.
fn decode_string(literal: &[u8]) -> Option<String> {
    serde_json::from_slice(literal).ok()
}

/// The string-valued members at the top level of the JSON object `bytes`
/// starts, up to where `bytes` is cut off.
fn head_members(bytes: &[u8]) -> Vec<(String, String)> {
    let mut members = Vec::new();
    let mut pos = skip_space(bytes, 0);
    if bytes.get(pos) != Some(&b'{') {
        return members;
    }
    pos += 1;
    loop {
        pos = skip_space(bytes, pos);
        let Some(key_end) = string_end(bytes, pos) else {
            break;
        };
        let Some(key) = decode_string(&bytes[pos..key_end]) else {
            break;
        };
        pos = skip_space(bytes, key_end);
        if bytes.get(pos) != Some(&b':') {
            break;
        }
        pos = skip_space(bytes, pos + 1);
        let Some(value_end) = value_end(bytes, pos) else {
            break;
        };
        if bytes[pos] == b'"'
            && let Some(value) = decode_string(&bytes[pos..value_end])
        {
            members.push((key, value));
        }
        pos = skip_space(bytes, value_end);
        if bytes.get(pos) != Some(&b',') {
            break;
        }
        pos += 1;
    }
    members
}

fn skip_space(bytes: &[u8], mut pos: usize) -> usize {
    while bytes.get(pos).copied().is_some_and(is_json_space) {
        pos += 1;
    }
    pos
}

/// The end (exclusive) of the string literal starting at `start`.
fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut pos = start + 1;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => pos += 2,
            b'"' => return Some(pos + 1),
            _ => pos += 1,
        }
    }
    None
}

/// The end (exclusive) of the JSON value starting at `start`.
fn value_end(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start)? {
        b'"' => string_end(bytes, start),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut pos = start;
            while pos < bytes.len() {
                match bytes[pos] {
                    b'"' => {
                        pos = string_end(bytes, pos)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(pos + 1);
                        }
                    }
                    _ => {}
                }
                pos += 1;
            }
            None
        }
        _ => {
            let end = bytes[start..]
                .iter()
                .position(|&byte| matches!(byte, b',' | b'}' | b']') || is_json_space(byte))
                .map(|len| start + len)?;
            (end > start).then_some(end)
        }
    }
}

/// The string-valued members at the top level of the JSON object `bytes`
/// ends, read backward from its closing brace until `bytes` is cut off.
fn tail_members(bytes: &[u8]) -> Vec<(String, String)> {
    let mut members = Vec::new();
    let mut end = skip_space_back(bytes, bytes.len());
    if end == 0 || bytes[end - 1] != b'}' {
        return members;
    }
    end -= 1;
    loop {
        end = skip_space_back(bytes, end);
        let Some(value_start) = value_start(bytes, end) else {
            break;
        };
        let value = (bytes[value_start] == b'"')
            .then(|| decode_string(&bytes[value_start..end]))
            .flatten();
        end = skip_space_back(bytes, value_start);
        if end == 0 || bytes[end - 1] != b':' {
            break;
        }
        end = skip_space_back(bytes, end - 1);
        let Some(key_start) = string_start(bytes, end) else {
            break;
        };
        let Some(key) = decode_string(&bytes[key_start..end]) else {
            break;
        };
        if let Some(value) = value {
            members.push((key, value));
        }
        end = skip_space_back(bytes, key_start);
        if end == 0 || bytes[end - 1] != b',' {
            break;
        }
        end -= 1;
    }
    members
}

fn skip_space_back(bytes: &[u8], mut end: usize) -> usize {
    while end > 0 && is_json_space(bytes[end - 1]) {
        end -= 1;
    }
    end
}

/// The start of the string literal whose closing quote is `bytes[end - 1]`.
///
/// A quote opens the literal when an even number of backslashes precedes it.
/// When the backslashes run to the start of `bytes`, their number is unknown,
/// and so is the answer.
fn string_start(bytes: &[u8], end: usize) -> Option<usize> {
    if end < 2 || bytes[end - 1] != b'"' {
        return None;
    }
    let mut pos = end - 1;
    while pos > 0 {
        pos -= 1;
        if bytes[pos] != b'"' {
            continue;
        }
        let backslashes = bytes[..pos]
            .iter()
            .rev()
            .take_while(|&&byte| byte == b'\\')
            .count();
        if backslashes == pos {
            return None;
        }
        if backslashes % 2 == 0 {
            return Some(pos);
        }
    }
    None
}

/// The start of the JSON value ending at `end` (exclusive).
fn value_start(bytes: &[u8], end: usize) -> Option<usize> {
    match bytes.get(end.checked_sub(1)?)? {
        b'"' => string_start(bytes, end),
        b'}' | b']' => {
            let mut depth = 0usize;
            let mut pos = end;
            while pos > 0 {
                pos -= 1;
                match bytes[pos] {
                    b'"' => {
                        pos = string_start(bytes, pos + 1)?;
                    }
                    b'}' | b']' => depth += 1,
                    b'{' | b'[' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(pos);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        _ => {
            let start = bytes[..end]
                .iter()
                .rposition(|&byte| matches!(byte, b':' | b',' | b'{' | b'[') || is_json_space(byte))
                .map(|pos| pos + 1)?;
            (start < end).then_some(start)
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncWriteExt, DuplexStream, duplex};
    use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

    use super::*;

    const MIB: usize = 1024 * 1024;
    const MASK: [u8; 4] = [0x37, 0xfa, 0x21, 0x3d];
    const TEXT: u8 = 0x1;
    const BINARY: u8 = 0x2;
    const CONTINUE: u8 = 0x0;
    const CLOSE: u8 = 0x8;
    const PING: u8 = 0x9;

    /// A client frame: masked with `mask`, as RFC 6455 requires of clients.
    fn frame(opcode: u8, fin: bool, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
        let mut out = header(opcode, fin, payload.len() as u64, Some(mask));
        out.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        out
    }

    fn header(opcode: u8, fin: bool, len: u64, mask: Option<[u8; 4]>) -> Vec<u8> {
        let mut out = vec![if fin { 0x80 } else { 0 } | opcode];
        let masked = if mask.is_some() { 0x80 } else { 0 };
        match len {
            0..=125 => out.push(masked | len as u8),
            126..=0xFFFF => {
                out.push(masked | 126);
                out.extend((len as u16).to_be_bytes());
            }
            _ => {
                out.push(masked | 127);
                out.extend(len.to_be_bytes());
            }
        }
        out.extend(mask.unwrap_or_default());
        out
    }

    fn reader(limit: usize) -> (DuplexStream, InboundReader<DuplexStream>) {
        let (client, server) = duplex(MIB);
        (client, InboundReader::new(server, limit))
    }

    /// Writes `bytes` from a separate task, so the reader drains the pipe.
    fn send(mut client: DuplexStream, bytes: Vec<u8>) -> tokio::task::JoinHandle<DuplexStream> {
        tokio::spawn(async move {
            client.write_all(&bytes).await.unwrap();
            client
        })
    }

    async fn next(reader: &mut InboundReader<DuplexStream>) -> Inbound {
        reader.next().await.expect("open").expect("valid")
    }

    async fn next_error(reader: &mut InboundReader<DuplexStream>) -> Error {
        match reader.next().await {
            Some(Err(error)) => error,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    fn text(inbound: Inbound) -> String {
        match inbound {
            Inbound::Message(Message::Text(text)) => text.to_string(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn oversized(inbound: Inbound) -> Oversized {
        match inbound {
            Inbound::Oversized(oversized) => oversized,
            other => panic!("expected an oversized message, got {other:?}"),
        }
    }

    /// Payload bytes that differ at every position, so a misplaced edge shows.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|index| b'a' + (index % 23) as u8).collect()
    }

    #[test]
    fn unmask_at_any_offset_matches_bytewise_unmasking() {
        let payload = pattern(1031);
        let masked: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ MASK[index % 4])
            .collect();
        for start in 0..8 {
            for end in [start, start + 1, start + 5, 1031] {
                let mut chunk = masked[start..end].to_vec();
                unmask(&mut chunk, MASK, start as u64);
                assert_eq!(chunk, payload[start..end], "{start}..{end}");
            }
        }
    }

    #[tokio::test]
    async fn reads_messages_within_the_limit() {
        let (client, mut reader) = reader(MIB);
        let mut bytes = frame(TEXT, true, b"{\"type\":\"ping\"}", MASK);
        bytes.extend(frame(BINARY, true, &[1, 2, 3], MASK));
        // A fragmented text message with a ping between its fragments; a
        // fragment boundary splits "é".
        let message = "héllo wörld".as_bytes();
        bytes.extend(frame(TEXT, false, &message[..2], MASK));
        bytes.extend(frame(PING, true, b"p", [1, 2, 3, 4]));
        bytes.extend(frame(CONTINUE, false, &message[2..7], [9, 9, 9, 9]));
        bytes.extend(frame(CONTINUE, true, &message[7..], MASK));
        // Exactly the limit, in one frame.
        bytes.extend(frame(TEXT, true, &pattern(MIB), MASK));
        send(client, bytes);

        assert_eq!(text(next(&mut reader).await), "{\"type\":\"ping\"}");
        assert!(matches!(
            next(&mut reader).await,
            Inbound::Message(Message::Binary(data)) if data.as_ref() == [1, 2, 3]
        ));
        assert!(matches!(
            next(&mut reader).await,
            Inbound::Message(Message::Ping(data)) if data.as_ref() == b"p"
        ));
        assert_eq!(text(next(&mut reader).await), "héllo wörld");
        assert_eq!(text(next(&mut reader).await).as_bytes(), pattern(MIB));
    }

    #[tokio::test]
    async fn drops_a_single_frame_over_the_limit_and_keeps_its_edges() {
        let payload = pattern(3 * MIB + 7);
        let (client, mut reader) = reader(MIB);
        let mut bytes = frame(TEXT, true, &payload, MASK);
        bytes.extend(frame(TEXT, true, b"after", MASK));
        send(client, bytes);

        let dropped = oversized(next(&mut reader).await);
        assert_eq!(dropped.size, payload.len() as u64);
        assert_eq!(dropped.head, payload[..EDGE_BYTES]);
        assert_eq!(dropped.tail, payload[payload.len() - EDGE_BYTES..]);
        assert_eq!(text(next(&mut reader).await), "after");
    }

    #[tokio::test]
    async fn drops_a_fragmented_message_over_the_limit_with_control_frames_between() {
        let payload = pattern(3 * MIB + 7);
        let (client, mut reader) = reader(MIB);
        let mut bytes = Vec::new();
        // Fragments of every size around the edges: some shorter than an
        // edge, so the head and the tail span several of them.
        let mut offset = 0;
        for (index, len) in [100, 70_000, 400_000, MIB, 1, MIB, 5_000]
            .into_iter()
            .enumerate()
        {
            let end = (offset + len).min(payload.len());
            let opcode = if index == 0 { TEXT } else { CONTINUE };
            bytes.extend(frame(opcode, false, &payload[offset..end], MASK));
            bytes.extend(frame(PING, true, &[index as u8], MASK));
            offset = end;
        }
        let rest = &payload[offset..];
        bytes.extend(frame(CONTINUE, false, &rest[..rest.len() - 10], MASK));
        bytes.extend(frame(CONTINUE, true, &rest[rest.len() - 10..], MASK));
        bytes.extend(frame(TEXT, true, b"after", MASK));
        send(client, bytes);

        for index in 0..7u8 {
            assert!(matches!(
                next(&mut reader).await,
                Inbound::Message(Message::Ping(data)) if data.as_ref() == [index]
            ));
        }
        let dropped = oversized(next(&mut reader).await);
        assert_eq!(dropped.size, payload.len() as u64);
        assert_eq!(dropped.head, payload[..EDGE_BYTES]);
        assert_eq!(dropped.tail, payload[payload.len() - EDGE_BYTES..]);
        assert_eq!(text(next(&mut reader).await), "after");
    }

    #[tokio::test]
    async fn close_frame_then_end_of_stream() {
        let (client, mut reader) = reader(MIB);
        let mut payload = 1000u16.to_be_bytes().to_vec();
        payload.extend(b"bye");
        let client = send(client, frame(CLOSE, true, &payload, MASK));
        drop(client.await.unwrap());

        match next(&mut reader).await {
            Inbound::Message(Message::Close(Some(close))) => {
                assert_eq!(close.code, CloseCode::Normal);
                assert_eq!(close.reason.as_str(), "bye");
            }
            other => panic!("expected a close frame, got {other:?}"),
        }
        assert!(reader.next().await.is_none());
    }

    #[tokio::test]
    async fn end_of_stream_without_a_close_frame_is_an_error() {
        let (client, mut reader) = reader(MIB);
        let client = send(client, frame(TEXT, false, b"half", MASK));
        drop(client.await.unwrap());
        assert!(matches!(
            next_error(&mut reader).await,
            Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)
        ));
        assert!(reader.next().await.is_none());
    }

    #[tokio::test]
    async fn protocol_violations_end_the_connection() {
        let mut unmasked = header(TEXT, true, 2, None);
        unmasked.extend(b"hi");
        let mut reserved_bits = frame(TEXT, true, b"hi", MASK);
        reserved_bits[0] |= 0x40;
        let mut fragmented_ping = frame(PING, false, b"p", MASK);
        fragmented_ping.extend(frame(CONTINUE, true, b"p", MASK));
        let mut interleaved = frame(TEXT, false, b"a", MASK);
        interleaved.extend(frame(TEXT, true, b"b", MASK));
        type Expected = fn(&Error) -> bool;
        let cases: [(&str, Vec<u8>, Expected); 7] = [
            ("unmasked frame", unmasked, |error| {
                matches!(
                    error,
                    Error::Protocol(ProtocolError::UnmaskedFrameFromClient)
                )
            }),
            ("reserved bits", reserved_bits, |error| {
                matches!(error, Error::Protocol(ProtocolError::NonZeroReservedBits))
            }),
            ("fragmented control frame", fragmented_ping, |error| {
                matches!(
                    error,
                    Error::Protocol(ProtocolError::FragmentedControlFrame)
                )
            }),
            (
                "control frame over 125 bytes",
                frame(PING, true, &[0; 126], MASK),
                |error| matches!(error, Error::Protocol(ProtocolError::ControlFrameTooBig)),
            ),
            (
                "continuation without a message",
                frame(CONTINUE, true, b"x", MASK),
                |error| {
                    matches!(
                        error,
                        Error::Protocol(ProtocolError::UnexpectedContinueFrame)
                    )
                },
            ),
            ("new message inside a message", interleaved, |error| {
                matches!(error, Error::Protocol(ProtocolError::ExpectedFragment(_)))
            }),
            (
                "invalid UTF-8 in a text message",
                frame(TEXT, true, &[b'a', 0xff, b'b'], MASK),
                |error| matches!(error, Error::Utf8(_)),
            ),
        ];
        for (case, bytes, expected) in cases {
            let (client, mut reader) = reader(MIB);
            send(client, bytes);
            let error = next_error(&mut reader).await;
            assert!(expected(&error), "{case}: {error:?}");
            assert!(reader.next().await.is_none(), "{case}: reader must end");
        }
    }

    #[tokio::test]
    async fn a_tungstenite_client_is_read_the_same() {
        use futures_util::SinkExt;
        use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Role};

        let (client, mut reader) = reader(MIB);
        let mut client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let large = "é".repeat(MIB / 2);
        tokio::spawn(async move {
            client.send(Message::text("small")).await.unwrap();
            client.send(Message::text(large)).await.unwrap();
            client
                .send(Message::text("x".repeat(MIB + 1)))
                .await
                .unwrap();
            client
                .send(Message::Ping(b"p".to_vec().into()))
                .await
                .unwrap();
            client.close(None).await.unwrap();
            client
        });
        assert_eq!(text(next(&mut reader).await), "small");
        assert_eq!(text(next(&mut reader).await), "é".repeat(MIB / 2));
        assert_eq!(oversized(next(&mut reader).await).size, MIB as u64 + 1);
        assert!(matches!(
            next(&mut reader).await,
            Inbound::Message(Message::Ping(_))
        ));
        assert!(matches!(
            next(&mut reader).await,
            Inbound::Message(Message::Close(None))
        ));
    }

    /// Streams `size` payload bytes as fragments of `fragment` bytes (one
    /// frame when `fragment` is `None`), with a zero mask so the test does
    /// not spend its time masking.
    async fn stream_message(mut client: DuplexStream, size: u64, fragment: Option<u64>) {
        let block = vec![b'x'; MIB];
        let fragment = fragment.unwrap_or(size);
        let mut sent = 0;
        while sent < size {
            let len = fragment.min(size - sent);
            let opcode = if sent == 0 { TEXT } else { CONTINUE };
            client
                .write_all(&header(opcode, sent + len == size, len, Some([0; 4])))
                .await
                .unwrap();
            let mut left = len;
            while left > 0 {
                let chunk = left.min(block.len() as u64) as usize;
                client.write_all(&block[..chunk]).await.unwrap();
                left -= chunk as u64;
            }
            sent += len;
        }
        client.shutdown().await.unwrap();
    }

    async fn peak_reading(size: u64, fragment: Option<u64>) -> (Inbound, usize) {
        let limit = crate::workers::worker::MAX_JSON_MESSAGE_BYTES;
        let (client, mut reader) = reader(limit);
        tokio::spawn(stream_message(client, size, fragment));
        let inbound = next(&mut reader).await;
        (inbound, reader.peak_buffered)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn memory_stays_bounded_while_dropping_a_200_mib_message() {
        let limit = crate::workers::worker::MAX_JSON_MESSAGE_BYTES;
        let size = 200 * MIB as u64;

        // One 200 MiB frame: dropped from its header on, so only the read
        // buffer and the edges are ever held.
        let (inbound, peak) = peak_reading(size, None).await;
        assert_eq!(oversized(inbound).size, size);
        assert!(peak <= MIB, "single frame: peak {peak} bytes");

        // 1 MiB fragments: the first 64 MiB are kept until the message
        // crosses the limit, then released.
        let (inbound, peak) = peak_reading(size, Some(MIB as u64)).await;
        assert_eq!(oversized(inbound).size, size);
        assert!(peak <= limit + MIB, "fragments: peak {peak} bytes");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_message_of_exactly_the_limit_is_kept_in_one_allocation() {
        let limit = crate::workers::worker::MAX_JSON_MESSAGE_BYTES;
        for fragment in [None, Some(MIB as u64)] {
            let (inbound, peak) = peak_reading(limit as u64, fragment).await;
            assert_eq!(text(inbound).len(), limit, "{fragment:?}");
            assert!(peak <= limit + MIB, "{fragment:?}: peak {peak} bytes");
        }
    }

    fn edges(envelope: &str) -> Oversized {
        let bytes = envelope.as_bytes();
        let head = bytes[..EDGE_BYTES.min(bytes.len())].to_vec();
        let tail = bytes[bytes.len().saturating_sub(EDGE_BYTES).max(head.len())..].to_vec();
        Oversized {
            size: bytes.len() as u64,
            head,
            tail,
        }
    }

    /// A large JSON value whose strings hold quotes, backslashes, braces and
    /// a decoy `invocation_id`.
    fn large_data() -> String {
        let decoy = r#"{\"type\":\"invocationresult\",\"invocation_id\":\"00000000-0000-0000-0000-000000000000\"} \\\" }] "#;
        let item = format!(r#"{{"k":"{decoy}","n":[1,2,{{"x":null}}],"invocation_id":"decoy"}}"#);
        format!("[{}]", vec![item; 2_000].join(","))
    }

    #[test]
    fn envelope_fields_are_found_in_every_sdk_field_order() {
        let id = "7c9e6679-7425-40de-944b-e07fc1f90ae7";
        let data = large_data();
        assert!(data.len() > 2 * EDGE_BYTES);
        let expected = |message_type: &str| EnvelopeFields {
            message_type: Some(message_type.to_string()),
            invocation_id: Some(id.parse().unwrap()),
            function_id: Some("orders::create".to_string()),
        };
        let cases = [
            (
                "Node, browser and Rust invoke",
                format!(
                    r#"{{"type":"invokefunction","invocation_id":"{id}","function_id":"orders::create","data":{data},"traceparent":"00-abc-def-01"}}"#
                ),
                expected("invokefunction"),
            ),
            (
                "Python invoke, 0.24 spacing",
                format!(
                    r#"{{"function_id": "orders::create", "data": {data}, "metadata": {{"a": "}}"}}, "invocation_id": "{id}", "traceparent": "t", "type": "invokefunction"}}"#
                ),
                expected("invokefunction"),
            ),
            (
                "Go invoke, sorted keys",
                format!(
                    r#"{{"action":{{"type":"void"}},"baggage":"b","data":{data},"function_id":"orders::create","invocation_id":"{id}","metadata":null,"type":"invokefunction"}}"#
                ),
                expected("invokefunction"),
            ),
            (
                "Python result returning null after type",
                format!(
                    r#"{{"invocation_id":"{id}","function_id":"orders::create","error":{data},"type":"invocationresult","result":null}}"#
                ),
                expected("invocationresult"),
            ),
            (
                "Go result, sorted keys",
                format!(
                    r#"{{"baggage":"b","function_id":"orders::create","invocation_id":"{id}","result":{data},"traceparent":"t","type":"invocationresult"}}"#
                ),
                expected("invocationresult"),
            ),
            (
                "pretty-printed result with a string value",
                format!(
                    "{{\n  \"type\": \"invocationresult\",\n  \"result\": \"{}\",\n  \"invocation_id\": \"{id}\",\n  \"function_id\": \"orders::create\"\n}}\n",
                    "\\\"q\\\\".repeat(50_000)
                ),
                expected("invocationresult"),
            ),
            (
                "Rust 0.24 void invoke without an id",
                format!(
                    r#"{{"type":"invokefunction","invocation_id":null,"function_id":"orders::create","data":{data}}}"#
                ),
                EnvelopeFields {
                    message_type: Some("invokefunction".to_string()),
                    invocation_id: None,
                    function_id: Some("orders::create".to_string()),
                },
            ),
            ("not JSON", "x".repeat(300_000), EnvelopeFields::default()),
        ];
        for (case, envelope, expected) in cases {
            assert_eq!(edges(&envelope).envelope(), expected, "{case}");
        }
    }

    #[test]
    fn a_tail_cut_inside_a_backslash_run_is_not_guessed() {
        let member = vec![("type".to_string(), "invocationresult".to_string())];
        // Two backslashes after a known byte: the quote is unescaped.
        assert_eq!(tail_members(br#"x\\"type":"invocationresult"}"#), member);
        // One backslash: the quote is escaped, and the key never opens.
        assert!(tail_members(br#"x\"type":"invocationresult"}"#).is_empty());
        // Backslashes from the cut on: their number is unknown, so the quote
        // after them cannot be placed.
        assert!(tail_members(br#"\\"type":"invocationresult"}"#).is_empty());
    }
}
