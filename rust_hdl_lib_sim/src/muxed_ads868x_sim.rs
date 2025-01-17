use super::ads868x_sim::ADS868XSimulator;
use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(LogicBlock)]
pub struct MuxedADS868XSimulators<const N: usize> {
    // Input SPI bus
    pub wires: SPIWiresSlave,
    pub addr: Signal<In, Bits<3>>,
    pub mux: MuxSlaves<N, 3>,
    pub clock: Signal<In, Clock>,
    adcs: [ADS868XSimulator; N],
}

impl<const N: usize> MuxedADS868XSimulators<N> {
    pub fn new(config: SPIConfig) -> Self {
        assert!(N <= 8);
        Self {
            wires: Default::default(),
            mux: Default::default(),
            addr: Default::default(),
            clock: Default::default(),
            adcs: array_init::array_init(|_| ADS868XSimulator::new(config)),
        }
    }
}

impl<const N: usize> Logic for MuxedADS868XSimulators<N> {
    #[hdl_gen]
    fn update(&mut self) {
        SPIWiresSlave::link(&mut self.wires, &mut self.mux.from_master);
        for i in 0..N {
            self.adcs[i].clock.next = self.clock.val();
            SPIWiresMaster::join(&mut self.mux.to_slaves[i], &mut self.adcs[i].wires);
        }
        self.mux.sel.next = self.addr.val();
    }
}

#[test]
fn test_mux_is_synthesizable() {
    let mut uut: MuxedADS868XSimulators<8> =
        MuxedADS868XSimulators::new(ADS868XSimulator::spi_hw());
    uut.connect_all();
    yosys_validate("mux_8689", &generate_verilog(&uut)).unwrap();
}
