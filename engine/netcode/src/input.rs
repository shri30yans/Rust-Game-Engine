//! Tick-stamped input frames, redundancy bundling, and server-side dedup.

use std::collections::VecDeque;

use core::marker::PhantomData;
use engine_core::Tick;
use serde::de::{DeserializeOwned, SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::NetError;
use crate::protocol::MAX_MESSAGE_BYTES;
use crate::snapshot::PacketAck;

/// How many past frames ride along with the newest one in a bundle.
pub const INPUT_REDUNDANCY: usize = 4;
/// How many ticks of input history the client retains locally.
pub const INPUT_HISTORY: usize = 64;

/// Per-tick input contents supplied by the game (or a kit).
pub trait InputPayload:
    Serialize + DeserializeOwned + Clone + PartialEq + core::fmt::Debug
{
    /// Upper bound on the postcard-encoded size of one payload.
    const MAX_BYTES: usize;

    /// Rejects impossible values after decode.
    ///
    /// # Errors
    /// Returns [`NetError`] when the payload is illegal for the game.
    fn validate(&self) -> Result<(), NetError>;
}

impl InputPayload for () {
    const MAX_BYTES: usize = 1;

    fn validate(&self) -> Result<(), NetError> {
        Ok(())
    }
}

/// One simulation tick's validated input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputFrame<I> {
    tick: Tick,
    payload: I,
}

impl<I: InputPayload> InputFrame<I> {
    /// Builds a frame after validation succeeds.
    ///
    /// # Errors
    /// Propagates payload validation failures.
    pub fn new(tick: Tick, payload: I) -> Result<Self, NetError> {
        payload.validate()?;
        Ok(InputFrame { tick, payload })
    }

    /// Simulation tick this input applies to.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Borrow the payload.
    #[must_use]
    pub const fn payload(&self) -> &I {
        &self.payload
    }
}

/// Newest frame first, plus up to [`INPUT_REDUNDANCY`] strictly older contiguous ticks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputBundle<I> {
    frames: InputFrames<I>,
    snapshot_ack: PacketAck,
}

/// Inline bounded storage for the newest input and its redundancy frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFrames<I> {
    frames: [Option<InputFrame<I>>; INPUT_REDUNDANCY + 1],
    len: usize,
}

impl<I> InputFrames<I> {
    fn new() -> Self {
        Self {
            frames: core::array::from_fn(|_| None),
            len: 0,
        }
    }

    fn push(&mut self, frame: InputFrame<I>) -> Result<(), InputFrame<I>> {
        let Some(slot) = self.frames.get_mut(self.len) else {
            return Err(frame);
        };
        *slot = Some(frame);
        self.len += 1;
        Ok(())
    }

    /// Number of stored frames.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no frames are stored.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Frame at a newest-first offset.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&InputFrame<I>> {
        self.frames.get(index).and_then(Option::as_ref)
    }

    /// Iterates stored frames newest first.
    pub fn iter(&self) -> impl Iterator<Item = &InputFrame<I>> {
        self.frames.iter().take(self.len).filter_map(Option::as_ref)
    }
}

impl<I: Serialize> Serialize for InputFrames<I> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut sequence = serializer.serialize_seq(Some(self.len))?;
        for frame in self.iter() {
            sequence.serialize_element(frame)?;
        }
        sequence.end()
    }
}

impl<'de, I: Deserialize<'de>> Deserialize<'de> for InputFrames<I> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct FramesVisitor<I>(PhantomData<I>);

        impl<'de, I: Deserialize<'de>> Visitor<'de> for FramesVisitor<I> {
            type Value = InputFrames<I>;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(formatter, "at most {} input frames", INPUT_REDUNDANCY + 1)
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut frames = InputFrames::new();
                while let Some(frame) = sequence.next_element()? {
                    if frames.push(frame).is_err() {
                        return Err(serde::de::Error::invalid_length(
                            INPUT_REDUNDANCY + 2,
                            &self,
                        ));
                    }
                }
                Ok(frames)
            }
        }

        deserializer.deserialize_seq(FramesVisitor(PhantomData))
    }
}

impl<I> InputBundle<I> {
    /// Frames stored newest-first (only [`InputHistory::bundle`] constructs bundles).
    pub(crate) fn from_frames(frames: InputFrames<I>, snapshot_ack: PacketAck) -> Self {
        InputBundle {
            frames,
            snapshot_ack,
        }
    }

    /// Newest-first view of bundled frames.
    #[must_use]
    pub fn frames(&self) -> &InputFrames<I> {
        &self.frames
    }

    /// Newest received server state packet, if any.
    #[must_use]
    pub const fn snapshot_ack(&self) -> PacketAck {
        self.snapshot_ack
    }
}

/// Fixed-capacity ring of recent client input frames (strictly increasing ticks).
pub struct InputHistory<I> {
    frames: VecDeque<InputFrame<I>>,
    last_tick: Option<Tick>,
    snapshot_ack: PacketAck,
}

impl<I: InputPayload> InputHistory<I> {
    /// Pre-allocates an empty history with storage for [`INPUT_HISTORY`] frames.
    #[must_use]
    pub fn new() -> Self {
        InputHistory {
            frames: VecDeque::with_capacity(INPUT_HISTORY),
            last_tick: None,
            snapshot_ack: PacketAck::NONE,
        }
    }

    /// Records a frame; ticks must strictly increase.
    ///
    /// When the buffer is full, the oldest frame is overwritten.
    ///
    /// # Errors
    /// Returns [`NetError::InputTickNotIncreasing`] when the tick does not advance.
    pub fn push(&mut self, frame: InputFrame<I>) -> Result<(), NetError> {
        if let Some(last) = self.last_tick
            && !frame.tick().is_after(last)
        {
            return Err(NetError::InputTickNotIncreasing {
                last,
                got: frame.tick(),
            });
        }
        if self.frames.len() == INPUT_HISTORY {
            self.frames.pop_front();
        }
        self.last_tick = Some(frame.tick());
        self.frames.push_back(frame);
        Ok(())
    }

    /// Sets the packet acknowledgement included in subsequent bundles.
    pub fn acknowledge_snapshots(&mut self, ack: PacketAck) {
        self.snapshot_ack = ack;
    }

    /// Builds a redundancy bundle from the tail of the history, if any.
    ///
    /// Stops at the first tick gap: only contiguous ticks ending at the newest are included.
    #[must_use]
    pub fn bundle(&self) -> Option<InputBundle<I>> {
        if self.frames.is_empty() {
            return None;
        }
        let mut out = InputFrames::new();
        let mut iter = self.frames.iter().rev();
        let newest = iter.next()?;
        out.push(newest.clone()).ok()?;
        let mut expected_older = newest.tick().raw().wrapping_sub(1);
        for frame in iter.take(INPUT_REDUNDANCY) {
            if frame.tick().raw() != expected_older {
                break;
            }
            out.push(frame.clone()).ok()?;
            expected_older = expected_older.wrapping_sub(1);
        }
        debug_assert!(frames_are_valid_redundancy(&out));
        Some(InputBundle::from_frames(out, self.snapshot_ack))
    }
}

impl<I: InputPayload> Default for InputHistory<I> {
    fn default() -> Self {
        Self::new()
    }
}

fn frames_are_valid_redundancy<I: InputPayload>(frames: &InputFrames<I>) -> bool {
    for index in 1..frames.len() {
        let Some(newer_frame) = frames.get(index - 1) else {
            return false;
        };
        let Some(older_frame) = frames.get(index) else {
            return false;
        };
        let newer = newer_frame.tick();
        let older = older_frame.tick();
        if !newer.is_after(older) {
            return false;
        }
        let expected = newer.raw().wrapping_sub(1);
        if older.raw() != expected {
            return false;
        }
    }
    true
}

/// Server-side filter that yields each tick at most once across redundant bundles.
#[derive(Debug, Clone, Default)]
pub struct InputReceiver {
    last_accepted: Option<Tick>,
}

/// Server-side last-value repeat policy and missing-input metric.
#[derive(Debug, Clone)]
pub struct InputRepeater<I> {
    last: Option<I>,
    missing_ticks: u64,
}

impl<I: Clone> InputRepeater<I> {
    /// Creates a repeater with no previous input.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: None,
            missing_ticks: 0,
        }
    }

    /// Uses `input` when present; otherwise repeats the prior value and increments the loss count.
    #[must_use]
    pub fn for_tick(&mut self, input: Option<I>) -> Option<I> {
        if let Some(input) = input {
            self.last = Some(input.clone());
            Some(input)
        } else {
            self.missing_ticks = self.missing_ticks.saturating_add(1);
            self.last.clone()
        }
    }

    /// Number of ticks that used a repeated input.
    #[must_use]
    pub const fn missing_ticks(&self) -> u64 {
        self.missing_ticks
    }
}

impl<I: Clone> Default for InputRepeater<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl InputReceiver {
    /// Creates an empty receiver (accepts every tick in the first bundle).
    #[must_use]
    pub fn new() -> Self {
        InputReceiver::default()
    }

    /// Yields frames strictly after the last accepted tick, ascending by tick.
    ///
    /// [`InputReceiver::last_accepted`] advances as each frame is yielded.
    #[must_use]
    pub fn accept<'a, I: InputPayload>(
        &'a mut self,
        bundle: &'a InputBundle<I>,
    ) -> AcceptIter<'a, I> {
        AcceptIter {
            receiver: self,
            bundle,
            scan: bundle.frames().len(),
        }
    }
}

/// Borrows frames from a bundle, oldest first, without allocating.
pub struct AcceptIter<'a, I: InputPayload> {
    receiver: &'a mut InputReceiver,
    bundle: &'a InputBundle<I>,
    /// Index into [`InputBundle::frames`] (newest-first); counts down toward zero.
    scan: usize,
}

impl<I: InputPayload> Iterator for AcceptIter<'_, I> {
    type Item = InputFrame<I>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.scan > 0 {
            self.scan -= 1;
            let frame = self.bundle.frames().get(self.scan)?;
            if self
                .receiver
                .last_accepted
                .is_some_and(|last| !frame.tick().is_after(last))
            {
                continue;
            }
            self.receiver.last_accepted = Some(frame.tick());
            return Some(frame.clone());
        }
        None
    }
}

/// Returns true when every payload in `bundle` validates.
pub(crate) fn validate_bundle<I: InputPayload>(bundle: &InputBundle<I>) -> Result<(), NetError> {
    for frame in bundle.frames().iter() {
        frame.payload().validate()?;
    }
    Ok(())
}

/// Encoded size of one payload (postcard) and enforces [`InputPayload::MAX_BYTES`].
pub(crate) fn encoded_payload_len<I: InputPayload>(payload: &I) -> Result<usize, NetError> {
    let mut scratch = [0; MAX_MESSAGE_BYTES];
    let len = postcard::to_slice(payload, &mut scratch)
        .map_err(NetError::Decode)?
        .len();
    if len > I::MAX_BYTES {
        return Err(NetError::PayloadTooLarge {
            size: len,
            cap: I::MAX_BYTES,
        });
    }
    Ok(len)
}
