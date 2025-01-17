use crate::sdr_sdram::bank::MemoryBank;
use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::{
    prelude::*,
    sdram::{cmd::SDRAMCommandDecoder, SDRAMDevice},
};

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum MasterState {
    Boot,
    WaitPrecharge,
    Precharge,
    WaitAutorefresh,
    LoadModeRegister,
    Ready,
    Error,
}

// Clock enable, and DQM are ignored.
#[derive(LogicBlock)]
pub struct SDRAMSimulator<
    const R: usize, // Number of rows
    const C: usize, // Number of columns
    const A: usize, // A = R + C
    const D: usize, // Bits per word
> {
    pub sdram: SDRAMDevice<D>,
    pub test_error: Signal<Out, Bit>,
    pub test_ready: Signal<Out, Bit>,
    decode: SDRAMCommandDecoder,
    clock: Signal<Local, Clock>,
    cmd: Signal<Local, SDRAMCommand>,
    state: DFF<MasterState>,
    counter: DFF<Bits<32>>,
    auto_refresh_init_counter: DFF<Bits<32>>,
    write_burst_mode: DFF<Bit>,
    cas_latency: DFF<Bits<3>>,
    burst_type: DFF<Bit>,
    burst_len: DFF<Bits<3>>,
    op_mode: DFF<Bits<2>>,
    banks: [MemoryBank<R, C, A, D>; 4],
    // Timings
    // Number of clocks to delay for boot initialization
    boot_delay: Constant<Bits<32>>,
    t_rp: Constant<Bits<32>>,
    load_mode_timing: Constant<Bits<32>>,
    t_rrd: Constant<Bits<32>>,
    banks_busy: Signal<Local, Bit>,
}

impl<const R: usize, const C: usize, const A: usize, const D: usize> Logic
    for SDRAMSimulator<R, C, A, D>
{
    #[hdl_gen]
    fn update(&mut self) {
        // Clock logic
        self.clock.next = self.sdram.clk.val();
        dff_setup!(
            self,
            clock,
            state,
            counter,
            auto_refresh_init_counter,
            write_burst_mode,
            cas_latency,
            burst_type,
            burst_len,
            op_mode
        );
        // Connect the command decoder to the bus
        self.decode.we_not.next = self.sdram.we_not.val();
        self.decode.cas_not.next = self.sdram.cas_not.val();
        self.decode.ras_not.next = self.sdram.ras_not.val();
        self.decode.cs_not.next = self.sdram.cs_not.val();
        self.cmd.next = self.decode.cmd.val();
        self.test_error.next = false;
        self.test_ready.next = false;
        // Connect up the banks to the I/O buffer
        self.sdram.read_data.next = 0.into();
        for i in 0..4 {
            self.banks[i].clock.next = self.clock.val();
            if self.sdram.write_enable.val() {
                self.banks[i].write_data.next = self.sdram.write_data.val();
            } else {
                self.banks[i].write_data.next = 0.into();
            }
            if self.banks[i].read_valid.val() {
                self.sdram.read_data.next = self.banks[i].read_data.val();
            }
            self.banks[i].address.next = self.sdram.address.val();
            self.banks[i].cmd.next = self.cmd.val();
            self.banks[i].write_burst.next = self.write_burst_mode.q.val();
            self.banks[i].burst_len.next = 1.into();
            match self.burst_len.q.val().index() {
                0 => self.banks[i].burst_len.next = 1.into(),
                1 => self.banks[i].burst_len.next = 2.into(),
                2 => self.banks[i].burst_len.next = 4.into(),
                3 => self.banks[i].burst_len.next = 8.into(),
                _ => self.state.d.next = MasterState::Error,
            }
            self.banks[i].cas_delay.next = 2.into();
            match self.cas_latency.q.val().index() {
                0 => self.banks[i].cas_delay.next = 0.into(),
                2 => self.banks[i].cas_delay.next = 2.into(),
                3 => self.banks[i].cas_delay.next = 3.into(),
                _ => self.state.d.next = MasterState::Error,
            }
            if self.sdram.bank.val().index() == i {
                self.banks[i].select.next = true;
            } else {
                self.banks[i].select.next = false;
            }
            if self.cmd.val() == SDRAMCommand::AutoRefresh {
                self.banks[i].select.next = true;
            }
            if (self.cmd.val() == SDRAMCommand::Precharge) & self.sdram.address.val().get_bit(10) {
                self.banks[i].select.next = true;
            }
        }
        self.banks_busy.next = self.banks[0].busy.val()
            | self.banks[1].busy.val()
            | self.banks[2].busy.val()
            | self.banks[3].busy.val();
        match self.state.q.val() {
            MasterState::Boot => {
                if (self.cmd.val() != SDRAMCommand::NOP) & (self.counter.q.val().any()) {
                    // self.state.d.next = MasterState::Error;
                    // Although the spec says you should not do this, it is unavoidable with
                    // the Lattice FPGA.
                }
                self.counter.d.next = self.counter.q.val() + 1;
                if self.counter.q.val() == self.boot_delay.val() {
                    self.state.d.next = MasterState::WaitPrecharge;
                }
            }
            MasterState::WaitPrecharge => {
                match self.cmd.val() {
                    SDRAMCommand::NOP => {}
                    SDRAMCommand::Precharge => {
                        // make sure the ALL bit is set
                        if self.sdram.address.val().get_bit(10) != true {
                            self.state.d.next = MasterState::Error;
                        } else {
                            self.counter.d.next = 0.into();
                            self.state.d.next = MasterState::Precharge;
                        }
                    }
                    _ => {
                        self.state.d.next = MasterState::Error;
                    }
                }
            }
            MasterState::Precharge => {
                self.counter.d.next = self.counter.q.val() + 1;
                if self.counter.q.val() == self.t_rp.val() {
                    self.state.d.next = MasterState::WaitAutorefresh;
                }
                if self.cmd.val() != SDRAMCommand::NOP {
                    self.state.d.next = MasterState::Error;
                }
            }
            MasterState::WaitAutorefresh => match self.cmd.val() {
                SDRAMCommand::NOP => {}
                SDRAMCommand::AutoRefresh => {
                    if self.banks_busy.val() {
                        self.state.d.next = MasterState::Error;
                    } else {
                        self.auto_refresh_init_counter.d.next =
                            self.auto_refresh_init_counter.q.val() + 1;
                    }
                }
                SDRAMCommand::LoadModeRegister => {
                    if self.auto_refresh_init_counter.q.val() < 2 {
                        self.state.d.next = MasterState::Error;
                    } else {
                        self.counter.d.next = 0.into();
                        self.state.d.next = MasterState::LoadModeRegister;
                        self.burst_len.d.next = self.sdram.address.val().get_bits::<3>(0);
                        self.burst_type.d.next = self.sdram.address.val().get_bit(3);
                        self.cas_latency.d.next = self.sdram.address.val().get_bits::<3>(4);
                        self.op_mode.d.next = self.sdram.address.val().get_bits::<2>(7);
                        self.write_burst_mode.d.next = self.sdram.address.val().get_bit(9);
                        if self.sdram.address.val().get_bits::<2>(10) != 0 {
                            self.state.d.next = MasterState::Error;
                        }
                    }
                }
                _ => {
                    self.state.d.next = MasterState::Error;
                }
            },
            MasterState::LoadModeRegister => {
                self.counter.d.next = self.counter.q.val() + 1;
                if self.counter.q.val() == self.load_mode_timing.val() {
                    self.state.d.next = MasterState::Ready;
                }
                if self.cmd.val() != SDRAMCommand::NOP {
                    self.state.d.next = MasterState::Error;
                }
                if self.burst_len.q.val() > 3 {
                    self.state.d.next = MasterState::Error;
                }
                if (self.cas_latency.q.val() > 3) | (self.cas_latency.q.val() == 0) {
                    self.state.d.next = MasterState::Error;
                }
                if self.op_mode.q.val() != 0 {
                    self.state.d.next = MasterState::Error;
                }
            }
            MasterState::Error => {
                self.test_error.next = true;
            }
            MasterState::Ready => {
                self.test_ready.next = true;
            }
            _ => {
                self.state.d.next = MasterState::Boot;
            }
        }
        // Any banks that are in error mean the chip is in error.
        for i in 0..4 {
            if self.banks[i].error.val() {
                self.state.d.next = MasterState::Error;
            }
        }
    }
}

impl<const R: usize, const C: usize, const A: usize, const D: usize> SDRAMSimulator<R, C, A, D> {
    pub fn new(timings: MemoryTimings) -> Self {
        // Calculate the number of picoseconds per clock cycle
        let boot_delay = timings.t_boot();
        let precharge_delay = timings.t_rp() - 1;
        let bank_bank_delay = timings.t_rrd() - 1;
        Self {
            clock: Default::default(),
            cmd: Signal::default(),
            sdram: Default::default(),
            test_error: Default::default(),
            test_ready: Default::default(),
            state: Default::default(),
            counter: Default::default(),
            auto_refresh_init_counter: Default::default(),
            write_burst_mode: Default::default(),
            cas_latency: Default::default(),
            burst_type: Default::default(),
            burst_len: Default::default(),
            op_mode: Default::default(),
            banks: array_init::array_init(|_| MemoryBank::new(timings)),
            boot_delay: Constant::new(boot_delay.to_bits()),
            t_rp: Constant::new(precharge_delay.to_bits()),
            t_rrd: Constant::new(bank_bank_delay.to_bits()),
            load_mode_timing: Constant::new(
                (timings.load_mode_command_timing_clocks - 1).to_bits(),
            ),
            banks_busy: Default::default(),
            decode: Default::default(),
        }
    }
}

#[cfg(test)]
fn mk_sdr_sim() -> SDRAMSimulator<5, 5, 10, 16> {
    let mut uut = SDRAMSimulator::new(MemoryTimings::fast_boot_sim(125e6));
    uut.sdram.link_connect_dest();
    uut.connect_all();
    uut
}

#[test]
fn test_sdram_sim_synthesizes() {
    let uut = mk_sdr_sim();
    let vlog = generate_verilog(&uut);
    yosys_validate("sdram", &vlog).unwrap();
}

#[macro_export]
macro_rules! sdram_cmd {
    ($uut: ident, $cmd: expr) => {
        match $cmd {
            SDRAMCommand::NOP => {
                $uut.sdram.ras_not.next = true;
                $uut.sdram.cas_not.next = true;
                $uut.sdram.we_not.next = true;
            }
            SDRAMCommand::BurstTerminate => {
                $uut.sdram.ras_not.next = true;
                $uut.sdram.cas_not.next = true;
                $uut.sdram.we_not.next = false;
            }
            SDRAMCommand::Read => {
                $uut.sdram.ras_not.next = true;
                $uut.sdram.cas_not.next = false;
                $uut.sdram.we_not.next = true;
            }
            SDRAMCommand::Write => {
                $uut.sdram.ras_not.next = true;
                $uut.sdram.cas_not.next = false;
                $uut.sdram.we_not.next = false;
            }
            SDRAMCommand::Active => {
                $uut.sdram.ras_not.next = false;
                $uut.sdram.cas_not.next = true;
                $uut.sdram.we_not.next = true;
            }
            SDRAMCommand::Precharge => {
                $uut.sdram.ras_not.next = false;
                $uut.sdram.cas_not.next = true;
                $uut.sdram.we_not.next = false;
            }
            SDRAMCommand::AutoRefresh => {
                $uut.sdram.ras_not.next = false;
                $uut.sdram.cas_not.next = false;
                $uut.sdram.we_not.next = true;
            }
            SDRAMCommand::LoadModeRegister => {
                $uut.sdram.ras_not.next = false;
                $uut.sdram.cas_not.next = false;
                $uut.sdram.we_not.next = false;
            }
        }
    };
}

#[macro_export]
macro_rules! sdram_activate {
    ($sim: ident, $clock: ident, $uut: ident, $bank: expr, $row: expr) => {
        sdram_cmd!($uut, SDRAMCommand::Active);
        $uut.sdram.address.next = ($row as u32).to_bits();
        $uut.sdram.bank.next = ($bank as u32).to_bits();
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
    };
}

#[macro_export]
macro_rules! sdram_write {
    ($sim: ident, $clock: ident, $uut: ident, $bank: expr, $addr: expr, $data: expr) => {
        sdram_cmd!($uut, SDRAMCommand::Write);
        $uut.sdram.bank.next = ($bank as u32).to_bits();
        $uut.sdram.write_enable.next = true;
        $uut.sdram.write_data.next = ($data[0] as u32).to_bits();
        $uut.sdram.address.next = ($addr as u32).to_bits();
        wait_clock_cycle!($sim, $clock, $uut);
        for i in 1..($data).len() {
            sdram_cmd!($uut, SDRAMCommand::NOP);
            $uut.sdram.write_data.next = ($data[i] as u32).to_bits();
            $uut.sdram.address.next = 0.into();
            wait_clock_cycle!($sim, $clock, $uut);
        }
        $uut.sdram.write_enable.next = false;
    };
}

#[macro_export]
macro_rules! sdram_read {
    ($sim: ident, $clock: ident, $uut: ident, $bank: expr, $addr: expr, $data: expr) => {
        sdram_cmd!($uut, SDRAMCommand::Read);
        $uut.sdram.bank.next = ($bank as u32).to_bits();
        $uut.sdram.address.next = ($addr as u32).to_bits();
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_cycles!($sim, $clock, $uut, 2); // Programmed CAS delay - 1
        for datum in $data {
            sdram_cmd!($uut, SDRAMCommand::NOP);
            sim_assert!(
                $sim,
                $uut.sdram.read_data.val() == (datum as u32).to_bits(),
                $uut
            );
            wait_clock_cycle!($sim, $clock, $uut);
        }
    };
}

#[macro_export]
macro_rules! sdram_reada {
    ($sim: ident, $clock: ident, $uut: ident, $bank: expr, $addr: expr, $data: expr) => {
        sdram_cmd!($uut, SDRAMCommand::Read);
        $uut.sdram.bank.next = ($bank as u32).to_bits();
        $uut.sdram.address.next = ($addr as u32 | 1024_u32).to_bits(); // Signal autoprecharge
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_cycles!($sim, $clock, $uut, 2); // Programmed CAS delay
        for datum in $data {
            sdram_cmd!($uut, SDRAMCommand::NOP);
            sim_assert!(
                $sim,
                $uut.sdram.read_data.val() == (datum as u32).to_bits(),
                $uut
            );
            wait_clock_cycle!($sim, $clock, $uut);
        }
    };
}

#[macro_export]
macro_rules! sdram_precharge_one {
    ($sim: ident, $clock: ident, $uut: ident, $bank: expr) => {
        sdram_cmd!($uut, SDRAMCommand::Precharge);
        $uut.sdram.bank.next = ($bank as u32).to_bits();
        $uut.sdram.address.next = 0.into();
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
    };
}

#[macro_export]
macro_rules! sdram_refresh {
    ($sim: ident, $clock: ident, $uut: ident, $timings: expr) => {
        sdram_cmd!($uut, SDRAMCommand::AutoRefresh);
        $uut.sdram.bank.next = 0.into();
        $uut.sdram.address.next = 0.into();
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_cycles!($sim, $clock, $uut, $timings.t_rfc());
    };
}

#[macro_export]
macro_rules! sdram_boot {
    ($sim: ident, $clock: ident, $uut: ident, $timings: ident) => {
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_true!($sim, $clock, $uut);
        // Wait for 100 microseconds
        // 100 microseconds = 100 * 1_000_000
        // Pad by 100 nanoseconds
        $uut = $sim.wait(
            (($timings.initial_delay_in_nanoseconds + 600.0) * 1000.0) as u64,
            $uut,
        )?;
        wait_clock_true!($sim, $clock, $uut);
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::Precharge);
        $uut.sdram.address.next = 0xFFF.into();
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_cycles!($sim, $clock, $uut, $timings.t_rp());
        sdram_cmd!($uut, SDRAMCommand::AutoRefresh);
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_cycles!($sim, $clock, $uut, $timings.t_rfc());
        sdram_cmd!($uut, SDRAMCommand::AutoRefresh);
        wait_clock_cycle!($sim, $clock, $uut);
        sdram_cmd!($uut, SDRAMCommand::NOP);
        wait_clock_cycles!($sim, $clock, $uut, $timings.t_rfc());
    };
}

#[test]
fn test_sdram_init_works() {
    let uut = mk_sdr_sim();
    let mut sim = Simulation::new();
    // Clock period at 125 MHz is 8000ps
    sim.add_clock(4000, |x: &mut Box<SDRAMSimulator<5, 5, 10, 16>>| {
        x.sdram.clk.next = !x.sdram.clk.val();
    });
    sim.add_testbench(move |mut sim: Sim<SDRAMSimulator<5, 5, 10, 16>>| {
        let mut x = sim.init()?;
        let timings = MemoryTimings::fast_boot_sim(125e6);
        wait_clock_cycles!(sim, clock, x, 16);
        sdram_boot!(sim, clock, x, timings);
        sdram_cmd!(x, SDRAMCommand::LoadModeRegister);
        x.sdram.address.next = 0b000_0_00_011_0_011.into();
        wait_clock_cycle!(sim, clock, x);
        sdram_cmd!(x, SDRAMCommand::NOP);
        wait_clock_cycles!(sim, clock, x, 5);
        sim_assert_eq!(sim, x.state.q.val(), MasterState::Ready, x);
        // Activate row 14 on bank 2
        sdram_activate!(sim, clock, x, 2, 14);
        // Activate row 7 on bank 1
        wait_clock_cycles!(sim, clock, x, timings.t_rrd());
        sdram_activate!(sim, clock, x, 1, 7);
        wait_clock_cycles!(sim, clock, x, timings.t_ras());
        sdram_write!(
            sim,
            clock,
            x,
            2,
            16,
            [0xABCD, 0xDEAD, 0xBEEF, 0x1234, 0xFACE, 0x5EA1, 0xCAFE, 0xBABE]
        );
        sdram_precharge_one!(sim, clock, x, 2);
        sdram_write!(
            sim,
            clock,
            x,
            1,
            24,
            [0xABCE, 0xDEAE, 0xBEE0, 0x1235, 0xFACF, 0x5EA2, 0xCAFF, 0xBABF]
        );
        sdram_precharge_one!(sim, clock, x, 1);
        wait_clock_cycles!(sim, clock, x, timings.t_rp() + 1);
        sim_assert!(sim, !x.banks_busy.val(), x);
        sim_assert_eq!(sim, x.state.q.val(), MasterState::Ready, x);
        sdram_activate!(sim, clock, x, 1, 7);
        wait_clock_cycles!(sim, clock, x, timings.t_rcd());
        sdram_read!(
            sim,
            clock,
            x,
            1,
            24,
            [0xABCE, 0xDEAE, 0xBEE0, 0x1235, 0xFACF, 0x5EA2, 0xCAFF, 0xBABF]
        );
        sdram_precharge_one!(sim, clock, x, 1);
        sdram_activate!(sim, clock, x, 2, 14);
        wait_clock_cycles!(sim, clock, x, timings.t_rcd());
        sdram_reada!(
            sim,
            clock,
            x,
            2,
            16,
            [0xABCD, 0xDEAD, 0xBEEF, 0x1234, 0xFACE, 0x5EA1, 0xCAFE, 0xBABE]
        );
        wait_clock_cycles!(sim, clock, x, timings.t_rp() + 1);
        sim_assert!(sim, !x.banks_busy.val(), x);
        sim_assert_eq!(sim, x.state.q.val(), MasterState::Ready, x);
        sdram_refresh!(sim, clock, x, timings);
        sim_assert!(sim, !x.banks_busy.val(), x);
        sim_assert_eq!(sim, x.state.q.val(), MasterState::Ready, x);
        wait_clock_cycles!(sim, clock, x, 10);
        sim.done(x)
    });
    sim.run_to_file(Box::new(uut), 200_000_000, &vcd_path!("sdr_init.vcd"))
        .unwrap()
}
