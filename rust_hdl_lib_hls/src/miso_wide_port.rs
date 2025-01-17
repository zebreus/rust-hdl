use crate::bus::SoCPortResponder;
use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(LogicBlock)]
pub struct MISOWidePort<const W: usize, const D: usize> {
    pub bus: SoCPortResponder<D>,
    pub port_in: Signal<In, Bits<W>>,
    pub strobe_in: Signal<In, Bit>,
    pub clock_out: Signal<Out, Clock>,
    accum: DFF<Bits<W>>,
    address_active: DFF<Bit>,
    offset: Constant<Bits<16>>,
    shift: Constant<Bits<16>>,
    modulo: Constant<Bits<8>>,
    count: DFF<Bits<8>>,
    ready: DFF<Bit>,
}

impl<const W: usize, const D: usize> Default for MISOWidePort<W, D> {
    fn default() -> Self {
        assert!(W > D);
        assert_eq!(W % D, 0);
        assert!(W / D < 256);
        assert!(W < 65536);
        Self {
            bus: Default::default(),
            port_in: Default::default(),
            strobe_in: Default::default(),
            clock_out: Default::default(),
            accum: Default::default(),
            address_active: Default::default(),
            offset: Constant::new(D.to_bits()),
            shift: Constant::new((W - D).to_bits()),
            modulo: Constant::new((W / D).to_bits()),
            count: Default::default(),
            ready: Default::default(),
        }
    }
}

impl<const W: usize, const D: usize> Logic for MISOWidePort<W, D> {
    #[hdl_gen]
    fn update(&mut self) {
        self.clock_out.next = self.bus.clock.val();
        dff_setup!(self, clock_out, accum, address_active, count, ready);
        // Latch prevention
        self.address_active.d.next = self.bus.select.val();
        self.bus.ready.next = false;
        // On the strobe in, load the new value into our accumulator
        if self.strobe_in.val() {
            self.accum.d.next = self.port_in.val();
            self.count.d.next = self.modulo.val();
        }
        self.bus.to_controller.next = 0.into();
        self.ready.d.next = self.count.q.val().any() & self.address_active.q.val();
        if self.address_active.q.val() {
            self.bus.to_controller.next =
                self.accum.q.val().get_bits::<D>(self.shift.val().index());
            self.bus.ready.next = self.ready.q.val() & self.count.q.val().any();
            if self.bus.strobe.val() {
                self.accum.d.next = self.accum.q.val() << bit_cast::<W, 16>(self.offset.val());
                self.count.d.next = self.count.q.val() - 1;
            }
        }
    }
}

#[test]
fn test_local_in_wide_port_is_synthesizable() {
    let mut dev = MISOWidePort::<64, 16>::default();
    dev.connect_all();
    let vlog = generate_verilog(&dev);
    yosys_validate("local_wide_in", &vlog).unwrap();
}
