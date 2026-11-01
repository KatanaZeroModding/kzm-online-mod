//! Both clients simulate tick N only when both inputs for N are here.
//! There is no prediction: a guessed slash would desync every enemy.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub buttons: u32,
    pub aim_x: i32,
    pub aim_y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checksum {
    pub tick: u32,
    pub count: u32,
    pub sum_x: i64,
    pub sum_y: i64,
    pub sum_hp: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimTick {
    pub tick: u32,
    pub local: Input,
    pub remote: Input,
}

pub struct Lockstep {
    delay: u32,
    next: u32,
    live: u32,
    local: BTreeMap<u32, Input>,
    remote: BTreeMap<u32, Input>,
}

impl Lockstep {
    pub fn new() -> Self {
        Self::with_delay(3)
    }

    fn with_delay(delay: u32) -> Self {
        let delay = delay.clamp(2, 8);
        let mut step = Self {
            delay,
            next: 0,
            live: 0,
            local: BTreeMap::new(),
            remote: BTreeMap::new(),
        };
        for tick in 0..delay {
            step.local.insert(tick, Input::default());
        }
        step
    }

    pub fn delay(&self) -> u32 {
        self.delay
    }

    /// One tick is about 16 ms. Ignored once the room has started, so the queues stay aligned.
    pub fn set_delay_from_rtt(&mut self, rtt_ms: u32) {
        if self.next == 0 && self.live == 0 {
            *self = Self::with_delay(rtt_ms / 16);
        }
    }

    pub fn next_tick(&self) -> u32 {
        self.next
    }

    /// The returned tick is the one this sample will be simulated on.
    pub fn push_local(&mut self, input: Input) -> u32 {
        let tick = self.delay + self.live;
        self.live += 1;
        self.local.insert(tick, input);
        tick
    }

    pub fn push_remote(&mut self, tick: u32, input: Input) {
        self.remote.insert(tick, input);
    }

    pub fn ready(&self) -> Option<SimTick> {
        let tick = self.next;
        Some(SimTick {
            tick,
            local: *self.local.get(&tick)?,
            remote: *self.remote.get(&tick)?,
        })
    }

    pub fn advance(&mut self) {
        let tick = self.next;
        self.local.remove(&tick);
        self.remote.remove(&tick);
        self.next += 1;
    }

    pub fn reset(&mut self) {
        *self = Self::with_delay(self.delay);
    }
}

impl Default for Lockstep {
    fn default() -> Self {
        Self::new()
    }
}

pub fn diverged(local: Checksum, remote: Checksum) -> bool {
    local.tick == remote.tick && local != remote
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(buttons: u32) -> Input {
        Input { buttons, aim_x: 0, aim_y: 0 }
    }

    #[test]
    fn tick_waits_until_both_inputs_exist() {
        let mut step = Lockstep::new();
        assert!(step.ready().is_none());
        step.push_remote(0, input(2));
        let ready = step.ready().expect("tick 0");
        assert_eq!(ready.tick, 0);
        assert_eq!(ready.remote.buttons, 2);
        assert_eq!(ready.local.buttons, 0);
        step.advance();
        assert!(step.ready().is_none());
        assert_eq!(step.push_local(input(4)), 3);
    }

    #[test]
    fn rtt_moves_the_delay_and_checksums_compare_the_same_tick() {
        let mut step = Lockstep::new();
        step.set_delay_from_rtt(80);
        assert_eq!(step.delay(), 5);
        step.set_delay_from_rtt(1);
        assert_eq!(step.delay(), 2);
        let same = Checksum { tick: 8, count: 1, sum_x: 1, sum_y: 1, sum_hp: 1 };
        assert!(!diverged(same, same));
        let other = Checksum { sum_hp: 0, ..same };
        assert!(diverged(same, other));
        assert!(!diverged(same, Checksum { tick: 9, ..other }));
    }
}
