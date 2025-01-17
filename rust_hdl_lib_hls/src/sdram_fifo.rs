use crate::bus::{FIFOReadResponder, FIFOWriteResponder};
use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(LogicBlock)]
pub struct SDRAMFIFO<const R: usize, const C: usize, const P: u32, const D: usize, const A: usize> {
    pub clock: Signal<In, Clock>,
    pub sdram: SDRAMDriver<D>,
    pub ram_clock: Signal<In, Clock>,
    pub bus_write: FIFOWriteResponder<Bits<D>>,
    pub bus_read: FIFOReadResponder<Bits<D>>,
    controller: SDRAMFIFOController<R, C, P, D, A>,
}

impl<const R: usize, const C: usize, const P: u32, const D: usize, const A: usize> Logic
    for SDRAMFIFO<R, C, P, D, A>
{
    #[hdl_gen]
    fn update(&mut self) {
        self.controller.data_in.next = self.bus_write.data.val();
        self.controller.write.next = self.bus_write.write.val();
        self.bus_write.full.next = self.controller.full.val();
        self.bus_write.almost_full.next = self.controller.full.val();
        self.bus_read.data.next = self.controller.data_out.val();
        self.bus_read.empty.next = self.controller.empty.val();
        self.bus_read.almost_empty.next = self.controller.empty.val();
        self.controller.read.next = self.bus_read.read.val();
        clock!(self, clock, controller);
        self.controller.ram_clock.next = self.ram_clock.val();
        SDRAMDriver::<D>::link(&mut self.sdram, &mut self.controller.sdram);
    }
}

impl<const R: usize, const C: usize, const P: u32, const D: usize, const A: usize>
    SDRAMFIFO<R, C, P, D, A>
{
    pub fn new(
        cas_delay: u32,
        timings: MemoryTimings,
        buffer: OutputBuffer,
    ) -> SDRAMFIFO<R, C, P, D, A> {
        Self {
            clock: Default::default(),
            sdram: Default::default(),
            ram_clock: Default::default(),
            bus_write: Default::default(),
            bus_read: Default::default(),
            controller: SDRAMFIFOController::new(cas_delay, timings, buffer),
        }
    }
}

#[test]
fn test_sdram_fifo_synthesizes() {
    let mut uut = SDRAMFIFO::<6, 4, 4, 16, 12>::new(
        3,
        MemoryTimings::fast_boot_sim(125e6),
        OutputBuffer::Wired,
    );
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("sdram_fifo_hls", &vlog).unwrap();
}
