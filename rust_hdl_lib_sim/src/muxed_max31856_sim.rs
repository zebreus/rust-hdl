use super::max31856_sim::MAX31856Simulator;
use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(LogicBlock)]
pub struct MuxedMAX31856Simulators {
    // Input SPI bus
    pub wires: SPIWiresSlave,
    pub mux: MuxSlaves<8, 3>,
    pub addr: Signal<In, Bits<3>>,
    pub clock: Signal<In, Clock>,
    adcs: Vec<MAX31856Simulator>,
}

impl MuxedMAX31856Simulators {
    pub fn new(config: SPIConfig) -> Self {
        Self {
            wires: Default::default(),
            mux: Default::default(),
            addr: Default::default(),
            clock: Default::default(),
            adcs: (0..8).map(|_| MAX31856Simulator::new(config)).collect(),
        }
    }
}

impl Logic for MuxedMAX31856Simulators {
    #[hdl_gen]
    fn update(&mut self) {
        SPIWiresSlave::link(&mut self.wires, &mut self.mux.from_master);
        self.mux.sel.next = self.addr.val();
        for i in 0..8 {
            self.adcs[i].clock.next = self.clock.val();
            SPIWiresMaster::join(&mut self.mux.to_slaves[i], &mut self.adcs[i].wires);
        }
    }
}

#[test]
fn test_mux_is_synthesizable() {
    use super::ad7193_sim::AD7193Config;
    let mut uut = MuxedMAX31856Simulators::new(AD7193Config::hw().spi);
    uut.connect_all();
    yosys_validate("mux_31865", &generate_verilog(&uut)).unwrap();
}
