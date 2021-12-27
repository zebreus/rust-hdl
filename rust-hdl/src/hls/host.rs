use crate::core::prelude::*;
use crate::hls::bidi::{BidiBusM, BidiMaster};
use crate::hls::bus::SoCBusController;
use crate::hls::controller::BaseController;
use crate::hls::cross_fifo::{CrossNarrow, CrossWiden};

// Creates a Host object that connects a bidirectional 8-bit
// bus to a Controller with the appropriate intermediate pieces.
#[derive(LogicBlock, Default)]
pub struct Host<const A: usize> {
    pub bidi_bus: BidiBusM<Bits<8>>,
    bidi_master: BidiMaster<Bits<8>>,
    bus_to_controller: CrossWiden<8, 4, 5, 16, 3, 4>,
    controller_to_bus: CrossNarrow<16, 3, 4, 8, 4, 5>,
    controller: BaseController<A>,
    pub bus: SoCBusController<16, A>,
    pub sys_clock: Signal<In, Clock>,
    pub bidi_clock: Signal<In, Clock>,
}

impl<const A: usize> Logic for Host<A> {
    #[hdl_gen]
    fn update(&mut self) {
        self.bidi_bus.link(&mut self.bidi_master.bus);
        self.bidi_master.clock.next = self.bidi_clock.val();
        self.bidi_master
            .data_from_bus
            .join(&mut self.bus_to_controller.narrow_bus);
        self.bus_to_controller.narrow_clock.next = self.bidi_clock.val();
        self.bus_to_controller.wide_clock.next = self.sys_clock.val();
        self.bidi_master
            .data_to_bus
            .join(&mut self.controller_to_bus.narrow_bus);
        self.controller_to_bus.narrow_clock.next = self.bidi_clock.val();
        self.controller_to_bus.wide_clock.next = self.sys_clock.val();
        self.controller
            .from_cpu
            .join(&mut self.bus_to_controller.wide_bus);
        self.controller
            .to_cpu
            .join(&mut self.controller_to_bus.wide_bus);
        self.controller.clock.next = self.sys_clock.val();
        self.bus.link(&mut self.controller.bus);
    }
}

#[test]
fn test_host_synthesizes() {
    let mut uut = Host::<8>::default();
    uut.sys_clock.connect();
    uut.bidi_clock.connect();
    uut.bidi_bus.link_connect_dest();
    uut.bus.ready.connect();
    uut.bus.to_controller.connect();
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("host", &vlog).unwrap();
}
