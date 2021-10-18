use rust_hdl_bsp_alchitry_cu::ice_pll::ICE40PLLBlock;
use rust_hdl_core::prelude::*;
use rust_hdl_yosys_synth::yosys_validate;
use rust_hdl_test_core::target_path;

#[test]
fn test_pll_synthesizable() {
    const MHZ100: u64 = 100_000_000;
    const MHZ25: u64 = 25_000_000;
    let mut uut: ICE40PLLBlock<MHZ100, MHZ25> = ICE40PLLBlock::default();
    uut.clock_in.add_location(0, "P7");
    uut.clock_in.connect();
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("vlog", &vlog).unwrap();
    println!("{}", vlog);
    println!("{}", target_path!("alchitry_cu/pll_cu"));
    rust_hdl_bsp_alchitry_cu::synth::generate_bitstream(uut, target_path!("alchitry_cu/pll_cu"));
}