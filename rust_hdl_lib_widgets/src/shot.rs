use rust_hdl_lib_core::prelude::*;
use std::time::Duration;

use crate::{dff::DFF, dff_setup};

#[derive(Clone, Debug, LogicBlock)]
pub struct Shot<const N: usize> {
    pub trigger: Signal<In, Bit>,
    pub active: Signal<Out, Bit>,
    pub clock: Signal<In, Clock>,
    pub fired: Signal<Out, Bit>,
    duration: Constant<Bits<N>>,
    counter: DFF<Bits<N>>,
    state: DFF<Bit>,
}

impl<const N: usize> Shot<N> {
    pub fn new(frequency: u64, duration: Duration) -> Self {
        let duration_nanos = duration.as_nanos() as f64 * NANOS_PER_FEMTO; // duration in femtos
        let clock_period_nanos = freq_hz_to_period_femto(frequency as f64);
        let clocks = (duration_nanos / clock_period_nanos).floor() as u64;
        assert!(clocks < (1_u64 << N));
        Self {
            trigger: Signal::default(),
            active: Signal::new_with_default(false),
            clock: Signal::default(),
            fired: Default::default(),
            duration: Constant::new(clocks.into()),
            counter: Default::default(),
            state: Default::default(),
        }
    }
}

impl<const N: usize> Logic for Shot<N> {
    #[hdl_gen]
    fn update(&mut self) {
        dff_setup!(self, clock, counter, state);
        if self.state.q.val() {
            self.counter.d.next = self.counter.q.val() + 1;
        }
        self.fired.next = false;
        if self.state.q.val() && (self.counter.q.val() == self.duration.val()) {
            self.state.d.next = false;
            self.fired.next = true;
        }
        self.active.next = self.state.q.val();
        if self.trigger.val() {
            self.state.d.next = true;
            self.counter.d.next = 0.into();
        }
    }
}
