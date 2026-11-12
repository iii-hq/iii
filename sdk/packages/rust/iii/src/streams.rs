//! Call-native streams: [`InputStream`] and [`OutputStream`] are values in
//! call payloads and results.
//!
//! A stream crosses a call as the ref of one end of an engine pipe
//! (`{ channel_id, access_key, direction }`, the JSON every SDK reads). The
//! SDK makes the pipe and moves the bytes:
//!
//! - What it sends (a call's payload, a handler's result) is walked for
//!   streams built here ([`InputStream::from_reader`], [`from_stream`],
//!   [`from_bytes`], [`OutputStream::to_writer`], or a received stream already
//!   read from or written to). Each gets a pipe from
//!   `engine::channels::create`, its ref takes the stream's place, and a task
//!   pumps the data: the source into the pipe, or the pipe into the sink. A
//!   received stream not yet used is sent as its own ref (forwarded): no byte
//!   passes through this worker.
//! - What it receives deserializes: a `read` ref is an [`InputStream`], a
//!   `write` ref an [`OutputStream`], in a handler's input and in a call's
//!   result alike. A captured `input` / `output` of an observed call arrives
//!   as a [`StreamInfo`].
//!
//! In-process (a call this worker dispatches to its own function) a stream
//! passes as is: the handler reads the caller's source, or writes into the
//! caller's sink, with no pipe.
//!
//! [`from_stream`]: InputStream::from_stream
//! [`from_bytes`]: InputStream::from_bytes

use std::{
    collections::{HashMap, HashSet},
    fmt, io,
    pin::Pin,
    sync::{
        Arc, LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt, future::BoxFuture};
use schemars::{JsonSchema, r#gen::SchemaGenerator, schema::Schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::{oneshot, watch};

use crate::{
    channels::{
        ChannelDirection, ChannelItem, Frame, FrameStream, MAX_FRAME_SIZE, StreamChannelRef,
        WriterEnd, abort_detached, channel_url, is_channel_ref, read_frames,
    },
    context,
    error::Error,
    iii::{IIIClient, MutexExt},
    protocol::{Activation, CallRequest},
};

// =============================================================================
// Binding: which engine a ref-backed stream dials
// =============================================================================

/// Where a ref-backed stream opens its socket: the engine address and the
/// handshake headers of a client.
#[derive(Clone)]
struct Endpoint {
    address: String,
    headers: HashMap<String, String>,
}

impl Endpoint {
    fn of(iii: &IIIClient) -> Self {
        let (address, headers) = iii.stream_endpoint();
        Self { address, headers }
    }
}

const UNBOUND: &str = "the stream is not bound to a client: it was received outside a handler \
     while no single IIIClient is alive; call bind(&iii) before reading or writing it";

/// The endpoint of `bound`, or else of the handler's client, or else of the
/// only client alive, which it is then bound to.
fn resolve_endpoint(bound: &Mutex<Option<Endpoint>>) -> Result<Endpoint, Error> {
    if let Some(endpoint) = bound.lock_or_recover().clone() {
        return Ok(endpoint);
    }
    let client = context::current_client()
        .or_else(IIIClient::only_live)
        .ok_or_else(|| Error::Runtime(UNBOUND.to_string()))?;
    let endpoint = Endpoint::of(&client);
    *bound.lock_or_recover() = Some(endpoint.clone());
    Ok(endpoint)
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// The reader id of a stream sent on: every later read fails.
const SENT: u64 = u64::MAX;

// =============================================================================
// Placeholders: locally built streams until a call or result carries them
// =============================================================================

/// The `channel_id` prefix of a placeholder ref.
const PLACEHOLDER_PREFIX: &str = "local:";

/// A placeholder not claimed within this long is dropped.
const PLACEHOLDER_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
enum Local {
    Input(Arc<InputInner>),
    Output(Arc<OutputInner>),
}

/// The streams serialized as placeholders, by token. A call or result that
/// carries one claims it (a pipe replaces it); a handler or caller in this
/// process that deserializes one takes the stream itself.
static PLACEHOLDERS: LazyLock<Mutex<HashMap<String, (Instant, Local)>>> =
    LazyLock::new(Default::default);

/// Set once a placeholder was made: until then no value needs a walk.
static PLACEHOLDERS_USED: AtomicBool = AtomicBool::new(false);

fn placeholders_used() -> bool {
    PLACEHOLDERS_USED.load(Ordering::Acquire)
}

fn register(token: &str, entry: Local) {
    let now = Instant::now();
    let expired: Vec<(Instant, Local)> = {
        let mut map = PLACEHOLDERS.lock_or_recover();
        let stale: Vec<String> = map
            .iter()
            .filter(|(_, (at, _))| now.duration_since(*at) >= PLACEHOLDER_TTL)
            .map(|(token, _)| token.clone())
            .collect();
        let expired = stale.iter().filter_map(|t| map.remove(t)).collect();
        map.insert(token.to_string(), (now, entry));
        expired
    };
    PLACEHOLDERS_USED.store(true, Ordering::Release);
    // Dropped outside the lock: a stream's drop may abort its socket.
    drop(expired);
}

fn claim(token: &str) -> Option<Local> {
    let claimed = PLACEHOLDERS.lock_or_recover().remove(token);
    claimed.map(|(_, entry)| entry)
}

fn peek(token: &str) -> Option<Local> {
    PLACEHOLDERS
        .lock_or_recover()
        .get(token)
        .map(|(_, entry)| entry.clone())
}

/// The token and direction of a placeholder ref.
fn placeholder_of(value: &Value) -> Option<(&str, ChannelDirection)> {
    let obj = value.as_object()?;
    let token = obj
        .get("channel_id")?
        .as_str()?
        .strip_prefix(PLACEHOLDER_PREFIX)?;
    if !obj.get("access_key")?.as_str()?.is_empty() {
        return None;
    }
    let direction = match obj.get("direction")?.as_str()? {
        "read" => ChannelDirection::Read,
        "write" => ChannelDirection::Write,
        _ => return None,
    };
    Some((token, direction))
}

fn serialize_placeholder<S: Serializer>(
    serializer: S,
    token: &str,
    direction: ChannelDirection,
) -> Result<S::Ok, S::Error> {
    StreamChannelRef {
        channel_id: format!("{PLACEHOLDER_PREFIX}{token}"),
        access_key: String::new(),
        direction,
    }
    .serialize(serializer)
}

fn new_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// What a stream serializes to.
enum Wire {
    /// Its own ref: forwarded.
    Ref(StreamChannelRef),
    /// A placeholder the outbound walk replaces.
    Placeholder(String),
}

/// The ref fields of a stream value; extra keys are tolerated.
#[derive(Deserialize)]
struct WireRef {
    channel_id: String,
    access_key: String,
    direction: ChannelDirection,
}

impl WireRef {
    fn placeholder_token(&self) -> Option<&str> {
        self.channel_id
            .strip_prefix(PLACEHOLDER_PREFIX)
            .filter(|_| self.access_key.is_empty())
    }

    fn into_ref(self) -> StreamChannelRef {
        StreamChannelRef {
            channel_id: self.channel_id,
            access_key: self.access_key,
            direction: self.direction,
        }
    }
}

fn gone(channel_id: &str) -> String {
    format!(
        "stream {channel_id} is no longer available: it was already received or sent, or not \
         claimed within 5 minutes"
    )
}

// =============================================================================
// InputStream
// =============================================================================

#[derive(Clone)]
enum ReadStatus {
    /// Nothing read yet.
    Unread,
    /// A reader started.
    Reading,
    /// The writer closed it and everything was read.
    Ended,
    /// It ended with this error (an abort, a failed dial).
    Failed(Error),
    /// Cancelled by its reader, or by the handler guard.
    Cancelled,
    /// Its own ref was sent on: the other side reads it.
    Forwarded,
    /// A pump took the rest of it, to send it on through a pipe.
    Piped,
}

struct ReadState {
    status: ReadStatus,
    /// The frames still to read: a local source from the start, the socket
    /// of a ref-backed stream from its first read.
    frames: Option<FrameStream>,
    /// The reader that owns the stream (one reader, R6); 0 before any.
    reader: u64,
    /// The rest of a binary frame an `AsyncRead` buffer could not take.
    leftover: Option<Bytes>,
    /// The waker of a read in flight, woken by a cancel.
    waker: Option<Waker>,
    /// The failure was already returned to the reader.
    failure_seen: bool,
    /// The invocation guard that settles it (0 for none, `SENT` while a
    /// placeholder carries it to an in-process receiver).
    holder: u64,
}

pub(crate) struct InputInner {
    /// The pipe end it reads, for a stream received or made with a pipe.
    channel: Option<StreamChannelRef>,
    bound: Mutex<Option<Endpoint>>,
    state: Mutex<ReadState>,
    token: OnceLock<String>,
}

impl InputInner {
    fn new(channel: Option<StreamChannelRef>, frames: Option<FrameStream>) -> Self {
        Self {
            channel,
            bound: Mutex::new(None),
            state: Mutex::new(ReadState {
                status: ReadStatus::Unread,
                frames,
                reader: 0,
                leftover: None,
                waker: None,
                failure_seen: false,
                holder: 0,
            }),
            token: OnceLock::new(),
        }
    }

    fn token(&self) -> &str {
        self.token.get_or_init(new_token)
    }

    fn open_frames(&self) -> Result<FrameStream, Error> {
        match &self.channel {
            Some(channel) => {
                let endpoint = resolve_endpoint(&self.bound)?;
                Ok(read_frames(
                    channel_url(&endpoint.address, channel),
                    endpoint.headers,
                ))
            }
            None => Ok(Box::pin(futures_util::stream::empty())),
        }
    }

    /// The next frame for `reader`. A second reader gets an error (R6).
    fn poll_frame(&self, cx: &mut Context<'_>, reader: u64) -> Poll<Option<Result<Frame, Error>>> {
        let mut st = self.state.lock_or_recover();
        if st.reader == 0 {
            st.reader = reader;
        } else if st.reader != reader {
            return Poll::Ready(Some(Err(already_read())));
        }
        match &st.status {
            ReadStatus::Unread | ReadStatus::Reading => {}
            ReadStatus::Ended | ReadStatus::Cancelled => return Poll::Ready(None),
            ReadStatus::Forwarded | ReadStatus::Piped => {
                return Poll::Ready(Some(Err(already_read())));
            }
            ReadStatus::Failed(error) => {
                if st.failure_seen {
                    return Poll::Ready(None);
                }
                let error = error.clone();
                st.failure_seen = true;
                return Poll::Ready(Some(Err(error)));
            }
        }
        st.status = ReadStatus::Reading;
        if let Some(rest) = st.leftover.take() {
            return Poll::Ready(Some(Ok(Frame::Binary(rest))));
        }
        if st.frames.is_none() {
            match self.open_frames() {
                Ok(frames) => st.frames = Some(frames),
                Err(error) => {
                    st.status = ReadStatus::Failed(error.clone());
                    st.failure_seen = true;
                    return Poll::Ready(Some(Err(error)));
                }
            }
        }
        let frames = st.frames.as_mut().expect("opened above");
        match frames.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(Ok(frame))),
            Poll::Ready(Some(Err(error))) => {
                st.frames = None;
                st.status = ReadStatus::Failed(error.clone());
                st.failure_seen = true;
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                st.frames = None;
                st.status = ReadStatus::Ended;
                Poll::Ready(None)
            }
            Poll::Pending => {
                st.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    fn put_back(&self, rest: Bytes) {
        self.state.lock_or_recover().leftover = Some(rest);
    }

    /// Stops reading: the socket, if open, is dropped, and a read in flight
    /// ends. A stream never read opens no socket.
    fn cancel(&self) {
        let (frames, waker) = {
            let mut st = self.state.lock_or_recover();
            if !matches!(st.status, ReadStatus::Unread | ReadStatus::Reading) {
                return;
            }
            st.status = ReadStatus::Cancelled;
            st.leftover = None;
            (st.frames.take(), st.waker.take())
        };
        drop(frames);
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// R3: cancels a stream the handler started reading and did not finish.
    /// Sent in-process after a read began: its receiver's first reader
    /// reads the rest.
    fn hand_over(&self) {
        let mut st = self.state.lock_or_recover();
        if matches!(st.status, ReadStatus::Reading) {
            st.reader = 0;
        }
    }

    /// R3, by the guard `holder`: cancelled if it started reading it. One
    /// it sent in-process is its receiver's to settle.
    fn settle(&self, holder: u64) {
        let reading = {
            let st = self.state.lock_or_recover();
            matches!(st.status, ReadStatus::Reading) && st.holder == holder
        };
        if reading {
            self.cancel();
        }
    }

    fn wire(&self) -> Wire {
        let mut st = self.state.lock_or_recover();
        match (&self.channel, &st.status) {
            (Some(channel), ReadStatus::Unread) => {
                st.status = ReadStatus::Forwarded;
                st.reader = SENT;
                Wire::Ref(channel.clone())
            }
            // Anything else, a stream already sent included, is the walk's
            // to pump or to refuse, or an in-process receiver's.
            _ => {
                st.holder = SENT;
                Wire::Placeholder(self.token().to_string())
            }
        }
    }

    /// The rest of the stream, for a pump that sends it on (R5).
    fn take_source(&self) -> Result<FrameStream, Error> {
        let (source, waker) = {
            let mut st = self.state.lock_or_recover();
            let source = match &st.status {
                ReadStatus::Unread | ReadStatus::Reading => {
                    let frames = match st.frames.take() {
                        Some(frames) => frames,
                        None => self.open_frames()?,
                    };
                    match st.leftover.take() {
                        Some(rest) => Box::pin(
                            futures_util::stream::once(async move { Ok(Frame::Binary(rest)) })
                                .chain(frames),
                        ) as FrameStream,
                        None => frames,
                    }
                }
                ReadStatus::Ended => return Err(read_to_the_end()),
                ReadStatus::Failed(error) => return Err(error.clone()),
                ReadStatus::Cancelled => {
                    return Err(Error::Runtime("the stream was cancelled".into()));
                }
                ReadStatus::Forwarded | ReadStatus::Piped => return Err(already_sent()),
            };
            st.status = ReadStatus::Piped;
            st.reader = SENT;
            (source, st.waker.take())
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        Ok(source)
    }

    fn read_to_the_end(&self) -> bool {
        matches!(self.state.lock_or_recover().status, ReadStatus::Ended)
    }

    fn describe(&self) -> &'static str {
        match self.state.lock_or_recover().status {
            ReadStatus::Unread => "unread",
            ReadStatus::Reading => "reading",
            ReadStatus::Ended => "ended",
            ReadStatus::Failed(_) => "failed",
            ReadStatus::Cancelled => "cancelled",
            ReadStatus::Forwarded => "forwarded",
            ReadStatus::Piped => "piped",
        }
    }
}

fn read_to_the_end() -> Error {
    Error::StreamInUse(
        "the stream was already read to the end: there is nothing left to send".into(),
    )
}

fn already_read() -> Error {
    Error::StreamInUse(
        "the stream already has a reader: an InputStream is read once, by one reader (a clone \
         is another reader), and not after it was sent on"
            .into(),
    )
}

fn already_sent() -> Error {
    Error::StreamInUse("the stream was already sent: a stream can be sent once".into())
}

fn sent_twice() -> Error {
    Error::StreamInUse("a stream can be sent once: the same stream appears twice".into())
}

/// The read side of a stream: bytes (and messages) a function receives.
///
/// An `InputStream` arrives as a field of a handler's input or of a call's
/// result (a `read` ref deserializes to it), or is built to be sent:
/// [`from_reader`](Self::from_reader), [`from_stream`](Self::from_stream),
/// [`from_bytes`](Self::from_bytes). Put it in a call payload or a handler
/// result and the SDK streams it to the other side.
///
/// Read it once, one way: as a `Stream` of byte chunks, an `AsyncRead`,
/// [`read_all`](Self::read_all), [`text`](Self::text),
/// [`lines`](Self::lines), [`messages`](Self::messages) (text frames, JSON
/// decoded) or [`frames`](Self::frames) (both, in order). Byte reads skip
/// text frames. A second reader gets [`Error::StreamInUse`]. The socket opens
/// on the first read.
///
/// An abort (its writer failed or called
/// [`OutputStream::abort`](crate::OutputStream::abort)) ends a read with
/// [`Error::StreamAborted`], after the data sent before it.
///
/// In a handler, a stream the handler started reading and did not finish is
/// cancelled when the handler returns; one never read is left alone. One sent
/// on unread (in a call payload or the result) is forwarded: its ref goes, no
/// byte passes through this worker. One sent on after reading is pumped from
/// where it is. A stream is sent once: sending it twice, or after it was read
/// to the end, fails with [`Error::StreamInUse`].
///
/// A received stream binds to the client of the handler it was received in,
/// or else to the only [`IIIClient`] alive; with several, call
/// [`bind`](Self::bind). A clone is a handle to the same stream, and another
/// reader: only one of them can read it.
pub struct InputStream {
    inner: Arc<InputInner>,
    /// This handle's reader id, for its `Stream` and `AsyncRead` reads.
    reader: u64,
}

impl Clone for InputStream {
    fn clone(&self) -> Self {
        Self::from_arc(self.inner.clone())
    }
}

impl InputStream {
    fn from_arc(inner: Arc<InputInner>) -> Self {
        Self {
            inner,
            reader: next_id(),
        }
    }

    fn from_inner(inner: InputInner) -> Self {
        Self::from_arc(Arc::new(inner))
    }

    pub(crate) fn from_ref(channel: StreamChannelRef, iii: Option<&IIIClient>) -> Self {
        let stream = Self::from_inner(InputInner::new(Some(channel), None));
        if let Some(iii) = iii {
            stream.bind(iii);
        }
        stream
    }

    fn from_frames(frames: FrameStream) -> Self {
        Self::from_inner(InputInner::new(None, Some(frames)))
    }

    /// A stream of what `reader` yields, read in chunks of up to 64 KiB. A
    /// read error aborts the stream with its message.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{CallRequest, IIIClient, InputStream};
    /// # use serde_json::json;
    /// # async fn demo(iii: &IIIClient) -> Result<(), Box<dyn std::error::Error>> {
    /// let result = iii
    ///     .call(CallRequest::new("files::process", json!({
    ///         "filename": "report.csv",
    ///         "body": InputStream::from_reader(tokio::fs::File::open("report.csv").await?),
    ///     })))
    ///     .await?;
    /// # Ok(()) }
    /// ```
    pub fn from_reader<R>(reader: R) -> Self
    where
        R: AsyncRead + Send + 'static,
    {
        let frames = futures_util::stream::unfold(Some(Box::pin(reader)), |reader| async move {
            let mut reader = reader?;
            let mut buf = vec![0u8; MAX_FRAME_SIZE];
            match reader.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok(Frame::Binary(Bytes::from(buf))), Some(reader)))
                }
                Err(error) => Some((
                    Err(Error::StreamAborted {
                        reason: error.to_string(),
                    }),
                    None,
                )),
            }
        });
        Self::from_frames(Box::pin(frames))
    }

    /// A stream of the chunks `stream` yields. An `Err` aborts the stream
    /// with its message: the reader gets [`Error::StreamAborted`], not a
    /// short stream that looks complete.
    pub fn from_stream<S, B, E>(stream: S) -> Self
    where
        S: Stream<Item = Result<B, E>> + Send + 'static,
        B: Into<Bytes>,
        E: fmt::Display,
    {
        let frames = stream.map(|item| match item {
            Ok(chunk) => Ok(Frame::Binary(chunk.into())),
            Err(error) => Err(Error::StreamAborted {
                reason: error.to_string(),
            }),
        });
        Self::from_frames(Box::pin(frames))
    }

    /// A stream of `bytes`.
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        let bytes = bytes.into();
        let frames: FrameStream = if bytes.is_empty() {
            Box::pin(futures_util::stream::empty())
        } else {
            Box::pin(futures_util::stream::once(async move {
                Ok(Frame::Binary(bytes))
            }))
        };
        Self::from_frames(frames)
    }

    /// Binds a received stream to `iii`: its socket opens on that client's
    /// engine, with its handshake headers. Needed only outside a handler
    /// while several clients are alive.
    pub fn bind(&self, iii: &IIIClient) -> &Self {
        *self.inner.bound.lock_or_recover() = Some(Endpoint::of(iii));
        self
    }

    fn frames_for(&self, reader: u64) -> Frames {
        Frames {
            inner: self.inner.clone(),
            reader,
            done: false,
        }
    }

    fn reader(&self) -> Frames {
        self.frames_for(next_id())
    }

    /// Reads the whole stream (its binary frames).
    pub async fn read_all(&self) -> Result<Vec<u8>, Error> {
        let mut frames = self.reader();
        let mut buffer = Vec::new();
        while let Some(frame) = frames.next().await {
            if let Frame::Binary(data) = frame? {
                buffer.extend_from_slice(&data);
            }
        }
        Ok(buffer)
    }

    /// Reads the whole stream as UTF-8 text; invalid sequences become
    /// U+FFFD.
    pub async fn text(&self) -> Result<String, Error> {
        let bytes = self.read_all().await?;
        Ok(match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
        })
    }

    /// The stream's lines, without their `\n` (or `\r\n`); the last one may
    /// have no newline. Invalid UTF-8 becomes U+FFFD.
    pub fn lines(&self) -> impl Stream<Item = Result<String, Error>> + Send + 'static {
        fn line(mut bytes: Vec<u8>) -> String {
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
            }
        }
        futures_util::stream::unfold(
            (self.reader(), Vec::<u8>::new(), false),
            |(mut frames, mut buf, mut ended)| async move {
                loop {
                    if let Some(newline) = buf.iter().position(|b| *b == b'\n') {
                        let mut taken: Vec<u8> = buf.drain(..=newline).collect();
                        taken.pop();
                        return Some((Ok(line(taken)), (frames, buf, ended)));
                    }
                    if ended {
                        if buf.is_empty() {
                            return None;
                        }
                        let last = String::from_utf8_lossy(&buf).into_owned();
                        return Some((Ok(last), (frames, Vec::new(), true)));
                    }
                    match frames.next().await {
                        Some(Ok(Frame::Binary(data))) => buf.extend_from_slice(&data),
                        Some(Ok(Frame::Text(_))) => {}
                        Some(Err(error)) => return Some((Err(error), (frames, Vec::new(), true))),
                        None => ended = true,
                    }
                }
            },
        )
    }

    /// The stream's messages: its text frames, each decoded as JSON into
    /// `T`. A frame that is not JSON is decoded from the raw string (so a
    /// `T` of `String` or `Value` takes it). Binary frames are skipped.
    pub fn messages<T>(&self) -> impl Stream<Item = Result<T, Error>> + Send + 'static
    where
        T: DeserializeOwned + Send + 'static,
    {
        self.reader().filter_map(|frame| async move {
            match frame {
                Ok(Frame::Binary(_)) => None,
                Ok(Frame::Text(text)) => Some(decode_message(text)),
                Err(error) => Some(Err(error)),
            }
        })
    }

    /// Every frame, in order: text frames (messages) and binary frames
    /// (bytes).
    pub fn frames(&self) -> impl Stream<Item = Result<ChannelItem, Error>> + Send + 'static {
        self.reader().map(|frame| frame.map(ChannelItem::from))
    }

    /// Stops reading. The socket, if open, closes, and the engine then
    /// closes the writer, as it does when a reader leaves. Later reads end.
    pub async fn cancel(&self) {
        self.inner.cancel();
    }
}

fn decode_message<T: DeserializeOwned>(text: String) -> Result<T, Error> {
    match serde_json::from_str::<T>(&text) {
        Ok(value) => Ok(value),
        Err(error) if error.is_syntax() || error.is_eof() => {
            T::deserialize(Value::String(text)).map_err(|_| Error::Serde(error.to_string()))
        }
        Err(error) => Err(Error::Serde(error.to_string())),
    }
}

/// The frames of one reader of a stream; ends after an error.
struct Frames {
    inner: Arc<InputInner>,
    reader: u64,
    done: bool,
}

impl Stream for Frames {
    type Item = Result<Frame, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        let polled = self.inner.poll_frame(cx, self.reader);
        if matches!(polled, Poll::Ready(None) | Poll::Ready(Some(Err(_)))) {
            self.done = true;
        }
        polled
    }
}

/// The byte chunks of the stream (its binary frames).
impl Stream for InputStream {
    type Item = Result<Bytes, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match self.inner.poll_frame(cx, self.reader) {
                Poll::Ready(Some(Ok(Frame::Binary(data)))) => return Poll::Ready(Some(Ok(data))),
                Poll::Ready(Some(Ok(Frame::Text(_)))) => continue,
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Some(Err(error))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The stream's bytes (its binary frames); an abort is an
/// [`io::Error`] wrapping [`Error::StreamAborted`].
impl AsyncRead for InputStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buf.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            match self.inner.poll_frame(cx, self.reader) {
                Poll::Ready(Some(Ok(Frame::Binary(data)))) => {
                    if data.is_empty() {
                        continue;
                    }
                    let n = data.len().min(buf.remaining());
                    buf.put_slice(&data[..n]);
                    if n < data.len() {
                        self.inner.put_back(data.slice(n..));
                    }
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Some(Ok(Frame::Text(_)))) => continue,
                Poll::Ready(Some(Err(error))) => return Poll::Ready(Err(io::Error::other(error))),
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl fmt::Debug for InputStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputStream")
            .field(
                "channel_id",
                &self.inner.channel.as_ref().map(|c| c.channel_id.as_str()),
            )
            .field("state", &self.inner.describe())
            .finish()
    }
}

impl Serialize for InputStream {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.inner.wire() {
            Wire::Ref(channel) => channel.serialize(serializer),
            Wire::Placeholder(token) => {
                register(&token, Local::Input(self.inner.clone()));
                serialize_placeholder(serializer, &token, ChannelDirection::Read)
            }
        }
    }
}

impl<'de> Deserialize<'de> for InputStream {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let wire = WireRef::deserialize(deserializer)?;
        if wire.direction != ChannelDirection::Read {
            return Err(D::Error::custom(
                "expected a stream to read (direction \"read\"): a \"write\" ref is an OutputStream",
            ));
        }
        let stream = match wire.placeholder_token() {
            Some(token) => match claim(token) {
                Some(Local::Input(inner)) => {
                    inner.hand_over();
                    InputStream::from_arc(inner)
                }
                Some(other) => {
                    register(token, other);
                    return Err(D::Error::custom(gone(&wire.channel_id)));
                }
                None => return Err(D::Error::custom(gone(&wire.channel_id))),
            },
            None => {
                // A ref the invocation received: the handler's stream, typed
                // or read from its raw input.
                let key = (wire.channel_id.clone(), ChannelDirection::Read);
                if let Some(Tracked::Input(inner)) = received_stream(&key) {
                    return Ok(InputStream::from_arc(inner));
                }
                InputStream::from_inner(InputInner::new(Some(wire.into_ref()), None))
            }
        };
        if let Some(client) = context::current_client() {
            let mut bound = stream.inner.bound.lock_or_recover();
            if bound.is_none() {
                *bound = Some(Endpoint::of(&client));
            }
        }
        if let Some(guard) = context::current_streams() {
            guard.track(Tracked::Input(stream.inner.clone()));
        }
        Ok(stream)
    }
}

impl JsonSchema for InputStream {
    fn schema_name() -> String {
        "InputStream".to_string()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        stream_schema(
            "InputStream",
            ChannelDirection::Read,
            "A stream the function reads: the ref of the read end of an engine pipe \
             ({channel_id, access_key, direction: \"read\"}). The SDK opens it on first read.",
        )
    }
}

fn stream_schema(title: &str, direction: ChannelDirection, description: &str) -> Schema {
    use schemars::schema::{InstanceType, Metadata, ObjectValidation, SchemaObject};
    let string = |value: Option<&str>| {
        Schema::Object(SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            const_value: value.map(|v| Value::String(v.to_string())),
            ..Default::default()
        })
    };
    let mut object = ObjectValidation::default();
    object
        .properties
        .insert("channel_id".to_string(), string(None));
    object
        .properties
        .insert("access_key".to_string(), string(None));
    object
        .properties
        .insert("direction".to_string(), string(Some(direction.as_str())));
    for key in ["channel_id", "access_key", "direction"] {
        object.required.insert(key.to_string());
    }
    let mut schema = SchemaObject {
        metadata: Some(Box::new(Metadata {
            title: Some(title.to_string()),
            description: Some(description.to_string()),
            ..Default::default()
        })),
        instance_type: Some(InstanceType::Object.into()),
        object: Some(Box::new(object)),
        ..Default::default()
    };
    schema
        .extensions
        .insert("x-iii-stream".to_string(), json!(direction.as_str()));
    Schema::Object(schema)
}

// =============================================================================
// OutputStream
// =============================================================================

/// How an output stream ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Outcome {
    Closed,
    Aborted(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Use {
    /// Not written, sent to, or bridged yet.
    Unused,
    /// Written, sent to, or bridged.
    Used,
    /// Its own ref was sent on: the other side writes it.
    Forwarded,
    /// A pump writes into it what another function writes into a pipe.
    Piped,
}

struct OutState {
    usage: Use,
    outcome: Option<Outcome>,
}

enum OutIo {
    /// The write end of a pipe: dialed on first use.
    Ws(Option<WriterEnd>),
    /// A local sink, until closed or aborted.
    Local(Option<Pin<Box<dyn AsyncWrite + Send>>>),
    /// The in-process receiver's handle on a stream of this process: what
    /// it does goes to that stream, and it ends on its own (dropped unfinished,
    /// it aborts that stream, as a dropped pipe writer would).
    Bridge(Arc<OutputInner>),
}

pub(crate) struct OutputInner {
    channel: Option<StreamChannelRef>,
    bound: Mutex<Option<Endpoint>>,
    io: tokio::sync::Mutex<OutIo>,
    state: Mutex<OutState>,
    kept_open: AtomicBool,
    /// Its in-process receiver dropped its handle without using it.
    released: AtomicBool,
    done: watch::Sender<Option<Outcome>>,
    token: OnceLock<String>,
}

fn closed() -> Error {
    Error::StreamClosed("the stream is closed: it was closed or aborted".into())
}

fn io_error(error: io::Error) -> Error {
    Error::Runtime(error.to_string())
}

impl OutputInner {
    fn new(channel: Option<StreamChannelRef>, io: OutIo) -> Self {
        Self {
            channel,
            bound: Mutex::new(None),
            io: tokio::sync::Mutex::new(io),
            state: Mutex::new(OutState {
                usage: Use::Unused,
                outcome: None,
            }),
            kept_open: AtomicBool::new(false),
            released: AtomicBool::new(false),
            done: watch::Sender::new(None),
            token: OnceLock::new(),
        }
    }

    /// The in-process receiver's handle on `target`.
    fn bridge(target: Arc<OutputInner>) -> Self {
        Self::new(None, OutIo::Bridge(target))
    }

    fn token(&self) -> &str {
        self.token.get_or_init(new_token)
    }

    /// Marks a write: fails on a stream that ended or was sent on.
    fn begin(&self) -> Result<(), Error> {
        let mut st = self.state.lock_or_recover();
        if st.outcome.is_some() {
            return Err(closed());
        }
        match st.usage {
            Use::Forwarded => Err(Error::StreamInUse(
                "the stream was sent to another function, which writes it".into(),
            )),
            Use::Unused => {
                st.usage = Use::Used;
                Ok(())
            }
            Use::Used | Use::Piped => Ok(()),
        }
    }

    fn writer_end<'a>(&self, slot: &'a mut Option<WriterEnd>) -> Result<&'a mut WriterEnd, Error> {
        if slot.is_none() {
            let channel = self.channel.as_ref().ok_or_else(closed)?;
            let endpoint = resolve_endpoint(&self.bound)?;
            *slot = Some(WriterEnd::new(
                channel_url(&endpoint.address, channel),
                endpoint.headers,
            ));
        }
        Ok(slot.as_mut().expect("set above"))
    }

    async fn write(&self, data: &[u8]) -> Result<(), Error> {
        let mut io = self.io.lock().await;
        self.begin()?;
        match &mut *io {
            OutIo::Ws(end) => self.writer_end(end)?.write(data).await,
            OutIo::Local(Some(sink)) => sink.write_all(data).await.map_err(io_error),
            OutIo::Local(None) => Err(closed()),
            OutIo::Bridge(target) => Box::pin(target.write(data)).await,
        }
    }

    async fn send_text(&self, text: &str) -> Result<(), Error> {
        let mut io = self.io.lock().await;
        self.begin()?;
        match &mut *io {
            OutIo::Ws(end) => self.writer_end(end)?.send_text(text).await,
            // A byte sink takes bytes only: messages are not written to it.
            OutIo::Local(_) => Ok(()),
            OutIo::Bridge(target) => Box::pin(target.send_text(text)).await,
        }
    }

    fn outcome(&self) -> Option<Outcome> {
        self.state.lock_or_recover().outcome.clone()
    }

    fn finish(&self, outcome: Outcome) {
        self.state.lock_or_recover().outcome = Some(outcome.clone());
        self.done.send_replace(Some(outcome));
    }

    fn forwarded(&self) -> bool {
        self.state.lock_or_recover().usage == Use::Forwarded
    }

    async fn close(&self) -> Result<(), Error> {
        let mut io = self.io.lock().await;
        if self.outcome().is_some() || self.forwarded() {
            return Ok(());
        }
        let result = match &mut *io {
            OutIo::Ws(end) => match self.writer_end(end) {
                Ok(end) => end.close().await,
                Err(error) => Err(error),
            },
            OutIo::Local(sink) => match sink.take() {
                Some(mut sink) => sink.shutdown().await.map_err(io_error),
                None => Ok(()),
            },
            OutIo::Bridge(target) => Box::pin(target.close()).await,
        };
        self.finish(Outcome::Closed);
        result
    }

    async fn abort(&self, reason: &str) -> Result<(), Error> {
        let mut io = self.io.lock().await;
        if self.outcome().is_some() || self.forwarded() {
            return Ok(());
        }
        let result = match &mut *io {
            OutIo::Ws(end) => match self.writer_end(end) {
                Ok(end) => end.abort(reason).await,
                Err(error) => Err(error),
            },
            OutIo::Local(sink) => {
                sink.take();
                Ok(())
            }
            OutIo::Bridge(target) => Box::pin(target.abort(reason)).await,
        };
        self.finish(Outcome::Aborted(reason.to_string()));
        result
    }

    /// Aborts it from code that cannot wait (a `Drop`).
    fn abort_soon(self: &Arc<Self>, reason: &'static str) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let inner = self.clone();
            runtime.spawn(async move {
                let _ = inner.abort(reason).await;
            });
            return;
        }
        let mut st = self.state.lock_or_recover();
        if st.outcome.is_none() {
            st.outcome = Some(Outcome::Aborted(reason.into()));
            drop(st);
            self.done
                .send_replace(Some(Outcome::Aborted(reason.into())));
        }
    }

    /// Its in-process receiver dropped its handle without using it.
    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.done.send_modify(|_| {});
    }

    /// Resolves once the stream closed or aborted.
    #[cfg(test)]
    async fn finished(&self) -> Outcome {
        loop {
            if let Some(outcome) = self.settled().await {
                return outcome;
            }
        }
    }

    /// Resolves once the stream closed or aborted (`Some`), or its
    /// in-process receiver dropped it unused (`None`).
    async fn settled(&self) -> Option<Outcome> {
        let mut done = self.done.subscribe();
        let ended = done
            .wait_for(|outcome| outcome.is_some() || self.released.load(Ordering::SeqCst))
            .await;
        match ended {
            Ok(outcome) => outcome.clone(),
            Err(_) => Some(Outcome::Aborted("dropped without close".into())),
        }
    }

    fn wire(&self) -> Wire {
        let mut st = self.state.lock_or_recover();
        match (&self.channel, st.usage, &st.outcome) {
            (Some(channel), Use::Unused, None) => {
                st.usage = Use::Forwarded;
                Wire::Ref(channel.clone())
            }
            // Anything else, a stream already sent included, is the walk's
            // to pump or to refuse.
            _ => Wire::Placeholder(self.token().to_string()),
        }
    }

    /// Takes the stream as the sink of a pump.
    fn take_sink(&self) -> Result<(), Error> {
        let mut st = self.state.lock_or_recover();
        if st.outcome.is_some() {
            return Err(closed());
        }
        if st.usage == Use::Forwarded {
            return Err(already_sent());
        }
        st.usage = Use::Piped;
        Ok(())
    }

    /// Its ref was passed on as raw JSON: the other side writes it now.
    fn transfer(&self) {
        let mut st = self.state.lock_or_recover();
        if st.usage == Use::Unused && st.outcome.is_none() {
            st.usage = Use::Forwarded;
        }
    }

    /// R2: what the handler guard does with the stream once the handler
    /// returned `result`; `activation` when the invocation was an observer
    /// activation.
    fn settle(
        self: &Arc<Self>,
        result: &Result<Value, Error>,
        activation: bool,
    ) -> Option<BoxFuture<'static, ()>> {
        if self.kept_open.load(Ordering::SeqCst) {
            return None;
        }
        let usage = {
            let st = self.state.lock_or_recover();
            if st.outcome.is_some() {
                return None;
            }
            st.usage
        };
        let abort = match (usage, result) {
            (Use::Used, Ok(_)) => None,
            (Use::Used, Err(error)) => Some(error.to_string()),
            // A plain call's untouched stream ends, so the caller's sink does.
            (Use::Unused, Ok(_)) if !activation => None,
            // Through an observer the event source decides: for the HTTP
            // worker, an untouched response with a value is a buffered
            // response, and closing it first would win its race.
            (Use::Unused, Ok(Value::Null)) => None,
            // Untouched on failure: the caller learns from the error.
            _ => return None,
        };
        let inner = self.clone();
        Some(Box::pin(async move {
            let outcome = match abort {
                None => inner.close().await,
                Some(reason) => inner.abort(&reason).await,
            };
            if let Err(error) = outcome {
                tracing::debug!(error = %error, "stream not ended after its handler returned");
            }
        }))
    }

    fn describe(&self) -> String {
        let st = self.state.lock_or_recover();
        match &st.outcome {
            Some(Outcome::Closed) => "closed".to_string(),
            Some(Outcome::Aborted(reason)) => format!("aborted: {reason}"),
            None => format!("{:?}", st.usage).to_lowercase(),
        }
    }
}

impl Drop for OutputInner {
    fn drop(&mut self) {
        let st = self.state.get_mut().unwrap_or_else(|e| e.into_inner());
        if st.outcome.is_some() {
            return;
        }
        let used = matches!(st.usage, Use::Used | Use::Piped);
        if !used {
            // An in-process receiver's handle dropped unused: the call that
            // passed the stream decides what that means.
            if st.usage == Use::Unused
                && let OutIo::Bridge(target) = self.io.get_mut()
            {
                target.release();
            }
            return;
        }
        // Used and never ended: the reader must not take what it got for the
        // whole stream.
        const REASON: &str = "dropped without close";
        st.outcome = Some(Outcome::Aborted(REASON.into()));
        self.done
            .send_replace(Some(Outcome::Aborted(REASON.into())));
        match self.io.get_mut() {
            OutIo::Ws(Some(end)) => {
                if let Some(socket) = end.take_socket() {
                    abort_detached(socket, REASON);
                }
            }
            OutIo::Local(sink) => drop(sink.take()),
            OutIo::Bridge(target) => target.abort_soon(REASON),
            OutIo::Ws(None) => {}
        }
    }
}

type PendingWrite = Option<BoxFuture<'static, io::Result<usize>>>;
type PendingClose = Option<BoxFuture<'static, io::Result<()>>>;

/// The write side of a stream: where a function writes bytes and messages.
///
/// An `OutputStream` arrives as a field of a handler's input or of a call's
/// result (a `write` ref deserializes to it), or is built around a local sink
/// with [`to_writer`](Self::to_writer): put it in a call payload and the
/// function writes into the sink; the call resolves once the sink got the
/// whole stream (and fails with [`Error::StreamAborted`] if the writer
/// aborted it), within the call's timeout. A call whose result holds a stream
/// to write (the function returned a sink) resolves without waiting: the
/// caller writes it.
///
/// [`write`](Self::write) sends bytes (in binary frames of up to 64 KiB),
/// [`send`](Self::send) a message (a JSON text frame), and each resolves once
/// the chunk was taken. End it with [`close`](Self::close), or fail it with
/// [`abort`](Self::abort): the reader then gets an error, not a short stream.
/// A write after either fails with [`Error::StreamClosed`]. It is also an
/// `AsyncWrite`. A byte sink made with `to_writer` takes the
/// bytes only: messages are not written to it.
///
/// In a handler, the stream is ended when the handler returns: one it used is
/// closed when it succeeded and aborted when it failed. One it never used is
/// closed when it succeeded and left as is when it failed; in an observer
/// activation (an HTTP route target, say) it is closed only when the handler
/// returned `null` (`()`) and left as is with any other result, as the HTTP
/// worker reads an untouched response with a value as a buffered response.
/// Call [`keep_open`](Self::keep_open) to keep writing after the handler
/// returned. Dropped while used and not ended, it aborts.
pub struct OutputStream {
    inner: Arc<OutputInner>,
    pending: Mutex<(PendingWrite, PendingClose)>,
}

impl Clone for OutputStream {
    fn clone(&self) -> Self {
        Self::from_arc(self.inner.clone())
    }
}

impl OutputStream {
    fn from_arc(inner: Arc<OutputInner>) -> Self {
        Self {
            inner,
            pending: Mutex::new((None, None)),
        }
    }

    pub(crate) fn from_ref(channel: StreamChannelRef, iii: Option<&IIIClient>) -> Self {
        let stream = Self::from_arc(Arc::new(OutputInner::new(Some(channel), OutIo::Ws(None))));
        if let Some(iii) = iii {
            stream.bind(iii);
        }
        stream
    }

    /// A stream whose bytes go to `writer`: closing it flushes and shuts the
    /// writer down; an abort drops it as is.
    ///
    /// ```rust,no_run
    /// # use iii_sdk::{CallRequest, IIIClient, OutputStream};
    /// # use serde_json::json;
    /// # async fn demo(iii: &IIIClient) -> Result<(), Box<dyn std::error::Error>> {
    /// // Resolves once the file holds the whole PDF.
    /// iii.call(CallRequest::new("reports::render-pdf", json!({
    ///     "report_id": "r-42",
    ///     "out": OutputStream::to_writer(tokio::fs::File::create("r-42.pdf").await?),
    /// })))
    /// .await?;
    /// # Ok(()) }
    /// ```
    pub fn to_writer<W>(writer: W) -> Self
    where
        W: AsyncWrite + Send + 'static,
    {
        Self::from_arc(Arc::new(OutputInner::new(
            None,
            OutIo::Local(Some(Box::pin(writer))),
        )))
    }

    /// Binds a received stream to `iii`: its socket opens on that client's
    /// engine, with its handshake headers. Needed only outside a handler
    /// while several clients are alive.
    pub fn bind(&self, iii: &IIIClient) -> &Self {
        *self.inner.bound.lock_or_recover() = Some(Endpoint::of(iii));
        self
    }

    /// Writes `data`, in binary frames of up to 64 KiB. Resolves once taken.
    pub async fn write(&self, data: &[u8]) -> Result<(), Error> {
        self.inner.write(data).await
    }

    /// Sends `message` as a JSON text frame.
    pub async fn send<T: Serialize + ?Sized>(&self, message: &T) -> Result<(), Error> {
        let text = serde_json::to_string(message)?;
        self.inner.send_text(&text).await
    }

    /// Sends `text` as a text frame, as is.
    pub async fn send_text(&self, text: &str) -> Result<(), Error> {
        self.inner.send_text(text).await
    }

    /// Ends the stream: the reader reads to the end. A stream never written
    /// is opened and closed, so the reader sees an empty stream. Closing a
    /// stream already ended does nothing.
    pub async fn close(&self) -> Result<(), Error> {
        self.inner.close().await
    }

    /// Fails the stream with `reason`: the reader's read ends with
    /// [`Error::StreamAborted`] (through an engine pipe the reason is the
    /// engine's). A stream never written is opened first, so its reader
    /// always learns of it. Aborting a stream already ended does nothing.
    pub async fn abort(&self, reason: impl Into<String>) -> Result<(), Error> {
        self.inner.abort(&reason.into()).await
    }

    /// Keeps the stream open after the handler that received it returns: it
    /// is then this code's to close or abort.
    pub fn keep_open(&self) -> &Self {
        self.inner.kept_open.store(true, Ordering::SeqCst);
        self
    }
}

impl AsyncWrite for OutputStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let (write, _) = this.pending.get_mut().unwrap_or_else(|e| e.into_inner());
        if write.is_none() {
            if buf.is_empty() {
                return Poll::Ready(Ok(0));
            }
            let n = buf.len().min(MAX_FRAME_SIZE);
            let chunk = buf[..n].to_vec();
            let inner = this.inner.clone();
            *write = Some(Box::pin(async move {
                inner
                    .write(&chunk)
                    .await
                    .map(|()| n)
                    .map_err(io::Error::other)
            }));
        }
        let polled = write.as_mut().expect("set above").as_mut().poll(cx);
        if polled.is_ready() {
            *write = None;
        }
        polled
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let (write, _) = this.pending.get_mut().unwrap_or_else(|e| e.into_inner());
        match write.as_mut() {
            None => Poll::Ready(Ok(())),
            Some(pending) => {
                let polled = pending.as_mut().poll(cx);
                if polled.is_ready() {
                    *write = None;
                }
                polled.map(|written| written.map(|_| ()))
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Poll::Ready(Err(error)) = self.as_mut().poll_flush(cx) {
            return Poll::Ready(Err(error));
        }
        let this = self.get_mut();
        let (write, close) = this.pending.get_mut().unwrap_or_else(|e| e.into_inner());
        if write.is_some() {
            return Poll::Pending;
        }
        if close.is_none() {
            let inner = this.inner.clone();
            *close = Some(Box::pin(async move {
                inner.close().await.map_err(io::Error::other)
            }));
        }
        let polled = close.as_mut().expect("set above").as_mut().poll(cx);
        if polled.is_ready() {
            *close = None;
        }
        polled
    }
}

impl fmt::Debug for OutputStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutputStream")
            .field(
                "channel_id",
                &self.inner.channel.as_ref().map(|c| c.channel_id.as_str()),
            )
            .field("state", &self.inner.describe())
            .finish()
    }
}

impl Serialize for OutputStream {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.inner.wire() {
            Wire::Ref(channel) => channel.serialize(serializer),
            Wire::Placeholder(token) => {
                register(&token, Local::Output(self.inner.clone()));
                serialize_placeholder(serializer, &token, ChannelDirection::Write)
            }
        }
    }
}

impl<'de> Deserialize<'de> for OutputStream {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let wire = WireRef::deserialize(deserializer)?;
        if wire.direction != ChannelDirection::Write {
            return Err(D::Error::custom(
                "expected a stream to write (direction \"write\"): a \"read\" ref is an InputStream",
            ));
        }
        let stream = match wire.placeholder_token() {
            // In-process, the receiver gets a handle of its own on the
            // sender's stream: it ends on its own, as a pipe writer would.
            Some(token) => match claim(token) {
                Some(Local::Output(inner)) => {
                    OutputStream::from_arc(Arc::new(OutputInner::bridge(inner)))
                }
                Some(other) => {
                    register(token, other);
                    return Err(D::Error::custom(gone(&wire.channel_id)));
                }
                None => return Err(D::Error::custom(gone(&wire.channel_id))),
            },
            None => {
                let key = (wire.channel_id.clone(), ChannelDirection::Write);
                if let Some(Tracked::Output(inner)) = received_stream(&key) {
                    return Ok(OutputStream::from_arc(inner));
                }
                OutputStream::from_arc(Arc::new(OutputInner::new(
                    Some(wire.into_ref()),
                    OutIo::Ws(None),
                )))
            }
        };
        if let Some(client) = context::current_client() {
            let mut bound = stream.inner.bound.lock_or_recover();
            if bound.is_none() {
                *bound = Some(Endpoint::of(&client));
            }
        }
        if let Some(guard) = context::current_streams() {
            guard.track(Tracked::Output(stream.inner.clone()));
        }
        Ok(stream)
    }
}

impl JsonSchema for OutputStream {
    fn schema_name() -> String {
        "OutputStream".to_string()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        stream_schema(
            "OutputStream",
            ChannelDirection::Write,
            "A stream the function writes: the ref of the write end of an engine pipe \
             ({channel_id, access_key, direction: \"write\"}). The SDK opens it on first write.",
        )
    }
}

// =============================================================================
// StreamInfo and Pipe
// =============================================================================

/// A stream an observed call carried, as its observers see it: that a
/// stream was passed, which way, and its channel id. It holds no bytes.
///
/// An observer's target receives it in place of each stream ref under the
/// `input` and `output` of an [`Observed`](crate::Observed) envelope. The
/// engine still sends the ref (access key included) to the observer's worker:
/// the SDK hides it, it does not keep it off the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StreamInfo {
    /// Always `true`: marks the value as a stream.
    pub stream: bool,
    /// `read` for a stream the function read, `write` for one it wrote.
    pub direction: ChannelDirection,
    /// The channel id (a local stream's placeholder id for a call this
    /// worker dispatched in-process).
    pub id: String,
}

/// The two ends of a pipe made with [`IIIClient::pipe`]: put one in a call
/// payload and use the other here.
#[derive(Debug, Clone)]
pub struct Pipe {
    /// The read end.
    pub input: InputStream,
    /// The write end.
    pub output: OutputStream,
}

/// A pipe from the engine: `(writer, reader)` refs.
/// `iii.call`, type-erased by a plain function: a call can run a handler
/// whose result needs a pipe, so the call's future would otherwise contain
/// itself.
fn call_erased(iii: &IIIClient, request: CallRequest) -> BoxFuture<'_, Result<Value, Error>> {
    Box::pin(iii.call(request))
}

async fn create_pipe(iii: &IIIClient) -> Result<(StreamChannelRef, StreamChannelRef), Error> {
    let mut result = call_erased(
        iii,
        CallRequest::new(
            "engine::channels::create",
            json!({ "buffer_size": Value::Null }),
        ),
    )
    .await?;
    let mut end = |key: &str| -> Result<StreamChannelRef, Error> {
        let value = result
            .get_mut(key)
            .map(Value::take)
            .ok_or_else(|| Error::Serde(format!("missing '{key}' in channel response")))?;
        serde_json::from_value(value).map_err(|e| Error::Serde(e.to_string()))
    };
    let writer = end("writer")?;
    let reader = end("reader")?;
    Ok((writer, reader))
}

pub(crate) async fn pipe(iii: &IIIClient) -> Result<Pipe, Error> {
    let (writer, reader) = create_pipe(iii).await?;
    Ok(Pipe {
        input: InputStream::from_ref(reader, Some(iii)),
        output: OutputStream::from_ref(writer, Some(iii)),
    })
}

// =============================================================================
// Handler guard (R2, R3)
// =============================================================================

#[derive(Clone)]
enum Tracked {
    Input(Arc<InputInner>),
    Output(Arc<OutputInner>),
}

impl Tracked {
    fn ptr_eq(&self, other: &Tracked) -> bool {
        match (self, other) {
            (Tracked::Input(a), Tracked::Input(b)) => Arc::ptr_eq(a, b),
            (Tracked::Output(a), Tracked::Output(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Its ref was sent on: a ref of it that comes back is a stream of its
    /// own again.
    fn transferred(&self) -> bool {
        match self {
            Tracked::Input(input) => {
                matches!(input.state.lock_or_recover().status, ReadStatus::Forwarded)
            }
            Tracked::Output(output) => output.state.lock_or_recover().usage == Use::Forwarded,
        }
    }

    /// Its ref was passed on as raw JSON: the guard leaves it alone.
    fn transfer(&self) {
        match self {
            Tracked::Input(input) => {
                let mut st = input.state.lock_or_recover();
                if matches!(st.status, ReadStatus::Unread) {
                    st.status = ReadStatus::Forwarded;
                    st.reader = SENT;
                }
            }
            Tracked::Output(output) => output.transfer(),
        }
    }
}

/// A received ref: its channel id and direction.
type RefKey = (String, ChannelDirection);

/// The refs that invocations of this process received, while their handlers
/// run: a handler that deserializes one gets the stream its guard tracks, and
/// one passed on as raw JSON is marked as sent.
static RECEIVED: LazyLock<Mutex<HashMap<RefKey, Tracked>>> = LazyLock::new(Default::default);

/// How many entries `RECEIVED` holds: none, and no walk is needed.
static RECEIVED_LEN: AtomicUsize = AtomicUsize::new(0);

fn received(key: &RefKey) -> Option<Tracked> {
    if RECEIVED_LEN.load(Ordering::Acquire) == 0 {
        return None;
    }
    RECEIVED.lock_or_recover().get(key).cloned()
}

/// The stream a received ref deserializes to: the one its invocation
/// tracks, unless that one was sent on (then the ref came back, and is a
/// stream of its own).
fn received_stream(key: &RefKey) -> Option<Tracked> {
    received(key).filter(|tracked| !tracked.transferred())
}

/// How deep the walks of received values go: a ref nested deeper is not
/// tracked (and the walks stay off the stack's limits).
const WALK_DEPTH: usize = 64;

/// The real refs (not placeholders) in `value`.
fn visit_refs<'a>(value: &'a Value, depth: usize, visit: &mut impl FnMut(&'a Value)) {
    if depth > WALK_DEPTH {
        return;
    }
    match value {
        Value::Object(map) => {
            if is_channel_ref(value) {
                if placeholder_of(value).is_none() {
                    visit(value);
                }
                return;
            }
            for item in map.values() {
                visit_refs(item, depth + 1, visit);
            }
        }
        Value::Array(items) => {
            for item in items {
                visit_refs(item, depth + 1, visit);
            }
        }
        _ => {}
    }
}

fn ref_of(value: &Value) -> Option<StreamChannelRef> {
    let field = |key: &str| value.get(key).and_then(Value::as_str);
    let direction = match field("direction")? {
        "read" => ChannelDirection::Read,
        "write" => ChannelDirection::Write,
        _ => return None,
    };
    Some(StreamChannelRef {
        channel_id: field("channel_id")?.to_string(),
        access_key: field("access_key")?.to_string(),
        direction,
    })
}

/// Marks the received refs `value` passes on as raw JSON (a call payload, a
/// result) as sent: their guards leave them to the other side.
pub(crate) fn mark_transferred(value: &Value) {
    if RECEIVED_LEN.load(Ordering::Acquire) == 0 {
        return;
    }
    visit_refs(value, 0, &mut |found| {
        if let Some(channel) = ref_of(found)
            && let Some(tracked) = received(&(channel.channel_id, channel.direction))
        {
            tracked.transfer();
        }
    });
}

/// The streams an invocation received, settled when its handler returns.
pub(crate) struct StreamGuard {
    id: u64,
    /// The invocation is an observer activation.
    activation: bool,
    settled: AtomicBool,
    tracked: Mutex<Vec<Tracked>>,
    /// The refs it registered in `RECEIVED`.
    keys: Mutex<Vec<(RefKey, Tracked)>>,
}

impl StreamGuard {
    pub(crate) fn new(activation: bool) -> Self {
        Self {
            id: next_id(),
            activation,
            settled: AtomicBool::new(false),
            tracked: Mutex::new(Vec::new()),
            keys: Mutex::new(Vec::new()),
        }
    }

    fn track(&self, stream: Tracked) {
        let mut tracked = self.tracked.lock_or_recover();
        if !self.settled.load(Ordering::SeqCst) {
            if let Tracked::Input(input) = &stream {
                input.state.lock_or_recover().holder = self.id;
            }
            tracked.push(stream);
        }
    }

    /// Tracks every stream ref in the handler's input as it arrived, so the
    /// handler guard applies to them whether the handler deserializes them
    /// or not. An engine-built `Observed` envelope is a copy of another
    /// call: nothing in it is the handler's.
    pub(crate) fn track_input(&self, iii: &IIIClient, input: &Value) {
        let mut found = Vec::new();
        visit_refs(input, 0, &mut |value| found.push(value));
        if found.is_empty() {
            return;
        }
        let endpoint = Endpoint::of(iii);
        let mut map = RECEIVED.lock_or_recover();
        let mut keys = self.keys.lock_or_recover();
        for value in found {
            let Some(channel) = ref_of(value) else {
                continue;
            };
            let key = (channel.channel_id.clone(), channel.direction);
            let tracked = match channel.direction {
                ChannelDirection::Read => {
                    let inner = InputInner::new(Some(channel), None);
                    *inner.bound.lock_or_recover() = Some(endpoint.clone());
                    Tracked::Input(Arc::new(inner))
                }
                ChannelDirection::Write => {
                    let inner = OutputInner::new(Some(channel), OutIo::Ws(None));
                    *inner.bound.lock_or_recover() = Some(endpoint.clone());
                    Tracked::Output(Arc::new(inner))
                }
            };
            map.insert(key.clone(), tracked.clone());
            keys.push((key, tracked.clone()));
            self.track(tracked);
        }
        RECEIVED_LEN.store(map.len(), Ordering::Release);
    }

    /// Applies R2 and R3 once the handler returned `result`: cancels input
    /// it started reading, and starts closing or aborting output, without
    /// waiting for it.
    pub(crate) fn settle(&self, result: &Result<Value, Error>) {
        let tracked = {
            let mut tracked = self.tracked.lock_or_recover();
            self.settled.store(true, Ordering::SeqCst);
            std::mem::take(&mut *tracked)
        };
        let keys = std::mem::take(&mut *self.keys.lock_or_recover());
        if !keys.is_empty() {
            let mut map = RECEIVED.lock_or_recover();
            for (key, mine) in keys {
                if map.get(&key).is_some_and(|entry| entry.ptr_eq(&mine)) {
                    map.remove(&key);
                }
            }
            RECEIVED_LEN.store(map.len(), Ordering::Release);
        }
        if tracked.is_empty() {
            return;
        }
        let mut ending = Vec::new();
        for stream in tracked {
            match stream {
                Tracked::Input(input) => input.settle(self.id),
                Tracked::Output(output) => ending.extend(output.settle(result, self.activation)),
            }
        }
        if !ending.is_empty() {
            tokio::spawn(futures_util::future::join_all(ending));
        }
    }
}

// =============================================================================
// Outbound walk: placeholders become pipes
// =============================================================================

fn visit_placeholders(value: &Value, visit: &mut impl FnMut(&str, ChannelDirection)) {
    match value {
        Value::Object(map) => {
            if let Some((token, direction)) = placeholder_of(value) {
                visit(token, direction);
                return;
            }
            for item in map.values() {
                visit_placeholders(item, visit);
            }
        }
        Value::Array(items) => {
            for item in items {
                visit_placeholders(item, visit);
            }
        }
        _ => {}
    }
}

fn replace_placeholders(value: &mut Value, refs: &mut HashMap<String, Value>) {
    if let Some((token, _)) = placeholder_of(value) {
        if let Some(wire) = refs.remove(token) {
            *value = wire;
        }
        return;
    }
    match value {
        Value::Object(map) => {
            for item in map.values_mut() {
                replace_placeholders(item, refs);
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_placeholders(item, refs);
            }
        }
        _ => {}
    }
}

/// Whether `value` holds a placeholder (a stream that must not reach the
/// wire as is).
pub(crate) fn has_placeholder(value: &Value) -> bool {
    if !placeholders_used() {
        return false;
    }
    let mut found = false;
    visit_placeholders(value, &mut |_, _| found = true);
    found
}

/// Whether `value` holds a stream: a ref or a placeholder.
pub(crate) fn contains_stream(value: &Value) -> bool {
    if is_channel_ref(value) {
        return true;
    }
    match value {
        Value::Object(map) => map.values().any(contains_stream),
        Value::Array(items) => items.iter().any(contains_stream),
        _ => false,
    }
}

pub(crate) fn enqueue_rejection() -> Error {
    Error::StreamNotAllowed(
        "a payload with a stream cannot be enqueued: streams need a live call (the job can run \
         after the pipe expired, and a retry cannot read the stream again); call it, or use \
         CallAction::Void"
            .into(),
    )
}

/// Drops the streams `value` carries as placeholders: a result nobody reads.
pub(crate) fn release(value: &Value) {
    if !placeholders_used() {
        return;
    }
    let mut tokens = Vec::new();
    visit_placeholders(value, &mut |token, _| tokens.push(token.to_string()));
    for token in tokens {
        drop(claim(&token));
    }
}

/// A pump task and its cancel signal.
struct Pump {
    cancel: Option<oneshot::Sender<String>>,
    task: tokio::task::JoinHandle<Result<(), Error>>,
}

/// The pumps started for what one call or result sends.
#[derive(Default)]
pub(crate) struct Pumps {
    sources: Vec<Pump>,
    sinks: Vec<Pump>,
}

impl Pumps {
    /// Settles a call that sent these pumps' streams (R4): with the result,
    /// it resolves once every sink got its whole stream, and fails with the
    /// error of a sink that did not (`STREAM_ABORTED` for an abort). With an
    /// error, it stops every pump (aborting their streams with it) first.
    ///
    /// `detach` (a void call) leaves the pumps running on their own once the
    /// call was sent, and so does a result that holds a stream to write (the
    /// caller writes it after the call returned, maybe into one of these
    /// sinks). The wait ends at `deadline`, the call's timeout: the sinks
    /// still open are then aborted, and the call fails with a timeout.
    pub(crate) async fn settle(
        self,
        result: Result<Value, Error>,
        detach: bool,
        deadline: tokio::time::Instant,
    ) -> Result<Value, Error> {
        if self.sources.is_empty() && self.sinks.is_empty() {
            return result;
        }
        let Pumps { mut sources, sinks } = self;
        let detach = detach || result.as_ref().is_ok_and(has_write_stream);
        if detach && result.is_ok() {
            // Dropping a pump's cancel sender detaches it: it runs to the end.
            return result;
        }
        match result {
            Ok(value) => {
                // Sources keep pumping: the function reads them.
                drop(sources);
                let (cancels, mut tasks): (Vec<_>, Vec<_>) = sinks
                    .into_iter()
                    .map(|pump| (pump.cancel, pump.task))
                    .unzip();
                // How many tasks finished: a finished JoinHandle is not polled again.
                let mut finished = 0;
                let wait = async {
                    let mut failure = None;
                    for task in tasks.iter_mut() {
                        let outcome = match task.await {
                            Ok(outcome) => outcome,
                            Err(error) => {
                                Err(Error::Runtime(format!("stream pump stopped: {error}")))
                            }
                        };
                        finished += 1;
                        if let Err(error) = outcome {
                            failure.get_or_insert(error);
                        }
                    }
                    failure
                };
                let waited = tokio::time::timeout_at(deadline, wait).await;
                match waited {
                    Ok(Some(error)) => Err(error),
                    Ok(None) => Ok(value),
                    Err(_) => {
                        for cancel in cancels.into_iter().flatten() {
                            let _ = cancel.send(SINK_TIMEOUT.to_string());
                        }
                        // The sinks are aborted before the call fails, as on
                        // the error path: a write to one after it fails.
                        for task in tasks.into_iter().skip(finished) {
                            let _ = task.await;
                        }
                        Err(Error::Timeout)
                    }
                }
            }
            Err(error) => {
                let reason = error.to_string();
                let mut sinks = sinks;
                for pump in sources.iter_mut().chain(sinks.iter_mut()) {
                    if let Some(cancel) = pump.cancel.take() {
                        let _ = cancel.send(reason.clone());
                    }
                }
                for sink in sinks {
                    let _ = sink.task.await;
                }
                Err(error)
            }
        }
    }
}

/// The reason a sink still open at the call's timeout is aborted with.
const SINK_TIMEOUT: &str = "the call timed out before its sinks ended";

/// Whether `value` holds a stream to write: a write ref, or a local one.
pub(crate) fn has_write_stream(value: &Value) -> bool {
    fn walk(value: &Value, depth: usize) -> bool {
        if depth > WALK_DEPTH {
            return false;
        }
        match value {
            Value::Object(map) => {
                if is_channel_ref(value) {
                    return value.get("direction").and_then(Value::as_str) == Some("write");
                }
                map.values().any(|item| walk(item, depth + 1))
            }
            Value::Array(items) => items.iter().any(|item| walk(item, depth + 1)),
            _ => false,
        }
    }
    walk(value, 0)
}

/// Refuses a payload that holds one stream twice (`STREAM_IN_USE`), on
/// every path, before anything is sent.
pub(crate) fn check_sent_once(value: &Value) -> Result<(), Error> {
    if !placeholders_used() {
        return Ok(());
    }
    let mut seen: Vec<(String, ChannelDirection)> = Vec::new();
    let mut twice = false;
    let mut note = |identity: (String, ChannelDirection)| {
        if seen.contains(&identity) {
            twice = true;
        } else {
            seen.push(identity);
        }
    };
    let mut ended = false;
    visit_streams(value, 0, &mut |found| {
        if let Some((token, direction)) = placeholder_of(found) {
            // A received stream sent again is a placeholder of its own ref.
            let channel = match peek(token) {
                Some(Local::Input(input)) => {
                    ended |= input.read_to_the_end();
                    input.channel.clone()
                }
                Some(Local::Output(output)) => output.channel.clone(),
                None => None,
            };
            match channel {
                Some(channel) => note((channel.channel_id, direction)),
                None => note((format!("{PLACEHOLDER_PREFIX}{token}"), direction)),
            }
        } else if let Some(channel) = ref_of(found) {
            note((channel.channel_id, channel.direction));
        }
    });
    if twice {
        Err(sent_twice())
    } else if ended {
        // Refused here on every path, as a remote call's walk refuses it.
        Err(read_to_the_end())
    } else {
        Ok(())
    }
}

/// The streams in `value`: refs and placeholders.
fn visit_streams<'a>(value: &'a Value, depth: usize, visit: &mut impl FnMut(&'a Value)) {
    if depth > WALK_DEPTH {
        return;
    }
    match value {
        Value::Object(map) => {
            if is_channel_ref(value) {
                visit(value);
                return;
            }
            for item in map.values() {
                visit_streams(item, depth + 1, visit);
            }
        }
        Value::Array(items) => {
            for item in items {
                visit_streams(item, depth + 1, visit);
            }
        }
        _ => {}
    }
}

/// Moves `frames` into `out` until they end (then closes it) or fail (then
/// aborts it with the reason). A cancel aborts it with the cancel's reason.
async fn pump(
    mut frames: FrameStream,
    out: Arc<OutputInner>,
    mut cancel: Option<oneshot::Receiver<String>>,
) -> Result<(), Error> {
    loop {
        let next = match cancel.as_mut() {
            Some(signal) => tokio::select! {
                biased;
                reason = signal => match reason {
                    Ok(reason) => {
                        drop(frames);
                        let _ = out.abort(&reason).await;
                        return Err(Error::Runtime(format!("stream stopped: {reason}")));
                    }
                    // Detached: pump to the end.
                    Err(_) => {
                        cancel = None;
                        continue;
                    }
                },
                next = frames.next() => next,
            },
            None => frames.next().await,
        };
        match next {
            Some(Ok(Frame::Binary(data))) => out.write(&data).await?,
            Some(Ok(Frame::Text(text))) => out.send_text(&text).await?,
            Some(Err(error)) => {
                let reason = match &error {
                    Error::StreamAborted { reason } => reason.clone(),
                    other => other.to_string(),
                };
                let _ = out.abort(&reason).await;
                return Err(error);
            }
            None => return out.close().await,
        }
    }
}

enum Claimed {
    Source(FrameStream),
    Sink(Arc<OutputInner>),
}

/// Replaces the placeholders in `value` (what a call or a result sends) with
/// pipe refs and starts their pumps: each source pumped into the writer end
/// of its pipe, the reader end of each pipe into its sink. The pipes are
/// created in parallel; a failure fails before anything is sent.
pub(crate) async fn prepare(iii: &IIIClient, value: &mut Value) -> Result<Pumps, Error> {
    if !placeholders_used() {
        return Ok(Pumps::default());
    }
    // Boxed: its state (a dial among it) would otherwise grow every call's
    // future, streams or not.
    Box::pin(prepare_streams(iii, value)).await
}

async fn prepare_streams(iii: &IIIClient, value: &mut Value) -> Result<Pumps, Error> {
    let mut tokens = Vec::new();
    visit_placeholders(value, &mut |token, direction| {
        tokens.push((token.to_string(), direction))
    });
    if tokens.is_empty() {
        return Ok(Pumps::default());
    }

    // Claim them all, or none.
    let mut seen = HashSet::new();
    let mut entries = Vec::with_capacity(tokens.len());
    let mut failure = None;
    for (token, direction) in &tokens {
        if !seen.insert(token.as_str()) {
            failure = Some(sent_twice());
            break;
        }
        match (claim(token), direction) {
            (Some(Local::Input(input)), ChannelDirection::Read) => {
                entries.push((token.clone(), Local::Input(input)))
            }
            (Some(Local::Output(output)), ChannelDirection::Write) => {
                entries.push((token.clone(), Local::Output(output)))
            }
            (other, _) => {
                if let Some(other) = other {
                    register(token, other);
                }
                failure = Some(Error::Runtime(gone(&format!(
                    "{PLACEHOLDER_PREFIX}{token}"
                ))));
                break;
            }
        }
    }
    if let Some(error) = failure {
        for (token, entry) in entries {
            register(&token, entry);
        }
        return Err(error);
    }

    let mut claimed = Vec::with_capacity(entries.len());
    for (token, entry) in entries {
        let taken = match entry {
            Local::Input(input) => input.take_source().map(Claimed::Source),
            Local::Output(output) => output.take_sink().map(|()| Claimed::Sink(output)),
        };
        match taken {
            Ok(taken) => claimed.push((token, taken)),
            Err(error) => {
                abandon(claimed, &error).await;
                return Err(error);
            }
        }
    }

    let pipes = futures_util::future::try_join_all(claimed.iter().map(|_| create_pipe(iii))).await;
    let pipes = match pipes {
        Ok(pipes) => pipes,
        Err(error) => {
            abandon(claimed, &error).await;
            return Err(error);
        }
    };

    let mut refs = HashMap::with_capacity(claimed.len());
    let mut pumps = Pumps::default();
    for ((token, taken), (writer, reader)) in claimed.into_iter().zip(pipes) {
        let (cancel, signal) = oneshot::channel();
        match taken {
            Claimed::Source(frames) => {
                refs.insert(token, serde_json::to_value(&reader)?);
                let out = OutputStream::from_ref(writer, Some(iii)).inner;
                pumps.sources.push(Pump {
                    cancel: Some(cancel),
                    task: tokio::spawn(pump(frames, out, Some(signal))),
                });
            }
            Claimed::Sink(sink) => {
                refs.insert(token, serde_json::to_value(&writer)?);
                let endpoint = Endpoint::of(iii);
                let frames = read_frames(channel_url(&endpoint.address, &reader), endpoint.headers);
                pumps.sinks.push(Pump {
                    cancel: Some(cancel),
                    task: tokio::spawn(pump(frames, sink, Some(signal))),
                });
            }
        }
    }
    replace_placeholders(value, &mut refs);
    Ok(pumps)
}

/// Ends what a failed walk claimed: its sinks abort with the error.
async fn abandon(claimed: Vec<(String, Claimed)>, error: &Error) {
    let reason = error.to_string();
    for (_, taken) in claimed {
        if let Claimed::Sink(sink) = taken {
            let _ = sink.abort(&reason).await;
        }
    }
}

// =============================================================================
// In-process calls (R4 for local sinks)
// =============================================================================

/// The sinks `payload` passes as placeholders, for a call this worker
/// dispatches in-process.
pub(crate) fn local_sinks(payload: &Value) -> Vec<(String, Arc<OutputInner>)> {
    if !placeholders_used() {
        return Vec::new();
    }
    let mut sinks = Vec::new();
    visit_placeholders(payload, &mut |token, direction| {
        if direction == ChannelDirection::Write
            && let Some(Local::Output(sink)) = peek(token)
        {
            sinks.push((token.to_string(), sink));
        }
    });
    sinks
}

/// Settles an in-process call's sinks (R4), as `Pumps::settle` does a
/// remote call's: with the result, a sink the handler never took, or dropped
/// unused, is closed (nothing else can end it), then the call waits for every
/// sink to end, until `deadline`, and fails with `STREAM_ABORTED` if one
/// aborted. A result that holds a stream to write is not waited for. With an
/// error, the sinks still open are aborted with it.
pub(crate) async fn settle_local_sinks(
    sinks: Vec<(String, Arc<OutputInner>)>,
    result: Result<Value, Error>,
    deadline: tokio::time::Instant,
) -> Result<Value, Error> {
    if sinks.is_empty() {
        return result;
    }
    // Boxed, as `prepare`: a close can dial.
    Box::pin(settle_sinks(sinks, result, deadline)).await
}

async fn settle_sinks(
    sinks: Vec<(String, Arc<OutputInner>)>,
    result: Result<Value, Error>,
    deadline: tokio::time::Instant,
) -> Result<Value, Error> {
    async fn abort_all(sinks: &[(String, Arc<OutputInner>)], reason: &str) {
        for (token, sink) in sinks {
            drop(claim(token));
            let _ = sink.abort(reason).await;
        }
    }
    match result {
        Ok(value) => {
            for (token, sink) in &sinks {
                if claim(token).is_some() {
                    let _ = sink.close().await;
                }
            }
            if has_write_stream(&value) {
                return Ok(value);
            }
            let wait = async {
                for (_, sink) in &sinks {
                    match sink.settled().await {
                        Some(Outcome::Closed) => {}
                        Some(Outcome::Aborted(reason)) => {
                            return Err(Error::StreamAborted { reason });
                        }
                        // Its receiver dropped it unused: a plain call's
                        // untouched stream ends.
                        None => {
                            let _ = sink.close().await;
                        }
                    }
                }
                Ok(())
            };
            match tokio::time::timeout_at(deadline, wait).await {
                Ok(Ok(())) => Ok(value),
                Ok(Err(error)) => Err(error),
                Err(_) => {
                    abort_all(&sinks, SINK_TIMEOUT).await;
                    Err(Error::Timeout)
                }
            }
        }
        Err(error) => {
            abort_all(&sinks, &error.to_string()).await;
            Err(error)
        }
    }
}

// =============================================================================
// Observed calls: refs become StreamInfo
// =============================================================================

fn stream_info(channel_id: &str, direction: &str) -> Value {
    json!({ "stream": true, "direction": direction, "id": channel_id })
}

fn refs_to_info(value: &mut Value) {
    if is_channel_ref(value) {
        let channel_id = value["channel_id"].as_str().unwrap_or_default().to_string();
        let direction = value["direction"].as_str().unwrap_or("read").to_string();
        *value = stream_info(&channel_id, &direction);
        return;
    }
    match value {
        Value::Object(map) => map.values_mut().for_each(refs_to_info),
        Value::Array(items) => items.iter_mut().for_each(refs_to_info),
        _ => {}
    }
}

/// An observer activation of an engine-built [`Observed`](crate::Observed)
/// envelope (its `activation_id` and `observer_id` are the invocation's):
/// the stream refs under its `input` and `output` become [`StreamInfo`]. A
/// call through an observer gets its activation id from the engine after the
/// caller built the payload, so its payload never matches.
pub(crate) fn rewrite_observed(data: &mut Value, activation: &Activation) -> bool {
    let Some(activation_id) = activation.activation_id.as_deref() else {
        return false;
    };
    let Some(envelope) = data.as_object_mut() else {
        return false;
    };
    if envelope.get("activation_id").and_then(Value::as_str) != Some(activation_id)
        || envelope.get("observer_id").and_then(Value::as_str)
            != Some(activation.observer_id.as_str())
    {
        return false;
    }
    for key in ["input", "output"] {
        if let Some(value) = envelope.get_mut(key) {
            refs_to_info(value);
        }
    }
    true
}

/// `value` as a report of an in-process call carries it: a local stream
/// becomes a [`StreamInfo`], since its placeholder must not reach the wire.
/// `None` when it holds none.
pub(crate) fn report_form(value: &Value) -> Option<Value> {
    if !has_placeholder(value) {
        return None;
    }
    fn convert(value: &mut Value) {
        if let Some((token, direction)) = placeholder_of(value) {
            let id = format!("{PLACEHOLDER_PREFIX}{token}");
            *value = stream_info(&id, direction.as_str());
            return;
        }
        match value {
            Value::Object(map) => map.values_mut().for_each(convert),
            Value::Array(items) => items.iter_mut().for_each(convert),
            _ => {}
        }
    }
    let mut value = value.clone();
    convert(&mut value);
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::TryStreamExt;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn a_local_source_reads_once() {
        let input = InputStream::from_bytes(&b"one\r\ntwo\nthree"[..]);
        let lines: Vec<String> = input.lines().try_collect().await.unwrap();
        assert_eq!(lines, ["one", "two", "three"]);
        let again = input.read_all().await.unwrap_err();
        assert!(
            again.to_string().contains("already has a reader"),
            "{again}"
        );
    }

    #[tokio::test]
    async fn async_read_keeps_what_the_buffer_could_not_take() {
        let mut input = InputStream::from_stream(futures_util::stream::iter([
            Ok::<_, std::io::Error>(Bytes::from_static(b"abcdef")),
            Ok(Bytes::from_static(b"gh")),
        ]));
        let mut first = [0u8; 4];
        input.read_exact(&mut first).await.unwrap();
        assert_eq!(&first, b"abcd");
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).await.unwrap();
        assert_eq!(rest, b"efgh");
    }

    #[tokio::test]
    async fn a_failing_source_aborts() {
        let input = InputStream::from_stream(futures_util::stream::iter([
            Ok(Bytes::from_static(b"ab")),
            Err("disk gone"),
        ]));
        let error = input.read_all().await.unwrap_err();
        assert_eq!(error.code(), Some(crate::protocol::STREAM_ABORTED));
        assert!(error.to_string().contains("disk gone"), "{error}");
    }

    #[tokio::test]
    async fn messages_decode_json_and_fall_back_to_the_raw_string() {
        assert_eq!(
            decode_message::<Value>(r#"{"a":1}"#.into()).unwrap(),
            json!({"a": 1})
        );
        assert_eq!(
            decode_message::<Value>("not json".into()).unwrap(),
            json!("not json")
        );
        assert_eq!(decode_message::<String>("plain".into()).unwrap(), "plain");
        assert!(decode_message::<u32>("nope".into()).is_err());
    }

    #[tokio::test]
    async fn a_local_stream_serializes_to_a_placeholder_and_deserializes_back() {
        let input = InputStream::from_bytes(&b"hi"[..]);
        let value = serde_json::to_value(&input).unwrap();
        let (token, direction) = placeholder_of(&value).expect("a placeholder");
        assert_eq!(direction, ChannelDirection::Read);
        assert!(peek(token).is_some());
        assert!(has_placeholder(&json!({ "body": value.clone() })));
        let back: InputStream = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(back.read_all().await.unwrap(), b"hi");
        // claimed: a second receiver gets an error, not the stream
        assert!(serde_json::from_value::<InputStream>(value).is_err());
    }

    #[tokio::test]
    async fn a_received_stream_not_yet_used_serializes_to_its_ref() {
        let wire = json!({"channel_id": "c1", "access_key": "k1", "direction": "read", "x": 1});
        let input: InputStream = serde_json::from_value(wire).unwrap();
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            json!({"channel_id": "c1", "access_key": "k1", "direction": "read"})
        );
        // sent on: it has a reader elsewhere
        assert!(input.read_all().await.is_err());

        let output: OutputStream = serde_json::from_value(
            json!({"channel_id": "c2", "access_key": "k2", "direction": "write"}),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&output).unwrap(),
            json!({"channel_id": "c2", "access_key": "k2", "direction": "write"})
        );
        assert!(output.write(b"x").await.is_err());
    }

    async fn read_by(how: u8, mut input: InputStream) -> Result<Vec<u8>, Error> {
        match how {
            0 => {
                let chunks: Vec<Bytes> = (&mut input).try_collect().await?;
                Ok(chunks.concat())
            }
            1 => {
                let mut got = Vec::new();
                match input.read_to_end(&mut got).await {
                    Ok(_) => Ok(got),
                    Err(error) => Err(*error
                        .into_inner()
                        .and_then(|inner| inner.downcast::<Error>().ok())
                        .expect("an Error inside")),
                }
            }
            _ => input.read_all().await,
        }
    }

    #[tokio::test]
    async fn a_clone_is_another_reader_on_every_path() {
        // 0: `Stream`, 1: `AsyncRead`, 2: `read_all`
        for (first, second) in [
            (0, 1),
            (1, 0),
            (0, 2),
            (1, 2),
            (2, 0),
            (2, 1),
            (0, 0),
            (1, 1),
        ] {
            let input = InputStream::from_bytes(&b"abc"[..]);
            let other = input.clone();
            assert_eq!(read_by(first, input).await.unwrap(), b"abc");
            let error = read_by(second, other).await.unwrap_err();
            assert_eq!(
                error.code(),
                Some(crate::protocol::STREAM_IN_USE),
                "{first} {second}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn writes_after_close_or_abort_are_stream_closed() {
        for abort in [false, true] {
            let out = OutputStream::to_writer(tokio::io::sink());
            if abort {
                out.abort("stop").await.unwrap();
            } else {
                out.close().await.unwrap();
            }
            for error in [
                out.write(b"x").await.unwrap_err(),
                out.send_text("x").await.unwrap_err(),
            ] {
                assert_eq!(
                    error.code(),
                    Some(crate::protocol::STREAM_CLOSED),
                    "{abort}: {error}"
                );
            }
        }
    }

    #[tokio::test]
    async fn one_stream_twice_in_a_payload_is_in_use() {
        let raw = json!({"channel_id": "c8", "access_key": "k", "direction": "read"});
        let input = InputStream::from_bytes(&b"x"[..]);
        let output = OutputStream::to_writer(tokio::io::sink());
        let received: InputStream = serde_json::from_value(
            json!({"channel_id": "c9", "access_key": "k", "direction": "read"}),
        )
        .unwrap();
        for payload in [
            json!({ "a": input, "b": input }),
            json!([output, { "again": output }]),
            // its ref, then a placeholder: it was sent already
            json!({ "a": received, "b": received }),
            json!({ "a": raw, "b": [raw] }),
        ] {
            let error = check_sent_once(&payload).unwrap_err();
            assert_eq!(error.code(), Some(crate::protocol::STREAM_IN_USE));
            assert_eq!(
                error.to_string(),
                "a stream can be sent once: the same stream appears twice"
            );
            release(&payload);
        }
        let (a, b) = (
            InputStream::from_bytes(&b"y"[..]),
            InputStream::from_bytes(&b"z"[..]),
        );
        let fine = json!({ "a": a, "b": b, "read": raw, "write": {"channel_id": "c8", "access_key": "k", "direction": "write"} });
        assert!(check_sent_once(&fine).is_ok());
        release(&fine);
    }

    #[tokio::test]
    async fn a_stream_read_to_the_end_or_already_sent_is_in_use() {
        let code = |error: Error| error.code().map(str::to_string);
        let in_use = Some(crate::protocol::STREAM_IN_USE.to_string());
        let input = InputStream::from_bytes(&b"x"[..]);
        input.read_all().await.unwrap();
        assert_eq!(
            input.inner.take_source().err().map(code),
            Some(in_use.clone())
        );

        let received: InputStream = serde_json::from_value(
            json!({"channel_id": "c10", "access_key": "k", "direction": "read"}),
        )
        .unwrap();
        serde_json::to_value(&received).unwrap();
        assert_eq!(
            received.inner.take_source().err().map(code),
            Some(in_use.clone())
        );

        let output: OutputStream = serde_json::from_value(
            json!({"channel_id": "c11", "access_key": "k", "direction": "write"}),
        )
        .unwrap();
        serde_json::to_value(&output).unwrap();
        assert_eq!(code(output.inner.take_sink().unwrap_err()), in_use);
        assert_eq!(code(output.write(b"x").await.unwrap_err()), in_use);
    }

    #[tokio::test]
    async fn an_in_process_receiver_ends_its_handle_on_its_own() {
        let handle_of = |target: &OutputStream| -> OutputStream {
            serde_json::from_value(serde_json::to_value(target).unwrap()).unwrap()
        };
        // Used, kept open and dropped unfinished: the target aborts.
        let target = OutputStream::to_writer(tokio::io::sink());
        let handle = handle_of(&target);
        handle.keep_open();
        handle.write(b"x").await.unwrap();
        drop(handle);
        assert_eq!(
            target.inner.finished().await,
            Outcome::Aborted("dropped without close".into())
        );
        // Dropped unused: released for the call to decide, not ended.
        let target = OutputStream::to_writer(tokio::io::sink());
        drop(handle_of(&target));
        assert_eq!(target.inner.settled().await, None);
        assert!(target.inner.state.lock_or_recover().outcome.is_none());
        // Closed: the target closes.
        let (writer, mut reader) = tokio::io::duplex(64);
        let target = OutputStream::to_writer(writer);
        let handle = handle_of(&target);
        handle.write(b"ok").await.unwrap();
        handle.close().await.unwrap();
        assert_eq!(target.inner.finished().await, Outcome::Closed);
        let mut got = Vec::new();
        reader.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"ok");
    }

    #[test]
    fn a_result_with_a_stream_to_write_is_not_waited_for() {
        let write = json!({"channel_id": "c", "access_key": "k", "direction": "write"});
        let read = json!({"channel_id": "c", "access_key": "k", "direction": "read"});
        assert!(has_write_stream(&json!({ "out": [write] })));
        assert!(!has_write_stream(&json!({ "body": read, "n": 1 })));
        assert!(!has_write_stream(&Value::Null));
    }

    #[test]
    fn the_wrong_direction_does_not_deserialize() {
        let read = json!({"channel_id": "c", "access_key": "k", "direction": "read"});
        let write = json!({"channel_id": "c", "access_key": "k", "direction": "write"});
        assert!(serde_json::from_value::<OutputStream>(read).is_err());
        assert!(serde_json::from_value::<InputStream>(write).is_err());
    }

    #[tokio::test]
    async fn a_local_sink_gets_the_bytes_and_ends_once() {
        let (writer, mut reader) = tokio::io::duplex(1024);
        let out = OutputStream::to_writer(writer);
        out.write(b"pdf").await.unwrap();
        out.send(&json!({"ignored": true})).await.unwrap();
        out.close().await.unwrap();
        out.close().await.unwrap();
        out.abort("late").await.unwrap();
        assert!(out.write(b"more").await.is_err());
        let mut got = Vec::new();
        reader.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"pdf");
        assert_eq!(out.inner.finished().await, Outcome::Closed);
    }

    #[tokio::test]
    async fn a_used_sink_dropped_without_close_aborts() {
        let (writer, _reader) = tokio::io::duplex(1024);
        let out = OutputStream::to_writer(writer);
        out.write(b"x").await.unwrap();
        let mut done = out.inner.done.subscribe();
        drop(out);
        let outcome = done.wait_for(Option::is_some).await.unwrap().clone();
        assert_eq!(
            outcome,
            Some(Outcome::Aborted("dropped without close".into()))
        );
    }

    #[tokio::test]
    async fn the_guard_ends_an_untouched_output_by_the_kind_of_invocation() {
        let ok_value: Result<Value, Error> = Ok(json!({ "body": "buffered" }));
        let ok_null: Result<Value, Error> = Ok(Value::Null);
        let failed: Result<Value, Error> = Err(Error::Handler("x".into()));
        // (activation, result, the guard ends it)
        for (activation, result, ends) in [
            (false, &ok_value, true),
            (false, &ok_null, true),
            (false, &failed, false),
            (true, &ok_value, false),
            (true, &ok_null, true),
            (true, &failed, false),
        ] {
            let out = OutputStream::to_writer(tokio::io::sink());
            let settled = out.inner.settle(result, activation);
            assert_eq!(settled.is_some(), ends, "{activation} {result:?}");
        }
        // A used one is closed on success and aborted on failure, either way.
        for activation in [false, true] {
            let out = OutputStream::to_writer(tokio::io::sink());
            out.write(b"x").await.unwrap();
            out.inner.settle(&failed, activation).unwrap().await;
            assert_eq!(
                out.inner.finished().await,
                Outcome::Aborted("handler error: x".into())
            );
            let out = OutputStream::to_writer(tokio::io::sink());
            out.write(b"x").await.unwrap();
            out.inner.settle(&ok_value, activation).unwrap().await;
            assert_eq!(out.inner.finished().await, Outcome::Closed);
        }
    }

    #[test]
    fn observed_refs_become_stream_info_only_for_an_engine_envelope() {
        let r = json!({"channel_id": "c", "access_key": "k", "direction": "read"});
        let activation = Activation {
            observer_id: "o".into(),
            activation_id: Some("a".into()),
        };
        let mut observed = json!({
            "observer_id": "o", "activation_id": "a",
            "input": { "body": r.clone(), "list": [r.clone()] }, "output": r.clone(),
            "metadata": { "kept": r.clone() },
        });
        rewrite_observed(&mut observed, &activation);
        let info = json!({"stream": true, "direction": "read", "id": "c"});
        assert_eq!(observed["input"]["body"], info);
        assert_eq!(observed["input"]["list"][0], info);
        assert_eq!(observed["output"], info);
        assert_eq!(observed["metadata"]["kept"], r);
        let parsed: StreamInfo = serde_json::from_value(info).unwrap();
        assert_eq!(parsed.direction, ChannelDirection::Read);

        // A call through an observer: the caller's payload never matches.
        let mut through = json!({ "observer_id": "o", "input": r.clone() });
        rewrite_observed(&mut through, &activation);
        assert_eq!(through["input"], r);
    }

    #[test]
    fn the_schema_is_the_ref_object_marked_as_a_stream() {
        let schema = crate::iii::json_schema_for::<InputStream>().unwrap();
        assert_eq!(schema["title"], "InputStream");
        assert_eq!(schema["properties"]["direction"]["const"], "read");
        assert_eq!(schema["x-iii-stream"], "read");
        let schema = crate::iii::json_schema_for::<OutputStream>().unwrap();
        assert_eq!(schema["properties"]["direction"]["const"], "write");
    }

    #[test]
    fn enqueue_sees_refs_and_placeholders() {
        assert!(!contains_stream(&json!({"a": [1, {"b": "c"}]})));
        assert!(contains_stream(
            &json!({"a": [{"channel_id": "c", "access_key": "k", "direction": "write"}]})
        ));
        let value = serde_json::to_value(InputStream::from_bytes(&b""[..])).unwrap();
        assert!(contains_stream(&json!({ "x": value })));
        release(&json!({ "x": value }));
    }

    #[test]
    fn a_report_carries_stream_info_for_a_local_stream() {
        let value = serde_json::to_value(InputStream::from_bytes(&b"x"[..])).unwrap();
        let report = report_form(&json!({ "body": value.clone() })).unwrap();
        assert_eq!(report["body"]["stream"], true);
        assert_eq!(report["body"]["id"], value["channel_id"]);
        assert!(!has_placeholder(&report));
        release(&value);
    }
}
