use crate::dff_setup;
use crate::spi::master::SPIWiresMaster;
use crate::synchronizer::BitSynchronizer;
use crate::{dff::DFF, dff_with_init::DFFWithInit, spi::master::SPIConfig, strobe::Strobe};
use rust_hdl_lib_core::prelude::*;

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum SPIState {
    Idle,
    SetMode,
    Activate,
    Dwell,
    LoadBit,
    MActive,
    SampleMISO,
    MIdle,
    Finish,
}

#[derive(Copy, Clone)]
pub struct SPIConfigDynamicMode {
    pub clock_speed: u64,
    pub cs_off: bool,
    pub mosi_off: bool,
    pub speed_hz: u64,
}

impl From<SPIConfig> for SPIConfigDynamicMode {
    fn from(x: SPIConfig) -> Self {
        SPIConfigDynamicMode {
            clock_speed: x.clock_speed,
            cs_off: x.cs_off,
            mosi_off: x.mosi_off,
            speed_hz: x.speed_hz,
        }
    }
}

impl Into<SPIConfig> for SPIConfigDynamicMode {
    fn into(self) -> SPIConfig {
        SPIConfig {
            clock_speed: self.clock_speed,
            cs_off: self.cs_off,
            mosi_off: self.mosi_off,
            speed_hz: self.speed_hz,
            cpha: false,
            cpol: false,
        }
    }
}

#[derive(LogicBlock)]
pub struct SPIMasterDynamicMode<const N: usize> {
    pub clock: Signal<In, Clock>,
    pub bits_outbound: Signal<In, Bits<16>>,
    pub data_outbound: Signal<In, Bits<N>>,
    pub data_inbound: Signal<Out, Bits<N>>,
    pub start_send: Signal<In, Bit>,
    pub transfer_done: Signal<Out, Bit>,
    pub continued_transaction: Signal<In, Bit>,
    pub wires: SPIWiresMaster,
    pub busy: Signal<Out, Bit>,
    register_out: DFF<Bits<N>>,
    register_in: DFF<Bits<N>>,
    state: DFF<SPIState>,
    strobe: Strobe<32>,
    pointer: DFF<Bits<16>>,
    pointerm1: Signal<Local, Bits<16>>,
    clock_state: DFF<Bit>,
    done_flop: DFF<Bit>,
    msel_flop: DFFWithInit<Bit>,
    mosi_flop: DFF<Bit>,
    miso_synchronizer: BitSynchronizer,
    continued_save: DFF<Bit>,
    cs_off: Constant<Bit>,
    mosi_off: Constant<Bit>,
    cpha_flop: DFF<Bit>,
    cpol_flop: DFF<Bit>,
}

impl<const N: usize> SPIMasterDynamicMode<N> {
    pub fn new(config: SPIConfigDynamicMode) -> Self {
        assert!(8 * config.speed_hz <= config.clock_speed);
        Self {
            clock: Default::default(),
            bits_outbound: Default::default(),
            data_outbound: Default::default(),
            data_inbound: Default::default(),
            start_send: Default::default(),
            transfer_done: Default::default(),
            continued_transaction: Default::default(),
            wires: Default::default(),
            busy: Default::default(),
            register_out: Default::default(),
            register_in: Default::default(),
            state: Default::default(),
            strobe: Strobe::new(config.clock_speed, 4.0 * config.speed_hz as f64),
            pointer: Default::default(),
            pointerm1: Default::default(),
            clock_state: Default::default(),
            done_flop: Default::default(),
            msel_flop: DFFWithInit::new(config.cs_off),
            mosi_flop: Default::default(),
            miso_synchronizer: Default::default(),
            continued_save: Default::default(),
            cs_off: Constant::new(config.cs_off),
            mosi_off: Constant::new(config.mosi_off),
            cpha_flop: Default::default(),
            cpol_flop: Default::default(),
        }
    }
}

impl<const N: usize> Logic for SPIMasterDynamicMode<N> {
    #[hdl_gen]
    fn update(&mut self) {
        // Wire up the clocks.
        dff_setup!(
            self,
            clock,
            register_out,
            register_in,
            state,
            pointer,
            clock_state,
            done_flop,
            msel_flop,
            mosi_flop,
            continued_save,
            cpha_flop,
            cpol_flop
        );
        clock!(self, clock, miso_synchronizer, strobe);
        // Activate the baud strobe
        self.strobe.enable.next = true;
        // Connect the MISO synchronizer to the input line
        self.miso_synchronizer.sig_in.next = self.wires.miso.val();
        // Connect the rest of the SPI lines to the flops
        self.wires.mclk.next = self.clock_state.q.val();
        self.wires.mosi.next = self.mosi_flop.q.val();
        self.wires.msel.next = self.msel_flop.q.val();
        // Connect the output signals to the internal registers
        self.data_inbound.next = self.register_in.q.val();
        self.transfer_done.next = self.done_flop.q.val();
        // Latch prevention
        self.done_flop.d.next = false;
        self.pointerm1.next = self.pointer.q.val() - 1;
        self.busy.next = self.state.q.val() != SPIState::Idle;
        // The main state machine
        match self.state.q.val() {
            SPIState::Idle => {
                self.clock_state.d.next = self.cpol_flop.q.val();
                if self.start_send.val() {
                    // Capture the outgoing data in our register
                    self.register_out.d.next = self.data_outbound.val();
                    self.state.d.next = SPIState::SetMode; // Transition to the SetMode state - allows the clock to settle
                    self.pointer.d.next = self.bits_outbound.val() & 0x00FF; // set bit pointer to number of bit to send (1 based)
                                                                             // We bind the top two bits of the outbound register to the SPI mode.
                    self.cpha_flop.d.next = self.bits_outbound.val().get_bit(9);
                    self.cpol_flop.d.next = self.bits_outbound.val().get_bit(8);
                    self.register_in.d.next = 0.into(); // Clear out the input store register
                    self.continued_save.d.next = self.continued_transaction.val();
                } else {
                    if !self.continued_save.q.val() {
                        self.msel_flop.d.next = self.cs_off.val(); // Set the chip select signal to be "off"
                    }
                }
                self.mosi_flop.d.next = self.mosi_off.val(); // Set the mosi signal to be "off"
            }
            SPIState::SetMode => {
                self.clock_state.d.next = self.cpol_flop.q.val();
                // Wait for the clock polarity to settle
                if self.strobe.strobe.val() {
                    self.state.d.next = SPIState::Activate;
                }
            }
            SPIState::Activate => {
                if self.strobe.strobe.val() {
                    self.msel_flop.d.next = !self.cs_off.val(); // Activate the chip select
                    self.state.d.next = SPIState::Dwell;
                }
            }
            SPIState::Dwell => {
                if self.strobe.strobe.val() {
                    // Dwell timeout has reached zero
                    self.state.d.next = SPIState::LoadBit; // Transition to the loadbit state
                }
            }
            SPIState::LoadBit => {
                if self.pointer.q.val().any() {
                    // We have data to send
                    self.mosi_flop.d.next = self
                        .register_out
                        .q
                        .val()
                        .get_bit(self.pointerm1.val().index()); // Fetch the corresponding bit out of the register
                    self.pointer.d.next = self.pointerm1.val(); // Decrement the pointer
                    self.state.d.next = SPIState::MActive; // Move to the hold mclock low state
                    self.clock_state.d.next = self.cpol_flop.q.val() ^ self.cpha_flop.q.val();
                } else {
                    self.mosi_flop.d.next = self.mosi_off.val(); // Set the mosi signal to be "off"
                    self.clock_state.d.next = self.cpol_flop.q.val();
                    self.state.d.next = SPIState::Finish; // No data, go back to idle
                }
            }
            SPIState::MActive => {
                if self.strobe.strobe.val() {
                    self.state.d.next = SPIState::SampleMISO;
                }
            }
            SPIState::SampleMISO => {
                self.register_in.d.next = self.register_in.q.val().replace_bit(
                    self.pointer.q.val().index(),
                    self.miso_synchronizer.sig_out.val(),
                );
                self.clock_state.d.next = !self.clock_state.q.val();
                self.state.d.next = SPIState::MIdle;
            }
            SPIState::MIdle => {
                if self.strobe.strobe.val() {
                    self.state.d.next = SPIState::LoadBit;
                }
            }
            SPIState::Finish => {
                if self.strobe.strobe.val() {
                    self.done_flop.d.next = true; // signal the transfer is complete
                    self.state.d.next = SPIState::Idle;
                }
            }
            _ => {
                self.state.d.next = SPIState::Idle;
            }
        }
    }
}

#[test]
fn test_spi_master_dynamic_mode_is_synthesizable() {
    let config = SPIConfigDynamicMode {
        clock_speed: 48_000_000,
        cs_off: true,
        mosi_off: false,
        speed_hz: 1_000_000,
    };
    let mut dev = SPIMasterDynamicMode::<64>::new(config);
    dev.connect_all();
    yosys_validate("spi_master_dyn_mode", &generate_verilog(&dev)).unwrap();
}
