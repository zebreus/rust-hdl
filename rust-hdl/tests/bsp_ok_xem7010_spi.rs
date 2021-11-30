use rust_hdl::core::prelude::*;

mod test_common;

#[cfg(feature = "frontpanel")]
use test_common::spi::*;

use rust_hdl::bsp::ok_core::prelude::*;
use rust_hdl::bsp::ok_xem7010::*;

#[cfg(feature = "frontpanel")]
#[test]
fn test_opalkelly_xem_7010_synth_spi() {
    let mut uut = OpalKellySPITest::new::<XEM7010>();
    uut.hi.link_connect_dest();
    uut.connect_all();
    XEM7010::synth(uut, target_path!("xem_7010/spi"));
    test_opalkelly_spi_reg_read_runtime(target_path!("xem_7010/spi/top.bit")).unwrap();
    test_opalkelly_spi_reg_write_runtime(target_path!("xem_7010/spi/top.bit")).unwrap();
    test_opalkelly_spi_single_conversion_runtime(target_path!("xem_7010/spi/top.bit")).unwrap();
}
