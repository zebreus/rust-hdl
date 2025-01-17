use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum ADS868XState {
    Ready,
    Waiting,
    Dispatch,
    ReadWordCmd,
    ReadByteCmd,
    WriteWordCmd,
    WriteMSBCmd,
    WriteLSBCmd,
    WriteDone,
    Nop,
}

#[derive(LogicBlock)]
pub struct ADS868XSimulator {
    pub wires: SPIWiresSlave,
    pub clock: Signal<In, Clock>,
    // RAM to store register values
    reg_ram: RAM<Bits<16>, 5>,
    // SPI slave device
    spi_slave: SPISlave<32>,
    // FSM State
    state: DFF<ADS868XState>,
    // Rolling counter to emulate conversions
    conversion_counter: DFF<Bits<16>>,
    // Inbound register
    inbound: DFF<Bits<32>>,
    // Local signal to store the command bits
    read_cmd: Signal<Local, Bits<5>>,
    write_cmd: Signal<Local, Bits<7>>,
    address: Signal<Local, Bits<9>>,
    data_parity: Signal<Local, Bit>,
    id_parity: Signal<Local, Bit>,
}

impl ADS868XSimulator {
    pub fn spi_hw() -> SPIConfig {
        SPIConfig {
            clock_speed: 48_000_000,
            cs_off: true,
            mosi_off: true,
            speed_hz: 400_000,
            cpha: false,
            cpol: false,
        }
    }
    pub fn spi_sw() -> SPIConfig {
        SPIConfig {
            clock_speed: 1_000_000,
            cs_off: true,
            mosi_off: true,
            speed_hz: 10_000,
            cpha: false,
            cpol: false,
        }
    }

    pub fn new(spi_config: SPIConfig) -> Self {
        assert!(spi_config.clock_speed > 10 * spi_config.speed_hz);
        Self {
            wires: Default::default(),
            clock: Default::default(),
            reg_ram: Default::default(),
            spi_slave: SPISlave::new(spi_config),
            state: Default::default(),
            conversion_counter: Default::default(),
            inbound: Default::default(),
            read_cmd: Default::default(),
            write_cmd: Default::default(),
            address: Default::default(),
            data_parity: Default::default(),
            id_parity: Default::default(),
        }
    }
}

#[test]
fn test_indexing() {
    let val: Bits<32> = 0b11000_00_101_001_100_00000000_00000000.into();
    assert_eq!(val.get_bits::<5>(27).index(), 0b11000);
    assert_eq!(val.get_bits::<9>(16).index(), 0b101_001_100);
}

impl Logic for ADS868XSimulator {
    #[hdl_gen]
    fn update(&mut self) {
        // Connect the spi bus
        SPIWiresSlave::link(&mut self.wires, &mut self.spi_slave.wires);
        // Clock internal components
        self.reg_ram.read_clock.next = self.clock.val();
        self.reg_ram.write_clock.next = self.clock.val();
        clock!(self, clock, spi_slave);
        dff_setup!(self, clock, state, conversion_counter, inbound);
        // Set default values
        self.spi_slave.start_send.next = false;
        self.spi_slave.continued_transaction.next = false;
        self.spi_slave.bits.next = 0.into();
        self.spi_slave.data_outbound.next = 0.into();
        self.reg_ram.write_enable.next = false;
        self.reg_ram.write_data.next = 0.into();
        self.spi_slave.disabled.next = false;
        self.read_cmd.next = self.inbound.q.val().get_bits::<5>(27);
        self.write_cmd.next = self.inbound.q.val().get_bits::<7>(25);
        self.address.next = self.inbound.q.val().get_bits::<9>(16);
        self.reg_ram.write_address.next = bit_cast::<5, 9>(self.address.val() >> 1);
        self.reg_ram.read_address.next = 0.into();
        self.data_parity.next = self.conversion_counter.q.val().xor();
        self.id_parity.next = (self.reg_ram.read_data.val() & 0x0FF).xor();
        match self.state.q.val() {
            ADS868XState::Ready => {
                if !self.spi_slave.busy.val() {
                    self.state.d.next = ADS868XState::Nop;
                }
            }
            ADS868XState::Waiting => {
                if self.spi_slave.transfer_done.val() {
                    self.inbound.d.next = self.spi_slave.data_inbound.val();
                    self.state.d.next = ADS868XState::Dispatch;
                }
            }
            ADS868XState::Dispatch => {
                if self.read_cmd.val() == 0b11001 {
                    self.state.d.next = ADS868XState::ReadWordCmd;
                    self.reg_ram.read_address.next = bit_cast::<5, 9>(self.address.val() >> 1);
                } else if self.read_cmd.val() == 0b01001 {
                    self.state.d.next = ADS868XState::ReadByteCmd;
                    self.reg_ram.read_address.next = bit_cast::<5, 9>(self.address.val() >> 1);
                } else if self.write_cmd.val() == 0b11010_00 {
                    self.state.d.next = ADS868XState::WriteWordCmd;
                } else if self.write_cmd.val() == 0b11010_01 {
                    self.state.d.next = ADS868XState::WriteMSBCmd;
                    self.reg_ram.read_address.next = bit_cast::<5, 9>(self.address.val() >> 1);
                } else if self.write_cmd.val() == 0b11010_10 {
                    self.state.d.next = ADS868XState::WriteLSBCmd;
                    self.reg_ram.read_address.next = bit_cast::<5, 9>(self.address.val() >> 1);
                } else {
                    self.reg_ram.read_address.next = 0x02.into();
                    self.state.d.next = ADS868XState::Nop;
                }
            }
            ADS868XState::ReadWordCmd => {
                self.spi_slave.data_outbound.next =
                    bit_cast::<32, 16>(self.reg_ram.read_data.val());
                self.spi_slave.bits.next = 16.into();
                self.spi_slave.start_send.next = true;
                self.state.d.next = ADS868XState::Waiting;
            }
            ADS868XState::ReadByteCmd => {
                if self.address.val().get_bit(0) {
                    self.spi_slave.data_outbound.next =
                        bit_cast::<32, 16>(self.reg_ram.read_data.val() >> 8);
                } else {
                    self.spi_slave.data_outbound.next =
                        bit_cast::<32, 16>(self.reg_ram.read_data.val() & 0xFF);
                }
                self.spi_slave.bits.next = 8.into();
                self.spi_slave.start_send.next = true;
                self.state.d.next = ADS868XState::Waiting;
            }
            ADS868XState::WriteWordCmd => {
                self.reg_ram.write_data.next = bit_cast::<16, 32>(self.inbound.q.val() & 0xFFFF);
                self.reg_ram.write_enable.next = true;
                self.state.d.next = ADS868XState::WriteDone;
            }
            ADS868XState::WriteLSBCmd => {
                self.reg_ram.write_data.next = bit_cast::<16, 32>(self.inbound.q.val() & 0x00FF)
                    | (self.reg_ram.read_data.val() & 0xFF00);
                self.reg_ram.write_enable.next = true;
                self.state.d.next = ADS868XState::WriteDone;
            }
            ADS868XState::WriteMSBCmd => {
                self.reg_ram.write_data.next = bit_cast::<16, 32>(self.inbound.q.val() & 0xFF00)
                    | (self.reg_ram.read_data.val() & 0x00FF);
                self.reg_ram.write_enable.next = true;
                self.state.d.next = ADS868XState::WriteDone;
            }
            ADS868XState::WriteDone => {
                self.spi_slave.bits.next = 32.into();
                self.spi_slave.data_outbound.next = self.inbound.q.val();
                self.spi_slave.start_send.next = true;
                self.state.d.next = ADS868XState::Waiting;
            }
            ADS868XState::Nop => {
                self.spi_slave.bits.next = 32.into();
                // TODO - make this more accurate based on how
                // the output register is programmed.
                /*  self.spi_slave.data_outbound.next =
                (bit_cast::<32, 16>(self.conversion_counter.q.val()) << 16)
                    | bit_cast::<32, 16>(self.reg_ram.read_data.val() & 0x0FF) << 12
                    | bit_cast::<32, 1>(self.data_parity.val().into()) << 11
                    | bit_cast::<32, 1>((self.data_parity.val() ^ self.id_parity.val()).into())
                    << 10;
                    */
                self.spi_slave.data_outbound.next =
                    (bit_cast::<32, 16>(self.conversion_counter.q.val()) << 16)
                        | (bit_cast::<32, 16>(self.reg_ram.read_data.val() & 0x0FF) << 12)
                        | (bit_cast::<32, 1>(self.data_parity.val().into()) << 8)
                        | (bit_cast::<32, 1>(
                            (self.data_parity.val() ^ self.id_parity.val()).into(),
                        ) << 9);
                self.spi_slave.start_send.next = true;
                self.state.d.next = ADS868XState::Waiting;
                self.conversion_counter.d.next = self.conversion_counter.q.val() + 1;
            }
            _ => {
                self.state.d.next = ADS868XState::Ready;
            }
        }
    }
}

#[test]
fn test_ads8689_synthesizes() {
    let mut uut = ADS868XSimulator::new(ADS868XSimulator::spi_sw());
    uut.connect_all();
    yosys_validate("ads8689", &generate_verilog(&uut)).unwrap();
}

#[derive(LogicBlock)]
struct Test8689 {
    clock: Signal<In, Clock>,
    master: SPIMaster<32>,
    adc: ADS868XSimulator,
}

impl Logic for Test8689 {
    #[hdl_gen]
    fn update(&mut self) {
        clock!(self, clock, master, adc);
        SPIWiresMaster::join(&mut self.master.wires, &mut self.adc.wires);
    }
}

impl Default for Test8689 {
    fn default() -> Self {
        Self {
            clock: Default::default(),
            master: SPIMaster::new(ADS868XSimulator::spi_sw()),
            adc: ADS868XSimulator::new(ADS868XSimulator::spi_sw()),
        }
    }
}

#[cfg(test)]
fn do_spi_txn(
    bits: u16,
    value: u64,
    continued: bool,
    mut x: Box<Test8689>,
    sim: &mut Sim<Test8689>,
) -> Result<(Bits<32>, Box<Test8689>), SimError> {
    wait_clock_true!(sim, clock, x);
    x.master.data_outbound.next = value.to_bits();
    x.master.bits_outbound.next = bits.to_bits();
    x.master.continued_transaction.next = continued;
    x.master.start_send.next = true;
    wait_clock_cycle!(sim, clock, x);
    x.master.start_send.next = false;
    x = sim
        .watch(|x| x.master.transfer_done.val().into(), x)
        .unwrap();
    let ret = x.master.data_inbound.val();
    for _ in 0..50 {
        wait_clock_cycle!(sim, clock, x);
    }
    Ok((ret, x))
}

#[cfg(test)]
fn mk_test8689() -> Test8689 {
    let mut uut = Test8689::default();
    uut.clock.connect();
    uut.master.continued_transaction.connect();
    uut.master.start_send.connect();
    uut.master.data_outbound.connect();
    uut.master.bits_outbound.connect();
    uut.connect_all();
    uut
}

#[test]
fn test_yosys_validate_test_fixture() {
    let uut = mk_test8689();
    yosys_validate("8689_1", &generate_verilog(&uut)).unwrap();
}

#[test]
fn test_reg_writes() {
    let uut = mk_test8689();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test8689>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test8689>| {
        let mut x = sim.init()?;

        wait_clock_cycles!(sim, clock, x, 50);
        wait_clock_true!(sim, clock, x);
        wait_clock_cycle!(sim, clock, x);
        // Write an ID to register 2...
        let result = do_spi_txn(32, 0xd0_02_00_02, false, x, &mut sim)?;
        x = result.1;
        wait_clock_cycle!(sim, clock, x);
        wait_clock_cycle!(sim, clock, x);
        let result = do_spi_txn(32, 0x48_02_00_00, false, x, &mut sim)?;
        x = result.1;
        let result = do_spi_txn(8, 0x00, false, x, &mut sim)?;
        println!("ID Register read {:x}", result.0);
        x = result.1;
        sim_assert_eq!(sim, result.0.index(), 2, x);
        /*
        # Output should be 0x40 0x08
        [ 0xd0 0x10 0x40 0x08 ] % [ 0xc8 0x10 0x00 0x00 ] % { 0x00 0x00 ]
         */
        wait_clock_cycle!(sim, clock, x);
        let result = do_spi_txn(32, 0xd0_10_40_08, false, x, &mut sim)?;
        x = result.1;
        wait_clock_cycle!(sim, clock, x);
        let result = do_spi_txn(32, 0xc8_10_00_00, false, x, &mut sim)?;
        x = result.1;
        wait_clock_cycle!(sim, clock, x);
        let result = do_spi_txn(16, 0x00, false, x, &mut sim)?;
        x = result.1;
        sim_assert_eq!(sim, result.0.index(), 0x40_08, x);
        for i in 0..5 {
            wait_clock_cycle!(sim, clock, x);
            let result = do_spi_txn(32, 0x00_00_00_00, false, x, &mut sim)?;
            x = result.1;
            println!("Reading is {:x}", result.0);
            sim_assert_eq!(sim, (result.0 & 0xFFFF0000), ((i + 2) << 16), x);
            let parity_bit = result.0 & 0x100 != 0;
            let data: Bits<32> = (result.0 & 0xFFFF0000) >> 16;
            sim_assert_eq!(sim, data.xor(), parity_bit, x);
        }
        sim.done(x)
    });
    //    sim.run(Box::new(uut), 1_000_000).unwrap();
    sim.run_to_file(Box::new(uut), 1_000_000, &vcd_path!("ad868x.vcd"))
        .unwrap();
}

#[test]
fn test_parity_calculations() {
    for sample in [
        0x00020C00,
        0x92ab1400_u32,
        0x734b1800,
        0x4fc81400,
        0x7bee1400,
        0x94821800_u32,
        0x5eb31400,
        0x4eaa1400,
        0x8ac91800_u32,
        0x95321800_u32,
        0x54c01800,
        0x561a1800,
        0x91601800_u32,
        0x7e401800,
        0x50961400,
    ] {
        let mut data = (sample & 0xFFFF_0000_u32) >> 16;
        let mut parity = false;
        for _ in 0..16 {
            parity = parity ^ (data & 0x1 != 0);
            data = data >> 1;
        }
        let adc_flag = (sample & 0x800) != 0;
        assert_eq!(adc_flag, parity);
    }
}
