//! Deterministic in-process transport with configurable latency, jitter, loss and reorder.

use std::cmp::Ordering;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use engine_core::{SimRng, Tick};

use crate::error::NetError;
use crate::protocol::Channel;
use crate::transport::Transport;

/// Per-mille scale (1000 = 100 %).
const PER_MILLE: u16 = 1000;

/// Seeds [`SimRng`] for the first link direction in [`loopback_pair`].
const DIRECTION_A_TO_B: u64 = 0;
/// Seeds [`SimRng`] for the second link direction in [`loopback_pair`].
const DIRECTION_B_TO_A: u64 = 1;

/// Simulated link impairments between two loopback ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkConditions {
    /// Base one-way delay in simulation ticks.
    pub latency_ticks: u32,
    /// Extra delay drawn uniformly from `0..=jitter_ticks` on each send.
    pub jitter_ticks: u32,
    /// Packet loss probability for unreliable traffic, in per-mille.
    pub loss_per_mille: u16,
    /// Reorder probability for unreliable traffic, in per-mille.
    pub reorder_per_mille: u16,
}

impl LinkConditions {
    /// Zero delay, zero loss, zero reorder.
    pub const PERFECT: LinkConditions = LinkConditions {
        latency_ticks: 0,
        jitter_ticks: 0,
        loss_per_mille: 0,
        reorder_per_mille: 0,
    };

    /// Builds validated conditions.
    ///
    /// # Errors
    /// Returns [`NetError::InvalidLinkConditions`] when probabilities exceed 100 %.
    pub const fn new(
        latency_ticks: u32,
        jitter_ticks: u32,
        loss_per_mille: u16,
        reorder_per_mille: u16,
    ) -> Result<Self, NetError> {
        if loss_per_mille > PER_MILLE || reorder_per_mille > PER_MILLE {
            return Err(NetError::InvalidLinkConditions(
                "loss and reorder must be at most 1000 per mille",
            ));
        }
        Ok(LinkConditions {
            latency_ticks,
            jitter_ticks,
            loss_per_mille,
            reorder_per_mille,
        })
    }
}

#[derive(Debug, Clone)]
struct QueuedPacket {
    deliver_at: Tick,
    /// Total send order for unstable tie-breaking on `(deliver_at, send_seq)`.
    send_seq: u64,
    /// Present only for reliable datagrams; drives in-order reliable delivery.
    reliable_seq: Option<u64>,
    channel: Channel,
    bytes: Vec<u8>,
}

/// One end of a [`loopback_pair`] link.
pub struct LoopbackEnd {
    outbound: Sender<QueuedPacket>,
    inbound: Receiver<QueuedPacket>,
    conditions: LinkConditions,
    rng: SimRng,
    now: Tick,
    send_seq: u64,
    reliable_seq: u64,
    pending: Vec<QueuedPacket>,
    next_reliable_seq: u64,
    inbound_closed: bool,
}

impl LoopbackEnd {
    /// Sets the simulation clock used to release delayed packets.
    pub fn advance_to(&mut self, now: Tick) {
        self.now = now;
        self.drain_inbound();
    }

    fn drain_inbound(&mut self) {
        loop {
            match self.inbound.try_recv() {
                Ok(packet) => self.pending.push(packet),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.inbound_closed = true;
                    break;
                }
            }
        }
        self.pending.sort_by(|a, b| {
            let by_tick = tick_cmp(a.deliver_at, b.deliver_at);
            by_tick.then_with(|| a.send_seq.cmp(&b.send_seq))
        });
    }

    fn roll_per_mille(&mut self, per_mille: u16) -> bool {
        if per_mille == 0 {
            return false;
        }
        let threshold = u32::from(per_mille);
        self.rng.next_u32() % u32::from(PER_MILLE) < threshold
    }

    fn jitter_ticks(&mut self) -> u32 {
        let cap = self.conditions.jitter_ticks;
        if cap == 0 {
            return 0;
        }
        let bound = cap.saturating_add(1);
        self.rng.next_u32() % bound
    }

    /// Base one-way delay, plus seeded jitter.
    ///
    /// Unreliable packets selected for reorder get extra delay in `1..=spread` where
    /// `spread = max(2, jitter_ticks)` so a later send can pass an earlier one with the
    /// same base latency (when `jitter_ticks` is zero, spread is still at least two ticks).
    fn compute_deliver_at(&mut self, channel: Channel) -> Tick {
        let mut delay = self
            .conditions
            .latency_ticks
            .saturating_add(self.jitter_ticks());
        if channel == Channel::Unreliable && self.roll_per_mille(self.conditions.reorder_per_mille)
        {
            let spread = self.conditions.jitter_ticks.max(2);
            let extra = 1 + self.rng.next_u32() % spread;
            delay = delay.saturating_add(extra);
        }
        self.now.advance(delay)
    }
}

impl Transport for LoopbackEnd {
    fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        if channel == Channel::Unreliable && self.roll_per_mille(self.conditions.loss_per_mille) {
            return Ok(());
        }
        let send_seq = self.send_seq;
        self.send_seq = self.send_seq.saturating_add(1);
        let reliable_seq = if channel == Channel::Reliable {
            let seq = self.reliable_seq;
            self.reliable_seq = self.reliable_seq.saturating_add(1);
            Some(seq)
        } else {
            None
        };
        let deliver_at = self.compute_deliver_at(channel);
        let packet = QueuedPacket {
            deliver_at,
            send_seq,
            reliable_seq,
            channel,
            bytes: bytes.to_vec(),
        };
        self.outbound
            .send(packet)
            .map_err(|_| NetError::ChannelClosed)
    }

    fn recv(&mut self, out: &mut Vec<u8>) -> Result<Option<Channel>, NetError> {
        self.drain_inbound();
        let index = self.pending.iter().position(|packet| {
            if !is_ready(packet.deliver_at, self.now) {
                return false;
            }
            match packet.channel {
                Channel::Reliable => packet.reliable_seq == Some(self.next_reliable_seq),
                Channel::Unreliable => true,
            }
        });
        if let Some(idx) = index {
            let packet = self.pending.remove(idx);
            if packet.channel == Channel::Reliable {
                self.next_reliable_seq = self.next_reliable_seq.saturating_add(1);
            }
            out.clear();
            out.extend_from_slice(&packet.bytes);
            return Ok(Some(packet.channel));
        }
        out.clear();
        if self.inbound_closed {
            return Err(NetError::ChannelClosed);
        }
        Ok(None)
    }
}

/// Returns `true` when `deliver_at` is at or before `now` on the tick circle.
fn is_ready(deliver_at: Tick, now: Tick) -> bool {
    deliver_at.raw() == now.raw() || deliver_at.is_before(now)
}

fn tick_cmp(a: Tick, b: Tick) -> Ordering {
    if a.raw() == b.raw() {
        Ordering::Equal
    } else if a.is_before(b) {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

/// Builds two connected loopback ends with independent outbound link models.
#[must_use]
pub fn loopback_pair(
    a_to_b: LinkConditions,
    b_to_a: LinkConditions,
    seed: u64,
) -> (LoopbackEnd, LoopbackEnd) {
    let (tx_a, rx_b) = mpsc::channel();
    let (tx_b, rx_a) = mpsc::channel();
    let end_a = LoopbackEnd {
        outbound: tx_a,
        inbound: rx_a,
        conditions: a_to_b,
        rng: SimRng::new(seed, DIRECTION_A_TO_B),
        now: Tick::ZERO,
        send_seq: 0,
        reliable_seq: 0,
        pending: Vec::new(),
        next_reliable_seq: 0,
        inbound_closed: false,
    };
    let end_b = LoopbackEnd {
        outbound: tx_b,
        inbound: rx_b,
        conditions: b_to_a,
        rng: SimRng::new(seed, DIRECTION_B_TO_A),
        now: Tick::ZERO,
        send_seq: 0,
        reliable_seq: 0,
        pending: Vec::new(),
        next_reliable_seq: 0,
        inbound_closed: false,
    };
    (end_a, end_b)
}
