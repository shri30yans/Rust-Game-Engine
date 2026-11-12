//! The one place the simulation thread and the render thread meet. It is a
//! bounded hand-off of whole [`RenderFrame`]s, not shared simulation state, so a
//! `Mutex` and a `Condvar` are used here and nowhere else in the simulation path.
//!
//! Exactly two frames exist. The simulation acquires a free frame, fills it and
//! publishes it; the renderer takes the oldest published frame, draws it and
//! recycles it. Because a frame must come back before it can be reused, the
//! simulation is never more than one frame ahead of the renderer. Frames move
//! between the two ends and are never cloned, and no call allocates.
//!
//! Invariants: a frame is held by exactly one of {free list, ready queue, sim end,
//! render end}; a dropped end closes the exchange for the other side; a poisoned
//! lock is an error, never unwrapped.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use crate::error::AppError;
use crate::extract::RenderFrame;

/// Number of frames in flight: one being drawn and one being simulated.
const FRAMES_IN_FLIGHT: usize = 2;

/// State behind the lock.
struct State {
    free: Vec<RenderFrame>,
    ready: VecDeque<RenderFrame>,
    sim_alive: bool,
    render_alive: bool,
}

/// Shared by both ends. The one condvar wakes whichever side is waiting.
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    /// Locks the state, mapping a poisoned lock to an error.
    fn lock(&self) -> Result<MutexGuard<'_, State>, AppError> {
        self.state.lock().map_err(|_| AppError::ExchangePoisoned)
    }
}

/// Creates the two frames and the two ends of the exchange.
///
/// Both frames start in the free list, so the simulation can acquire two frames
/// back to back. A single thread can therefore run acquire, fill, publish, next,
/// draw, recycle in turn without ever blocking.
#[must_use]
pub fn frame_exchange(capacity_per_frame: usize) -> (SimEnd, RenderEnd) {
    let mut free = Vec::with_capacity(FRAMES_IN_FLIGHT);
    for _ in 0..FRAMES_IN_FLIGHT {
        free.push(RenderFrame::with_capacity(capacity_per_frame));
    }
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            free,
            ready: VecDeque::with_capacity(FRAMES_IN_FLIGHT),
            sim_alive: true,
            render_alive: true,
        }),
        changed: Condvar::new(),
    });
    (
        SimEnd {
            shared: Arc::clone(&shared),
        },
        RenderEnd { shared },
    )
}

/// The simulation thread's end of the exchange.
pub struct SimEnd {
    shared: Arc<Shared>,
}

impl SimEnd {
    /// Waits for a free frame and takes it.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExchangeClosed`] when the render end was dropped before a frame
    /// came free, and [`AppError::ExchangePoisoned`] when a lock was poisoned.
    pub fn acquire(&mut self) -> Result<RenderFrame, AppError> {
        let mut state = self.shared.lock()?;
        loop {
            if let Some(frame) = state.free.pop() {
                return Ok(frame);
            }
            if !state.render_alive {
                return Err(AppError::ExchangeClosed);
            }
            state = self
                .shared
                .changed
                .wait(state)
                .map_err(|_| AppError::ExchangePoisoned)?;
        }
    }

    /// Hands a filled frame to the renderer.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExchangeClosed`] when the render end was dropped, and
    /// [`AppError::ExchangePoisoned`] when a lock was poisoned.
    pub fn publish(&mut self, frame: RenderFrame) -> Result<(), AppError> {
        let mut state = self.shared.lock()?;
        if !state.render_alive {
            return Err(AppError::ExchangeClosed);
        }
        state.ready.push_back(frame);
        self.shared.changed.notify_all();
        Ok(())
    }
}

impl Drop for SimEnd {
    fn drop(&mut self) {
        // A poisoned lock cannot be cleared here, but every later call on either end
        // reports `ExchangePoisoned`, so no waiter can use the stale flag to proceed.
        if let Ok(mut state) = self.shared.state.lock() {
            state.sim_alive = false;
        }
        self.shared.changed.notify_all();
    }
}

/// The render thread's end of the exchange.
pub struct RenderEnd {
    shared: Arc<Shared>,
}

impl RenderEnd {
    /// Waits for the oldest published frame and takes it. Frames published before the
    /// simulation end was dropped are still delivered.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExchangeClosed`] when the simulation end is gone and nothing is
    /// left to deliver, and [`AppError::ExchangePoisoned`] when a lock was poisoned.
    pub fn next_frame(&mut self) -> Result<RenderFrame, AppError> {
        let mut state = self.shared.lock()?;
        loop {
            if let Some(frame) = state.ready.pop_front() {
                return Ok(frame);
            }
            if !state.sim_alive {
                return Err(AppError::ExchangeClosed);
            }
            state = self
                .shared
                .changed
                .wait(state)
                .map_err(|_| AppError::ExchangePoisoned)?;
        }
    }

    /// Returns a drawn frame so the simulation can reuse it.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ExchangeClosed`] when the simulation end was dropped, and
    /// [`AppError::ExchangePoisoned`] when a lock was poisoned.
    pub fn recycle(&mut self, frame: RenderFrame) -> Result<(), AppError> {
        let mut state = self.shared.lock()?;
        if !state.sim_alive {
            return Err(AppError::ExchangeClosed);
        }
        state.free.push(frame);
        self.shared.changed.notify_all();
        Ok(())
    }
}

impl Drop for RenderEnd {
    fn drop(&mut self) {
        // Same reasoning as `SimEnd::drop`: a poisoned lock makes every later call fail.
        if let Ok(mut state) = self.shared.state.lock() {
            state.render_alive = false;
        }
        self.shared.changed.notify_all();
    }
}
