use rust_hdl::prelude::*;
use rust_hdl_bsp_ok_xem6010::xem6010::{synth, XEM6010};
use rust_hdl_lib_ok_core::test_common::spi::{
    test_opalkelly_spi_reg_read_runtime, test_opalkelly_spi_reg_write_runtime,
    test_opalkelly_spi_single_conversion_runtime, OpalKellySPITest,
};

#[test]
fn test_opalkelly_xem_6010_synth_spi() {
    let mut uut = OpalKellySPITest::new::<XEM6010>();
    uut.connect_all();
    synth::synth_obj(uut, target_path!("xem_6010/spi"));
    test_opalkelly_spi_reg_read_runtime(
        target_path!("xem_6010/spi/top.bit"),
        env!("XEM6010_SERIAL"),
    )
    .unwrap();
    test_opalkelly_spi_reg_write_runtime(
        target_path!("xem_6010/spi/top.bit"),
        env!("XEM6010_SERIAL"),
    )
    .unwrap();
    test_opalkelly_spi_single_conversion_runtime(
        target_path!("xem_6010/spi/top.bit"),
        env!("XEM6010_SERIAL"),
    )
    .unwrap();
}
