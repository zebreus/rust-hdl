use super::ad7193_sim::AD7193Config;
use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum MAX31856State {
    Start,
    Ready,
    GettingCmd,
    RegFetchRead,
    ReadCmd,
    WaitReadComplete,
    WriteCmd,
    DoWrite,
}

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum DAQState {
    Idle,
    Convert,
    Copy0,
    Copy1,
}

#[derive(LogicBlock)]
pub struct MAX31856Simulator {
    // Slave SPI bus
    pub wires: SPIWiresSlave,
    pub clock: Signal<In, Clock>,
    // RAM that stores the memory contents
    reg_ram: RAM<Bits<8>, 4>,
    // Used to handle auto conversions
    auto_conversions_enabled: DFF<Bit>,
    auto_conversion_strobe: Strobe<32>,
    auto_conversion_counter: DFF<Bits<19>>,
    // Separate bits out of the SPI message
    cmd: Signal<Local, Bits<8>>,
    rw_flag: Signal<Local, Bit>,
    reg_index: Signal<Local, Bits<4>>,
    // The SPI slave device
    spi_slave: SPISlave<64>,
    // FSM state:
    state: DFF<MAX31856State>,
    reg_read_index: DFF<Bits<4>>,
    reg_write_index: DFF<Bits<4>>,
    // Boot timer
    boot: DFF<Bits<4>>,
    // DAQ state:
    dstate: DFF<DAQState>,
}

const MAX31856_REG_INITS: [u8; 16] = [
    0x00, 0x03, 0xFF, 0x7F, 0xC0, 0x7F, 0xFF, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

impl MAX31856Simulator {
    pub fn new(config: SPIConfig) -> Self {
        let reg_ram = MAX31856_REG_INITS.iter().map(|x| x.to_bits()).into();
        Self {
            wires: Default::default(),
            clock: Default::default(),
            reg_ram,
            auto_conversions_enabled: Default::default(),
            auto_conversion_strobe: Strobe::new(config.clock_speed, 100.0),
            auto_conversion_counter: Default::default(),
            cmd: Default::default(),
            rw_flag: Default::default(),
            spi_slave: SPISlave::new(config),
            state: Default::default(),
            reg_read_index: Default::default(),
            reg_write_index: Default::default(),
            boot: DFF::default(),
            reg_index: Default::default(),
            dstate: Default::default(),
        }
    }
}

impl Logic for MAX31856Simulator {
    #[hdl_gen]
    fn update(&mut self) {
        // Connect the spi bus
        SPIWiresSlave::link(&mut self.wires, &mut self.spi_slave.wires);
        // Clock the internal logic
        self.reg_ram.write_clock.next = self.clock.val();
        self.reg_ram.read_clock.next = self.clock.val();
        // Setup the DFF and internal widgets
        dff_setup!(
            self,
            clock,
            auto_conversions_enabled,
            auto_conversion_counter,
            state,
            reg_read_index,
            reg_write_index,
            boot,
            dstate
        );
        clock!(self, clock, auto_conversion_strobe, spi_slave);
        // Set default values
        self.spi_slave.start_send.next = false;
        self.spi_slave.continued_transaction.next = false;
        self.spi_slave.bits.next = 0.into();
        self.spi_slave.data_outbound.next = 0.into();
        self.reg_ram.write_enable.next = false;
        self.spi_slave.disabled.next = false;
        self.cmd.next = self.spi_slave.data_inbound.val().get_bits::<8>(0);
        self.reg_index.next = self.cmd.val().get_bits::<4>(0);
        self.rw_flag.next = self.cmd.val().get_bit(7);
        self.reg_ram.read_address.next = self.reg_read_index.q.val();
        self.reg_ram.write_address.next = self.reg_write_index.q.val();
        self.reg_ram.write_data.next = self.spi_slave.data_inbound.val().get_bits::<8>(0);
        self.auto_conversion_strobe.enable.next = self.auto_conversions_enabled.q.val();
        match self.state.q.val() {
            MAX31856State::Start => {
                self.boot.d.next = self.boot.q.val() + 1;
                if self.boot.q.val().all() {
                    self.state.d.next = MAX31856State::Ready
                }
            }
            MAX31856State::Ready => {
                self.spi_slave.continued_transaction.next = true;
                self.spi_slave.bits.next = 8.into();
                self.spi_slave.data_outbound.next = 0xFF.into();
                self.spi_slave.start_send.next = true;
                self.state.d.next = MAX31856State::GettingCmd;
            }
            MAX31856State::GettingCmd => {
                if self.spi_slave.transfer_done.val() {
                    if !self.rw_flag.val() {
                        self.reg_read_index.d.next = self.reg_index.val();
                        self.state.d.next = MAX31856State::RegFetchRead;
                    } else {
                        self.reg_write_index.d.next = self.reg_index.val();
                        self.state.d.next = MAX31856State::WriteCmd;
                    }
                }
            }
            MAX31856State::RegFetchRead => {
                self.state.d.next = MAX31856State::ReadCmd;
            }
            MAX31856State::ReadCmd => {
                self.spi_slave.continued_transaction.next = true;
                self.spi_slave.bits.next = 8.into();
                self.spi_slave.data_outbound.next = bit_cast::<64, 8>(self.reg_ram.read_data.val());
                self.spi_slave.start_send.next = true;
                self.state.d.next = MAX31856State::WaitReadComplete;
            }
            MAX31856State::WaitReadComplete => {
                if !self.spi_slave.busy.val() & self.spi_slave.transfer_done.val() {
                    self.state.d.next = MAX31856State::Ready;
                }
                if self.spi_slave.busy.val() & self.spi_slave.transfer_done.val() {
                    self.reg_read_index.d.next = self.reg_read_index.q.val() + 1;
                    self.state.d.next = MAX31856State::RegFetchRead;
                }
            }
            MAX31856State::WriteCmd => {
                self.spi_slave.continued_transaction.next = true;
                self.spi_slave.bits.next = 8.into();
                self.spi_slave.data_outbound.next = 0xFF.into();
                self.spi_slave.start_send.next = true;
                self.state.d.next = MAX31856State::DoWrite;
            }
            MAX31856State::DoWrite => {
                if !self.spi_slave.busy.val() & self.spi_slave.transfer_done.val() {
                    if !self.reg_write_index.q.val().any() {
                        self.auto_conversions_enabled.d.next =
                            self.spi_slave.data_inbound.val().get_bit(7);
                    }
                    self.reg_ram.write_enable.next = true;
                    self.state.d.next = MAX31856State::Ready;
                }
                if self.spi_slave.busy.val() & self.spi_slave.transfer_done.val() {
                    self.reg_ram.write_enable.next = true;
                    self.reg_write_index.d.next = self.reg_write_index.q.val() + 1;
                    self.state.d.next = MAX31856State::WriteCmd;
                }
            }
            _ => {
                self.state.d.next = MAX31856State::Start;
            }
        }
        // Warning! There is a contention between writes from the SPI bus and
        // writes from the DAQ...  A more sophisticated model would segment
        // the register ram into 2 blocks, and limit SPI writes to the lower block.
        match self.dstate.q.val() {
            DAQState::Idle => {
                if self.auto_conversion_strobe.strobe.val() {
                    self.auto_conversion_counter.d.next = self.auto_conversion_counter.q.val() + 1;
                    self.dstate.d.next = DAQState::Convert;
                }
            }
            DAQState::Convert => {
                self.reg_ram.write_address.next = 0x0E.into();
                self.reg_ram.write_data.next =
                    bit_cast::<8, 3>(self.auto_conversion_counter.q.val().get_bits::<3>(0)) << 5;
                self.reg_ram.write_enable.next = true;
                self.dstate.d.next = DAQState::Copy0;
            }
            DAQState::Copy0 => {
                self.reg_ram.write_address.next = 0x0D.into();
                self.reg_ram.write_data.next =
                    self.auto_conversion_counter.q.val().get_bits::<8>(3);
                self.reg_ram.write_enable.next = true;
                self.dstate.d.next = DAQState::Copy1;
            }
            DAQState::Copy1 => {
                self.reg_ram.write_address.next = 0x0C.into();
                self.reg_ram.write_data.next =
                    self.auto_conversion_counter.q.val().get_bits::<8>(11);
                self.reg_ram.write_enable.next = true;
                self.dstate.d.next = DAQState::Idle;
            }
            _ => {
                self.dstate.d.next = DAQState::Idle;
            }
        }
    }
}

#[test]
fn test_max31856_synthesizes() {
    let mut uut = MAX31856Simulator::new(SPIConfig {
        clock_speed: 1_000_000,
        cs_off: true,
        mosi_off: true,
        speed_hz: 10_000,
        cpha: true,
        cpol: true,
    });
    uut.connect_all();
    yosys_validate("max31856", &generate_verilog(&uut)).unwrap();
}

#[derive(LogicBlock)]
struct Test31856 {
    clock: Signal<In, Clock>,
    master: SPIMaster<64>,
    uut: MAX31856Simulator,
}

impl Logic for Test31856 {
    #[hdl_gen]
    fn update(&mut self) {
        clock!(self, clock, master, uut);
        SPIWiresMaster::join(&mut self.master.wires, &mut self.uut.wires);
    }
}

impl Default for Test31856 {
    fn default() -> Self {
        Self {
            clock: Default::default(),
            master: SPIMaster::new(AD7193Config::sw().spi),
            uut: MAX31856Simulator::new(AD7193Config::sw().spi),
        }
    }
}

#[cfg(test)]
fn reg_read(
    reg_index: u32,
    x: Box<Test31856>,
    sim: &mut Sim<Test31856>,
) -> Result<(Bits<64>, Box<Test31856>), SimError> {
    let cmd = (reg_index << 8) as u64;
    let result = do_spi_txn(16, cmd.into(), false, x, sim)?;
    let reg_val = result.0 & 0xFF;
    Ok((reg_val, result.1))
}

#[cfg(test)]
fn reg_write(
    reg_index: u32,
    reg_value: u64,
    x: Box<Test31856>,
    sim: &mut Sim<Test31856>,
) -> Result<Box<Test31856>, SimError> {
    let mut cmd = (((1 << 7) | reg_index) << 8) as u64;
    cmd = cmd | (reg_value & 0xFF);
    let ret = do_spi_txn(16, cmd.into(), false, x, sim)?;
    Ok(ret.1)
}

#[cfg(test)]
fn do_spi_txn(
    bits: u16,
    value: u64,
    continued: bool,
    mut x: Box<Test31856>,
    sim: &mut Sim<Test31856>,
) -> Result<(Bits<64>, Box<Test31856>), SimError> {
    wait_clock_true!(sim, clock, x);
    wait_clock_cycles!(sim, clock, x, 10);
    x.master.data_outbound.next = value.to_bits();
    x.master.bits_outbound.next = bits.to_bits();
    x.master.continued_transaction.next = continued;
    x.master.start_send.next = true;
    wait_clock_cycle!(sim, clock, x);
    x.master.start_send.next = false;
    x = sim.watch(
        |x| x.clock.val().clk && x.master.transfer_done.val().into(),
        x,
    )?;
    let ret = x.master.data_inbound.val();
    wait_clock_true!(sim, clock, x);
    wait_clock_cycles!(sim, clock, x, 50);
    Ok((ret, x))
}

#[cfg(test)]
fn mk_test31856() -> Test31856 {
    let mut uut = Test31856::default();
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
    let uut = mk_test31856();
    yosys_validate("31856_1", &generate_verilog(&uut)).unwrap();
}

#[test]
fn test_multireg_reads() {
    let uut = mk_test31856();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test31856>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test31856>| {
        let mut x = sim.init()?;

        wait_clock_true!(sim, clock, x);
        wait_clock_cycles!(sim, clock, x, 20);
        let cmd = 1 << 32;
        let result = do_spi_txn(40, cmd, false, x, &mut sim)?;
        x = result.1;
        sim_assert_eq!(
            sim,
            result.0 & 0xFF_FF_FF_FF,
            Bits::<64>::from(0x03_FF_7F_C0),
            x
        );
        sim.done(x)
    });
    sim.run(Box::new(uut), 100_000).unwrap();
}

#[test]
fn test_multireg_write() {
    let uut = mk_test31856();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test31856>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test31856>| {
        let mut x = sim.init()?;

        wait_clock_true!(sim, clock, x);
        wait_clock_cycles!(sim, clock, x, 20);
        let cmd = 0x81 << 32 | 0xDEADBEEF;
        println!("CMD = {:x}", cmd);
        let result = do_spi_txn(40, cmd, false, x, &mut sim)?;
        x = result.1;
        let cmd = 0x1 << 32;
        let result = do_spi_txn(40, cmd, false, x, &mut sim)?;
        x = result.1;
        sim_assert_eq!(
            sim,
            result.0 & 0xFF_FF_FF_FF,
            0xDEADBEEF_u32.to_bits::<64>(),
            x
        );
        sim.done(x)
    });
    sim.run(Box::new(uut), 100_000).unwrap();
}

#[test]
fn test_reg_reads() {
    let uut = mk_test31856();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test31856>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test31856>| {
        let mut x = sim.init()?;

        wait_clock_true!(sim, clock, x);
        wait_clock_cycles!(sim, clock, x, 20);
        for ndx in 0..16 {
            println!("Reading register index {}", ndx);
            let result = reg_read(ndx, x, &mut sim)?;
            x = result.1;
            println!("Value {} -> {:x}", ndx, result.0);
            sim_assert_eq!(
                sim,
                result.0,
                MAX31856_REG_INITS[ndx as usize].to_bits::<64>(),
                x
            );
            wait_clock_true!(sim, clock, x);
        }
        sim.done(x)
    });
    //    sim.run_traced(Box::new(uut), 1_000_000, std::fs::File::create("max3.vcd").unwrap()).unwrap();
    sim.run(Box::new(uut), 1_000_000).unwrap();
}

#[test]
fn test_reg_writes() {
    use std::num::Wrapping;
    let uut = mk_test31856();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test31856>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test31856>| {
        let mut x = sim.init()?;

        // Initialize the chip...
        wait_clock_true!(sim, clock, x);
        wait_clock_cycles!(sim, clock, x, 20);
        for ndx in 0..16 {
            let result = reg_read(ndx, x, &mut sim)?;
            x = result.1;
            sim_assert_eq!(
                sim,
                result.0,
                MAX31856_REG_INITS[ndx as usize].to_bits::<64>(),
                x
            );
            println!("Read of register {} -> {:x}", ndx, result.0);
            x = reg_write(
                ndx,
                (MAX31856_REG_INITS[ndx as usize] as u64 + 1) as u64,
                x,
                &mut sim,
            )?;
            let result = reg_read(ndx, x, &mut sim)?;
            x = result.1;
            sim_assert_eq!(
                sim,
                result.0,
                (Wrapping(MAX31856_REG_INITS[ndx as usize]) + Wrapping(1))
                    .0
                    .to_bits::<64>(),
                x
            );
            println!("Re-read of register {} -> {:x}", ndx, result.0);
        }
        sim.done(x)
    });
    sim.run(Box::new(uut), 1_000_000).unwrap();
}

#[test]
fn test_single_conversion() {
    let uut = mk_test31856();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test31856>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test31856>| {
        let mut x = sim.init()?;

        wait_clock_true!(sim, clock, x);
        wait_clock_cycles!(sim, clock, x, 50);
        x = reg_write(0, 0x80, x, &mut sim)?;
        x = sim.wait(200_000, x)?;
        let result = reg_read(0x0E, x, &mut sim)?;
        x = result.1;
        sim_assert_eq!(sim, result.0 & 0xFF, 0x40, x);
        let cmd = 0xC << 24;
        let result = do_spi_txn(32, cmd, false, x, &mut sim)?;
        x = result.1;
        sim_assert_eq!(sim, result.0 & 0xFFFFFF, 0x40, x);
        sim.done(x)
    });
    //    sim.run(Box::new(uut), 1_000_000).unwrap();
    sim.run_to_file(Box::new(uut), 1_000_000, "/tmp/mread.vcd")
        .unwrap();
}
