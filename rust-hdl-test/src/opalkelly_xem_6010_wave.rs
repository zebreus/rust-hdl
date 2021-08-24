use rust_hdl_core::prelude::*;
use rust_hdl_ok::prelude::*;

use crate::alchitry_cu_pwm_vec_srom::FaderWithSyncROM;

#[derive(LogicBlock)]
pub struct OpalKellyXEM6010Wave {
    pub hi: OpalKellyHostInterface,
    pub ok_host: OpalKellyHost,
    pub led: Signal<Out, Bits<8>>,
    pub local: Signal<Local, Bits<8>>,
    pub faders: [FaderWithSyncROM; 8],
}

impl Logic for OpalKellyXEM6010Wave {
    #[hdl_gen]
    fn update(&mut self) {
        self.hi.link(&mut self.ok_host.hi);
        for i in 0_usize..8_usize {
            self.faders[i].clock.next = self.ok_host.ti_clk.val();
            self.faders[i].enable.next = true;
        }
        self.local.next = 0x00_u8.into();
        for i in 0_usize..8_usize {
            self.local.next = self
                .local
                .val()
                .replace_bit(i, !self.faders[i].active.val());
        }
        self.led.next = self.local.val();
    }
}

impl Default for OpalKellyXEM6010Wave {
    fn default() -> Self {
        let faders: [FaderWithSyncROM; 8] = [
            FaderWithSyncROM::new(MHZ48, 0),
            FaderWithSyncROM::new(MHZ48, 18),
            FaderWithSyncROM::new(MHZ48, 36),
            FaderWithSyncROM::new(MHZ48, 54),
            FaderWithSyncROM::new(MHZ48, 72),
            FaderWithSyncROM::new(MHZ48, 90),
            FaderWithSyncROM::new(MHZ48, 108),
            FaderWithSyncROM::new(MHZ48, 128),
        ];
        Self {
            hi: OpalKellyHostInterface::xem_6010(),
            ok_host: Default::default(),
            local: Signal::default(),
            faders,
            led: xem_6010_leds(),
        }
    }
}

#[test]
fn test_opalkelly_xem_6010_wave() {
    let mut uut = OpalKellyXEM6010Wave::default();
    uut.hi.sig_in.connect();
    uut.hi.sig_out.connect();
    uut.hi.sig_inout.connect();
    uut.hi.sig_aa.connect();
    uut.connect_all();
    crate::ok_tools::synth_obj(uut, "opalkelly_xem_6010_wave");
}
