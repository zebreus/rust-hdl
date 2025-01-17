use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;
use std::time::Duration;

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum AD7193State {
    Init,
    Ready,
    GettingCmd,
    ReadCmd,
    WaitSlaveIdle,
    WriteCmd,
    DoWrite,
    SingleConversion,
    SingleConversionCommit,
}

#[derive(LogicBlock)]
pub struct AD7193Simulator {
    // Slave SPI bus
    pub wires: SPIWiresSlave,
    pub clock: Signal<In, Clock>,
    // ROM that stores register widths
    reg_width_rom: ROM<Bits<5>, 3>,
    // RAM that stores register contents
    reg_ram: RAM<Bits<24>, 3>,
    // Used to time a single conversion
    oneshot: Shot<32>,
    // Separate bits out of a SPI message
    cmd: Signal<Local, Bits<8>>,
    reg_index: Signal<Local, Bits<3>>,
    rw_flag: Signal<Local, Bit>,
    // The spi slave device
    spi_slave: SPISlave<64>,
    // FSM state
    state: DFF<AD7193State>,
    reg_write_index: DFF<Bits<3>>,
    // Rolling counter to emulate conversions
    conversion_counter: DFF<Bits<24>>,
}

#[derive(Clone, Copy)]
pub struct AD7193Config {
    pub spi: SPIConfig,
    pub sample_time: Duration,
}

impl AD7193Config {
    pub fn hw() -> Self {
        Self {
            spi: SPIConfig {
                clock_speed: 48_000_000,
                cs_off: true,
                mosi_off: true,
                speed_hz: 400_000,
                cpha: true,
                cpol: true,
            },
            sample_time: Duration::from_micros(10100),
        }
    }
    pub fn sw() -> Self {
        Self {
            spi: SPIConfig {
                clock_speed: 1_000_000,
                cs_off: true,
                mosi_off: true,
                speed_hz: 10_000,
                cpha: true,
                cpol: true,
            },
            sample_time: Duration::from_micros(100),
        }
    }
}

pub const AD7193_REG_WIDTHS: [u32; 8] = [8, 24, 24, 24, 8, 8, 24, 24];
const AD7193_REG_INITS: [u64; 8] = [0x40, 0x80060, 0x117, 0x0, 0xa2, 0x0, 0x800000, 0x5544d0];

impl AD7193Simulator {
    pub fn new(config: AD7193Config) -> Self {
        assert!(config.spi.clock_speed > 10 * config.spi.speed_hz);
        let reg_width_rom = AD7193_REG_WIDTHS.iter().map(|x| x.to_bits()).into();
        let reg_ram = AD7193_REG_INITS.iter().map(|x| x.to_bits()).into();
        Self {
            wires: Default::default(),
            clock: Default::default(),
            reg_width_rom,
            reg_ram,
            oneshot: Shot::new(config.spi.clock_speed, config.sample_time),
            cmd: Default::default(),
            reg_index: Default::default(),
            rw_flag: Default::default(),
            spi_slave: SPISlave::new(config.spi),
            state: Default::default(),
            reg_write_index: Default::default(),
            conversion_counter: Default::default(),
        }
    }
}

impl Logic for AD7193Simulator {
    #[hdl_gen]
    fn update(&mut self) {
        // Connect the spi bus
        SPIWiresSlave::link(&mut self.wires, &mut self.spi_slave.wires);
        // Clock internal components
        self.reg_ram.read_clock.next = self.clock.val();
        self.reg_ram.write_clock.next = self.clock.val();
        clock!(self, clock, oneshot, spi_slave);
        dff_setup!(self, clock, state, reg_write_index, conversion_counter);
        // Set default values
        self.spi_slave.start_send.next = false;
        self.cmd.next = self.spi_slave.data_inbound.val().get_bits::<8>(0);
        self.reg_index.next = self.cmd.val().get_bits::<3>(3);
        self.rw_flag.next = self.cmd.val().get_bit(6);
        self.reg_width_rom.address.next = self.reg_index.val();
        self.reg_ram.read_address.next = self.reg_index.val();
        self.reg_ram.write_address.next = self.reg_index.val();
        self.spi_slave.continued_transaction.next = false;
        self.spi_slave.bits.next = 0.into();
        self.spi_slave.data_outbound.next = 0.into();
        self.reg_ram.write_enable.next = false;
        self.reg_ram.write_data.next = 0.into();
        self.spi_slave.disabled.next = false;
        self.oneshot.trigger.next = false;
        match self.state.q.val() {
            AD7193State::Init => {
                if self.spi_slave.transfer_done.val() {
                    self.state.d.next = AD7193State::Ready;
                }
            }
            AD7193State::Ready => {
                self.spi_slave.continued_transaction.next = true;
                self.spi_slave.bits.next = 8.into();
                self.spi_slave.data_outbound.next = 0xFF.into();
                self.spi_slave.start_send.next = true;
                self.state.d.next = AD7193State::GettingCmd;
            }
            AD7193State::GettingCmd => {
                if self.spi_slave.transfer_done.val() {
                    if self.rw_flag.val() {
                        self.state.d.next = AD7193State::ReadCmd;
                    } else {
                        self.reg_write_index.d.next = self.reg_index.val();
                        self.state.d.next = AD7193State::WriteCmd;
                    }
                }
            }
            AD7193State::ReadCmd => {
                self.spi_slave.continued_transaction.next = true;
                self.spi_slave.bits.next = bit_cast::<16, 5>(self.reg_width_rom.data.val()) + 8;
                self.spi_slave.data_outbound.next =
                    (bit_cast::<64, 24>(self.reg_ram.read_data.val()) << 8)
                        | Bits::<64>::from(0xBA);
                self.spi_slave.start_send.next = true;
                self.state.d.next = AD7193State::WaitSlaveIdle;
            }
            AD7193State::WriteCmd => {
                self.spi_slave.continued_transaction.next = true;
                self.spi_slave.bits.next = bit_cast::<16, 5>(self.reg_width_rom.data.val());
                self.spi_slave.data_outbound.next = 0xFFFF_FFFF_u64.to_bits();
                self.spi_slave.start_send.next = true;
                self.state.d.next = AD7193State::DoWrite;
            }
            AD7193State::DoWrite => {
                if self.spi_slave.transfer_done.val() {
                    self.reg_ram.write_data.next =
                        bit_cast::<24, 64>(self.spi_slave.data_inbound.val());
                    self.reg_ram.write_enable.next = true;
                    self.reg_ram.write_address.next = self.reg_write_index.q.val();
                    self.state.d.next = AD7193State::WaitSlaveIdle;
                    if (self.reg_write_index.q.val() == 1)
                        & self.spi_slave.data_inbound.val().get_bit(21)
                    {
                        self.state.d.next = AD7193State::SingleConversion;
                        self.oneshot.trigger.next = true;
                    }
                }
            }
            AD7193State::WaitSlaveIdle => {
                if !self.spi_slave.busy.val() {
                    self.state.d.next = AD7193State::Ready;
                }
            }
            AD7193State::SingleConversion => {
                self.spi_slave.disabled.next = true;
                if self.oneshot.fired.val() {
                    self.state.d.next = AD7193State::SingleConversionCommit;
                }
            }
            AD7193State::SingleConversionCommit => {
                self.reg_ram.write_address.next = 3.into();
                self.reg_ram.write_data.next = self.conversion_counter.q.val();
                self.reg_ram.write_enable.next = true;
                self.conversion_counter.d.next = self.conversion_counter.q.val() + 0x100;
                self.spi_slave.data_outbound.next = 0.into();
                self.state.d.next = AD7193State::Ready;
            }
            _ => {
                self.state.d.next = AD7193State::Init;
            }
        }
        if self.spi_slave.transfer_done.val() & self.spi_slave.data_inbound.val().all() {
            println!("Reset encountered");
            self.state.d.next = AD7193State::Ready;
        }
    }
}

#[test]
fn test_ad7193_synthesizes() {
    let mut uut = AD7193Simulator::new(AD7193Config::sw());
    uut.connect_all();
    yosys_validate("ad7193", &generate_verilog(&uut)).unwrap();
}

#[derive(LogicBlock)]
struct Test7193 {
    clock: Signal<In, Clock>,
    master: SPIMaster<64>,
    adc: AD7193Simulator,
}

impl Logic for Test7193 {
    #[hdl_gen]
    fn update(&mut self) {
        clock!(self, clock, master, adc);
        SPIWiresMaster::join(&mut self.master.wires, &mut self.adc.wires);
    }
}

impl Default for Test7193 {
    fn default() -> Self {
        Self {
            clock: Default::default(),
            master: SPIMaster::new(AD7193Config::sw().spi),
            adc: AD7193Simulator::new(AD7193Config::sw()),
        }
    }
}

#[cfg(test)]
fn reg_read(
    reg_index: u32,
    x: Box<Test7193>,
    sim: &mut Sim<Test7193>,
) -> Result<(Bits<64>, Box<Test7193>), SimError> {
    let cmd = (((1 << 6) | (reg_index << 3)) << 24).into();
    let result = do_spi_txn(32, cmd, false, x, sim)?;
    let width = AD7193_REG_WIDTHS[reg_index as usize];
    let reg_val = if width == 8 {
        (result.0 >> 16) & 0xFF
    } else {
        result.0 & 0xFFFFFF
    };
    Ok((reg_val, result.1))
}

#[cfg(test)]
fn reg_write(
    reg_index: u32,
    reg_value: u64,
    x: Box<Test7193>,
    sim: &mut Sim<Test7193>,
) -> Result<Box<Test7193>, SimError> {
    let mut cmd = (((0 << 6) | (reg_index << 3)) << 24).into();
    if AD7193_REG_WIDTHS[reg_index as usize] == 8 {
        cmd = cmd | reg_value << 16;
    } else {
        cmd = cmd | reg_value;
    }
    let ret = do_spi_txn(32, cmd, false, x, sim)?;
    Ok(ret.1)
}

#[cfg(test)]
fn do_spi_txn(
    bits: u16,
    value: u64,
    continued: bool,
    mut x: Box<Test7193>,
    sim: &mut Sim<Test7193>,
) -> Result<(Bits<64>, Box<Test7193>), SimError> {
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
fn mk_test7193() -> Test7193 {
    let mut uut = Test7193::default();
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
    let uut = mk_test7193();
    yosys_validate("7193_1", &generate_verilog(&uut)).unwrap();
}

#[test]
fn test_reg_reads() {
    let uut = mk_test7193();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test7193>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test7193>| {
        let mut x = sim.init()?;
        // Wait for reset to complete
        wait_clock_cycles!(sim, clock, x, 20);
        // Do the first read to initialize the chip
        let result = do_spi_txn(32, 0xFFFFFFFF, false, x, &mut sim)?;
        x = result.1;
        for ndx in 0..8 {
            println!("Reading register index {}", ndx);
            let result = reg_read(ndx, x, &mut sim)?;
            x = result.1;
            println!("Value {} -> {:x}", ndx, result.0);
            sim_assert!(
                sim,
                result.0 == Bits::<64>::from(AD7193_REG_INITS[ndx as usize]),
                x
            );
            wait_clock_true!(sim, clock, x);
        }
        sim.done(x)
    });
    sim.run(Box::new(uut), 1_000_000).unwrap();
}

#[test]
fn test_reg_writes() {
    let uut = mk_test7193();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test7193>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test7193>| {
        let mut x = sim.init()?;

        // Wait for reset to complete
        wait_clock_cycles!(sim, clock, x, 20);
        // Initialize the chip...
        let result = do_spi_txn(32, 0xFFFFFFFF, false, x, &mut sim)?;
        x = result.1;
        for ndx in 0..8 {
            let result = reg_read(ndx, x, &mut sim)?;
            x = result.1;
            sim_assert!(
                sim,
                result.0 == Bits::<64>::from(AD7193_REG_INITS[ndx as usize]),
                x
            );
            x = reg_write(ndx, AD7193_REG_INITS[ndx as usize] + 1, x, &mut sim)?;
            let result = reg_read(ndx, x, &mut sim)?;
            x = result.1;
            sim_assert!(
                sim,
                result.0 == Bits::<64>::from(AD7193_REG_INITS[ndx as usize] + 1),
                x
            );
        }
        sim.done(x)
    });
    sim.run(Box::new(uut), 1_000_000).unwrap();
}

#[test]
fn test_single_conversion() {
    let uut = mk_test7193();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<Test7193>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<Test7193>| {
        let mut x = sim.init()?;

        // Wait for reset to complete
        wait_clock_cycles!(sim, clock, x, 20);
        // Initialize the chip...
        let result = do_spi_txn(32, 0xFFFFFFFF, false, x, &mut sim)?;
        x = result.1;
        for n in 0..3 {
            wait_clock_cycle!(sim, clock, x, 100);
            let result = do_spi_txn(32, 0x8382006, true, x, &mut sim)?;
            x = result.1;
            wait_clock_cycle!(sim, clock, x, 100);
            sim_assert!(sim, x.master.wires.miso.val(), x);
            x = sim.watch(|x| !x.master.wires.miso.val(), x)?;
            wait_clock_cycle!(sim, clock, x, 100);
            let result = reg_read(3, x, &mut sim)?;
            println!("Conversion {} -> {:x}", n, result.0);
            x = result.1;
            sim_assert!(sim, result.0 == Bits::<64>::from(n * 0x100), x);
            println!("Conversion {} completed", n);
        }
        sim.done(x)
    });
    sim.run(Box::new(uut), 10_000_000).unwrap();
}
