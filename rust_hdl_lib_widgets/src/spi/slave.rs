use crate::edge_detector::EdgeDetector;
use crate::spi::master::{SPIConfig, SPIWiresSlave};
use crate::synchronizer::BitSynchronizer;
use crate::{dff::DFF, dff_setup};
use rust_hdl_lib_core::prelude::*;

#[derive(Copy, Clone, PartialEq, Debug, LogicState)]
enum SPISlaveState {
    Boot,
    Idle,
    Armed,
    Capture,
    Hold,
    Update,
    Settle,
    Waiting,
    Hangup,
    Disabled,
}

/// The [SPISlave] is mostly meant for testing the [SPIMaster], but you can
/// use it to implement a SPI endpoint in the FPGA if you want to.  This [SPISlave]
/// is not very robust, so be cautious with using it.  In particular, with a very
/// badly behaved SPI master, it may not operate as expected.
#[derive(LogicBlock)]
pub struct SPISlave<const N: usize> {
    /// The clock driving the [SPISlave]
    pub clock: Signal<In, Clock>,
    /// The bus connecting us to the [SPIMaster] or an external SPI bus.
    pub wires: SPIWiresSlave,
    /// Raise thie `disabled` signal if you want the [SPISlave] to ignore the `wires` signals.
    pub disabled: Signal<In, Bit>,
    /// Indicates the [SPISlave] is busy (typically, receiving data from the [SPIMaster].
    pub busy: Signal<Out, Bit>,
    /// Data received from the [SPIMaster] is output on these wires.
    pub data_inbound: Signal<Out, Bits<N>>,
    /// Assert for a single cycle to latch the data to be sent back to the [SPIMaster] on the MISO line.  Latches
    /// `data_outbound`,`bits` and `continued_transaction` when asserted.
    pub start_send: Signal<In, Bit>,
    /// Data destined for the [SPIMaster] on the next transaction.
    pub data_outbound: Signal<In, Bits<N>>,
    /// Number of bits to send.  Capped at 16 bits (which corresponds to 64K bits on the send - not realistic).
    pub bits: Signal<In, Bits<16>>,
    /// Set this to true to indicate that the next transaction will be continued from this one (i.e., do not hangup at the end).
    pub continued_transaction: Signal<In, Bit>,
    /// A flag that indicates the inbound data is valid.
    pub transfer_done: Signal<Out, Bit>,
    miso_flop: DFF<Bit>,
    done_flop: DFF<Bit>,
    register_out: DFF<Bits<N>>,
    register_in: DFF<Bits<N>>,
    state: DFF<SPISlaveState>,
    pointer: DFF<Bits<16>>,
    bits_saved: DFF<Bits<16>>,
    continued_saved: DFF<Bit>,
    capture_detector: EdgeDetector,
    advance_detector: EdgeDetector,
    edge_detector: EdgeDetector,
    mclk_synchronizer: BitSynchronizer,
    csel_synchronizer: BitSynchronizer,
    escape: DFF<Bits<16>>,
    clocks_per_baud: Constant<Bits<16>>,
    cpha: Constant<Bit>,
    cs_off: Constant<Bit>,
    boot_delay: DFF<Bits<4>>,
}

//
// Here is a table of the SPI setup:
// CPOL  CPHA  EDGE  ACTION
//  0     0     R     Sample
//  0     0     F     Change
//  0     1     R     Change
//  0     1     F     Sample
//  1     0     R     Change
//  1     0     F     Sample
//  1     1     R     Sample
//  1     1     F     Change
//
// So Sample on Rising edge if CPOL == CPHA
// Also, CPHA decides if we start in the sample state or in the change state
impl<const N: usize> SPISlave<N> {
    /// Generate a new [SPISlave] with the given [SPIConfig]
    ///
    /// # Arguments
    ///
    /// * `config`: The [SPIConfig] that configures the slave receiver.
    ///
    /// returns: SPISlave<{ N }>
    ///
    /// # Examples
    ///
    /// See [ADS868XSimulator] for an example of how a [SPISlave] can be used.
    pub fn new(config: SPIConfig) -> Self {
        // Because the synchronizers introduce a 2 clock delay, in the non-phased
        // modes, we need to be able to react quickly enough to capture the first
        // data edge.  Short of a new design, I have added this clock speed constraint.
        assert!(config.cpha | (config.clock_speed >= 40 * config.speed_hz));
        Self {
            clock: Default::default(),
            wires: Default::default(),
            disabled: Default::default(),
            busy: Default::default(),
            data_inbound: Default::default(),
            start_send: Default::default(),
            data_outbound: Default::default(),
            bits: Default::default(),
            continued_transaction: Default::default(),
            transfer_done: Default::default(),
            miso_flop: Default::default(),
            done_flop: Default::default(),
            register_out: Default::default(),
            register_in: Default::default(),
            state: Default::default(),
            pointer: Default::default(),
            bits_saved: Default::default(),
            continued_saved: Default::default(),
            capture_detector: EdgeDetector::new(!(config.cpol ^ config.cpha)),
            advance_detector: EdgeDetector::new(config.cpol ^ config.cpha),
            edge_detector: EdgeDetector::new(!config.cs_off),
            mclk_synchronizer: BitSynchronizer::default(),
            csel_synchronizer: BitSynchronizer::default(),
            escape: Default::default(),
            clocks_per_baud: Constant::new((2 * config.clock_speed / config.speed_hz).into()),
            cpha: Constant::new(config.cpha),
            cs_off: Constant::new(config.cs_off),
            boot_delay: Default::default(),
        }
    }
}

impl<const N: usize> Logic for SPISlave<N> {
    #[hdl_gen]
    fn update(&mut self) {
        dff_setup!(
            self,
            clock,
            miso_flop,
            done_flop,
            register_out,
            register_in,
            state,
            pointer,
            bits_saved,
            continued_saved,
            escape,
            boot_delay
        );
        clock!(
            self,
            clock,
            capture_detector,
            advance_detector,
            edge_detector,
            mclk_synchronizer,
            csel_synchronizer
        );
        // Connect the detectors
        self.capture_detector.input_signal.next = self.mclk_synchronizer.sig_out.val();
        self.advance_detector.input_signal.next = self.mclk_synchronizer.sig_out.val();
        self.edge_detector.input_signal.next = self.csel_synchronizer.sig_out.val();
        // Connect the synchronizers
        self.mclk_synchronizer.sig_in.next = self.wires.mclk.val();
        self.csel_synchronizer.sig_in.next = self.wires.msel.val();
        // Logic
        self.busy.next = (self.state.q.val() != SPISlaveState::Idle)
            | (self.csel_synchronizer.sig_out.val() != self.cs_off.val());
        if self.state.q.val() != SPISlaveState::Disabled {
            self.wires.miso.next = self.miso_flop.q.val();
        } else {
            self.wires.miso.next = true;
        }
        self.data_inbound.next = self.register_in.q.val();
        self.transfer_done.next = self.done_flop.q.val();
        self.done_flop.d.next = false;
        self.miso_flop.d.next = self
            .register_out
            .q
            .val()
            .get_bit(self.pointer.q.val().index());
        self.boot_delay.d.next = self.boot_delay.q.val() + 1;
        match self.state.q.val() {
            SPISlaveState::Boot => {
                if self.boot_delay.q.val() == 8 {
                    self.state.d.next = SPISlaveState::Idle;
                }
            }
            SPISlaveState::Idle => {
                if self.edge_detector.edge_signal.val() {
                    self.register_in.d.next = 0.into();
                    self.state.d.next = SPISlaveState::Waiting;
                    self.pointer.d.next = 0.into();
                    self.escape.d.next = 0.into();
                } else if self.start_send.val() {
                    self.register_out.d.next = self.data_outbound.val();
                    self.bits_saved.d.next = self.bits.val();
                    self.continued_saved.d.next = self.continued_transaction.val();
                    self.pointer.d.next = self.bits.val() - 1;
                    self.register_in.d.next = 0.into();
                    self.state.d.next = SPISlaveState::Armed;
                } else if self.disabled.val() {
                    self.state.d.next = SPISlaveState::Disabled;
                }
            }
            SPISlaveState::Armed => {
                if self.csel_synchronizer.sig_out.val() != self.cs_off.val() {
                    if self.cpha.val() & !self.continued_saved.q.val() {
                        self.state.d.next = SPISlaveState::Waiting;
                    } else {
                        self.state.d.next = SPISlaveState::Settle;
                    }
                }
            }
            SPISlaveState::Waiting => {
                if self.advance_detector.edge_signal.val() {
                    self.state.d.next = SPISlaveState::Settle;
                }
                // Hangup condition.  CSEL should remain low for the entire transaction.
                if self.cpha.val()
                    & !self.continued_saved.q.val()
                    & (self.csel_synchronizer.sig_out.val() == self.cs_off.val())
                {
                    self.state.d.next = SPISlaveState::Idle;
                }
                if !self.cpha.val() & (self.csel_synchronizer.sig_out.val() == self.cs_off.val()) {
                    self.escape.d.next = self.escape.q.val() + 1;
                    if self.escape.q.val().all() {
                        self.state.d.next = SPISlaveState::Idle;
                    }
                }
            }
            SPISlaveState::Settle => {
                if self.capture_detector.edge_signal.val() {
                    self.state.d.next = SPISlaveState::Capture;
                }
                // Hangup condition.  CSEL should remain low for the entire transaction.
                if self.csel_synchronizer.sig_out.val() == self.cs_off.val() {
                    self.state.d.next = SPISlaveState::Idle;
                }
            }
            SPISlaveState::Capture => {
                self.register_in.d.next = (self.register_in.q.val() << 1)
                    | bit_cast::<N, 1>(self.wires.mosi.val().into());
                self.state.d.next = SPISlaveState::Hold;
            }
            SPISlaveState::Hold => {
                if self.advance_detector.edge_signal.val() {
                    if self.pointer.q.val().any() {
                        self.state.d.next = SPISlaveState::Update;
                    } else {
                        if self.continued_saved.q.val() {
                            self.done_flop.d.next = true;
                            self.state.d.next = SPISlaveState::Idle;
                        } else {
                            self.state.d.next = SPISlaveState::Hangup;
                        }
                    }
                    self.escape.d.next = 0.into();
                } else if self.csel_synchronizer.sig_out.val() == self.cs_off.val() {
                    self.done_flop.d.next = true;
                    self.state.d.next = SPISlaveState::Idle;
                } else {
                    self.escape.d.next = self.escape.q.val() + 1;
                }
                if self.escape.q.val() == self.clocks_per_baud.val() {
                    self.done_flop.d.next = true;
                    self.state.d.next = SPISlaveState::Idle;
                }
            }
            SPISlaveState::Update => {
                if self.pointer.q.val().any() {
                    self.pointer.d.next = self.pointer.q.val() - 1;
                }
                self.state.d.next = SPISlaveState::Settle;
            }
            SPISlaveState::Hangup => {
                if self.csel_synchronizer.sig_out.val() == self.cs_off.val() {
                    self.done_flop.d.next = true;
                    self.state.d.next = SPISlaveState::Idle;
                }
                if self.disabled.val() {
                    self.state.d.next = SPISlaveState::Disabled;
                }
            }
            SPISlaveState::Disabled => {
                if !self.disabled.val() {
                    self.state.d.next = SPISlaveState::Idle;
                    self.register_out.d.next = 0.into();
                }
            }
            _ => {
                self.state.d.next = SPISlaveState::Boot;
            }
        }
    }
}

#[test]
fn test_spi_slave_synthesizes() {
    let config = SPIConfig {
        clock_speed: 48_000_000,
        cs_off: true,
        mosi_off: false,
        speed_hz: 1_000_000,
        cpha: true,
        cpol: false,
    };
    let mut uut: SPISlave<64> = SPISlave::new(config);
    uut.connect_all();
    yosys_validate("spi_slave", &generate_verilog(&uut)).unwrap();
}
