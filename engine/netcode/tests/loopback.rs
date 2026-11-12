#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    missing_docs
)]

use engine_core::Tick;
use engine_netcode::{Channel, LinkConditions, NetError, Transport, loopback_pair};

const PACKETS: usize = 10_000;
const LOSS_TOLERANCE_PERMILLE: i32 = 20;

#[test]
fn perfect_delivers_at_send_tick() {
    let (mut a, mut b) = loopback_pair(LinkConditions::PERFECT, LinkConditions::PERFECT, 1);
    a.advance_to(Tick::new(0));
    b.advance_to(Tick::new(0));
    a.send(Channel::Unreliable, b"hi").unwrap();
    b.advance_to(Tick::new(0));
    let mut out = Vec::new();
    assert_eq!(b.recv(&mut out).unwrap(), Some(Channel::Unreliable));
    assert_eq!(out, b"hi");
}

#[test]
fn latency_delays_by_n_ticks() {
    const LATENCY: u32 = 5;
    let conditions = LinkConditions::new(LATENCY, 0, 0, 0).unwrap();
    let (mut a, mut b) = loopback_pair(conditions, LinkConditions::PERFECT, 2);
    a.advance_to(Tick::new(0));
    a.send(Channel::Unreliable, &[7]).unwrap();
    let mut out = Vec::new();
    for t in 0..LATENCY {
        b.advance_to(Tick::new(t));
        assert_eq!(b.recv(&mut out).unwrap(), None);
    }
    b.advance_to(Tick::new(LATENCY));
    assert_eq!(b.recv(&mut out).unwrap(), Some(Channel::Unreliable));
    assert_eq!(out, [7]);
}

#[test]
fn reliable_lossless_and_ordered_under_loss() {
    let lossy = LinkConditions::new(0, 0, 500, 0).unwrap();
    let (mut a, mut b) = loopback_pair(lossy, LinkConditions::PERFECT, 3);
    let count = 200usize;
    a.advance_to(Tick::ZERO);
    for i in 0..count {
        a.send(Channel::Reliable, &[i as u8]).unwrap();
    }
    b.advance_to(Tick::new(1));
    let mut received = Vec::new();
    let mut out = Vec::new();
    while b.recv(&mut out).unwrap().is_some() {
        received.push(out[0]);
    }
    assert_eq!(received.len(), count);
    for (i, byte) in received.iter().enumerate() {
        assert_eq!(*byte, i as u8);
    }
}

#[test]
fn unreliable_loss_rate_within_tolerance() {
    let loss_per_mille = 300u16;
    let conditions = LinkConditions::new(0, 0, loss_per_mille, 0).unwrap();
    let (mut a, mut b) = loopback_pair(conditions, LinkConditions::PERFECT, 4);
    a.advance_to(Tick::ZERO);
    for _ in 0..PACKETS {
        a.send(Channel::Unreliable, &[1]).unwrap();
    }
    b.advance_to(Tick::new(1));
    let mut delivered = 0usize;
    let mut out = Vec::new();
    while b.recv(&mut out).unwrap().is_some() {
        delivered += 1;
    }
    let expected = (PACKETS as u64) * u64::from(loss_per_mille);
    let delivered_u64 = u64::try_from(delivered).expect("packet count fits in u64");
    let lost = (PACKETS as u64) * 1000 - delivered_u64 * 1000;
    let diff = (lost as i64 - expected as i64).abs() / 1000;
    assert!(
        diff <= (PACKETS as i64) * i64::from(LOSS_TOLERANCE_PERMILLE) / 1000,
        "delivered {delivered}, configured loss {loss_per_mille}‰"
    );
}

fn collect_unreliable(mut end: engine_netcode::LoopbackEnd, steps: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    for t in 0..steps {
        end.advance_to(Tick::new(t));
        let mut out = Vec::new();
        while end.recv(&mut out).unwrap().is_some() {
            bytes.push(out[0]);
        }
    }
    bytes
}

#[test]
fn same_seed_same_sequence() {
    let cond = LinkConditions::new(1, 2, 200, 100).unwrap();
    let run = |seed: u64| {
        let (mut a, b) = loopback_pair(cond, LinkConditions::PERFECT, seed);
        a.advance_to(Tick::ZERO);
        for i in 0..100u8 {
            let _ = a.send(Channel::Unreliable, &[i]);
        }
        collect_unreliable(b, 30)
    };
    assert_eq!(run(42), run(42));
}

#[test]
fn reliable_is_delivered_when_mixed_with_unreliable_traffic() {
    let cond = LinkConditions::new(2, 3, 500, 0).unwrap();
    let (mut a, mut b) = loopback_pair(cond, LinkConditions::PERFECT, 17);
    let ticks = 200u32;
    for t in 0..ticks {
        a.advance_to(Tick::new(t));
        let seq = u8::try_from(t % 256).expect("test sequence fits in u8");
        let _ = a.send(Channel::Unreliable, &[seq.wrapping_add(128)]);
        a.send(Channel::Reliable, &[seq]).unwrap();
    }
    let horizon = ticks + cond.latency_ticks + cond.jitter_ticks + 5;
    let mut received = Vec::new();
    let mut out = Vec::new();
    for t in 0..=horizon {
        b.advance_to(Tick::new(t));
        while let Some(Channel::Reliable) = b.recv(&mut out).unwrap() {
            received.push(out[0]);
        }
    }
    assert_eq!(received.len(), ticks as usize);
    for (i, byte) in received.iter().enumerate() {
        let expected = u8::try_from(i % 256).expect("test sequence fits in u8");
        assert_eq!(*byte, expected);
    }
}

#[test]
fn recv_reports_channel_closed_after_peer_dropped_and_pending_drained() {
    let cond = LinkConditions::new(2, 0, 0, 0).unwrap();
    let (mut a, mut b) = loopback_pair(cond, LinkConditions::PERFECT, 19);
    a.advance_to(Tick::ZERO);
    a.send(Channel::Unreliable, b"left").unwrap();
    drop(a);
    let mut out = Vec::new();
    b.advance_to(Tick::new(2));
    assert_eq!(b.recv(&mut out).unwrap(), Some(Channel::Unreliable));
    assert_eq!(out, b"left");
    assert_eq!(b.recv(&mut out), Err(NetError::ChannelClosed));
}

#[test]
fn unreliable_reorder_actually_reorders() {
    let cond = LinkConditions::new(3, 0, 0, 500).unwrap();
    let (mut a, mut b) = loopback_pair(cond, LinkConditions::PERFECT, 23);
    const TICKS: u32 = 1_000;
    for t in 0..TICKS {
        a.advance_to(Tick::new(t));
        a.send(Channel::Unreliable, &t.to_le_bytes()).unwrap();
    }
    let mut received = Vec::new();
    let mut out = Vec::new();
    for t in 0..(TICKS + cond.latency_ticks + 20) {
        b.advance_to(Tick::new(t));
        while b.recv(&mut out).unwrap().is_some() {
            let seq = u32::from_le_bytes(out.as_slice().try_into().expect("4-byte payload"));
            received.push(seq);
        }
    }
    assert_eq!(received.len(), TICKS as usize);
    let mut sorted = received.clone();
    sorted.sort_unstable();
    assert_ne!(
        received, sorted,
        "reorder should permute delivery order at least once"
    );
    let mut dedup = received.clone();
    dedup.sort_unstable();
    dedup.dedup();
    assert_eq!(dedup.len(), TICKS as usize);
}

#[test]
fn different_seed_different_sequence() {
    let cond = LinkConditions::new(1, 2, 200, 100).unwrap();
    let run = |seed: u64| {
        let (mut a, b) = loopback_pair(cond, LinkConditions::PERFECT, seed);
        a.advance_to(Tick::ZERO);
        for i in 0..100u8 {
            let _ = a.send(Channel::Unreliable, &[i]);
        }
        collect_unreliable(b, 30)
    };
    assert_ne!(run(1), run(2));
}
