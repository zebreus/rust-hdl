use rand::Rng;
use rust_hdl::core::prelude::*;
use rust_hdl::hls::prelude::*;
use rust_hdl::widgets::prelude::*;

mod test_common;
use crate::test_common::fifo_tester::bursty_vec;
use test_common::fifo_tester::{LazyFIFOFeeder, LazyFIFOReader};

#[derive(LogicBlock)]
struct BusTest {
    dtm_feeder: LazyFIFOFeeder<Bits<8>, 10>,
    dtm_reader: LazyFIFOReader<Bits<8>, 10>,
    mtd_feeder: LazyFIFOFeeder<Bits<8>, 10>,
    mtd_reader: LazyFIFOReader<Bits<8>, 10>,
    device_to_bus_fifo: SyncFIFO<Bits<8>, 4, 5, 1>,
    device_from_bus_fifo: SyncFIFO<Bits<8>, 4, 5, 1>,
    pub device: BidiSimulatedDevice<Bits<8>>,
    pub master: BidiMaster<Bits<8>>,
    master_from_bus_fifo: SyncFIFO<Bits<8>, 4, 5, 1>,
    master_to_bus_fifo: SyncFIFO<Bits<8>, 4, 5, 1>,
    pub clock: Signal<In, Clock>,
}

impl Default for BusTest {
    fn default() -> Self {
        let dlen = 256;
        let data1 = (0..dlen)
            .map(|x| Bits::<8>::from(rand::thread_rng().gen::<u8>()))
            .collect::<Vec<_>>();
        let data2 = (0..dlen)
            .map(|x| Bits::<8>::from(rand::thread_rng().gen::<u8>()))
            .collect::<Vec<_>>();

        Self {
            dtm_feeder: LazyFIFOFeeder::new(&data1, &bursty_vec(data1.len())),
            dtm_reader: LazyFIFOReader::new(&data1, &bursty_vec(data1.len())),
            mtd_feeder: LazyFIFOFeeder::new(&data2, &bursty_vec(data2.len())),
            mtd_reader: LazyFIFOReader::new(&data2, &bursty_vec(data2.len())),
            device_to_bus_fifo: Default::default(),
            device_from_bus_fifo: Default::default(),
            device: Default::default(),
            master: Default::default(),
            master_from_bus_fifo: Default::default(),
            master_to_bus_fifo: Default::default(),
            clock: Default::default(),
        }
    }
}

impl Logic for BusTest {
    #[hdl_gen]
    fn update(&mut self) {
        // Clock the components
        self.master.clock.next = self.clock.val();
        self.device.clock.next = self.clock.val();
        self.dtm_feeder.clock.next = self.clock.val();
        self.dtm_reader.clock.next = self.clock.val();
        self.mtd_feeder.clock.next = self.clock.val();
        self.mtd_reader.clock.next = self.clock.val();
        self.device_to_bus_fifo.clock.next = self.clock.val();
        self.device_from_bus_fifo.clock.next = self.clock.val();
        self.master_from_bus_fifo.clock.next = self.clock.val();
        self.master_to_bus_fifo.clock.next = self.clock.val();
        // Connect the busses
        self.device
            .data_to_bus
            .join(&mut self.device_to_bus_fifo.bus_read);
        self.device
            .data_from_bus
            .join(&mut self.device_from_bus_fifo.bus_write);
        self.master
            .data_to_bus
            .join(&mut self.master_to_bus_fifo.bus_read);
        self.master
            .data_from_bus
            .join(&mut self.master_from_bus_fifo.bus_write);
        self.master.bus.join(&mut self.device.bus);
        self.dtm_feeder
            .bus
            .join(&mut self.device_to_bus_fifo.bus_write);
        self.mtd_feeder
            .bus
            .join(&mut self.master_to_bus_fifo.bus_write);
        self.dtm_reader
            .bus
            .join(&mut self.master_from_bus_fifo.bus_read);
        self.mtd_reader
            .bus
            .join(&mut self.device_from_bus_fifo.bus_read);
    }
}

#[test]
fn test_bidi2_bus_test_synthesizes() {
    let mut uut = BusTest::default();
    uut.mtd_feeder.start.connect();
    uut.mtd_reader.start.connect();
    uut.dtm_feeder.start.connect();
    uut.dtm_reader.start.connect();
    uut.clock.connect();
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("tribus", &vlog).unwrap();
}

#[test]
fn test_bidi2_bus_works() {
    let mut uut = BusTest::default();
    uut.mtd_feeder.start.connect();
    uut.mtd_reader.start.connect();
    uut.dtm_feeder.start.connect();
    uut.dtm_reader.start.connect();
    uut.clock.connect();
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("tribus_0", &vlog).unwrap();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<BusTest>| x.clock.next = !x.clock.val());
    sim.add_testbench(move |mut sim: Sim<BusTest>| {
        let mut x = sim.init()?;
        wait_clock_true!(sim, clock, x);
        x.dtm_feeder.start.next = true;
        x.dtm_reader.start.next = true;
        x.mtd_feeder.start.next = true;
        x.mtd_reader.start.next = true;
        wait_clock_cycle!(sim, clock, x);
        x.dtm_feeder.start.next = false;
        x.dtm_reader.start.next = false;
        x.mtd_feeder.start.next = false;
        x.mtd_reader.start.next = false;
        x = sim.watch(
            |x| {
                x.dtm_feeder.done.val()
                    & x.dtm_reader.done.val()
                    & x.mtd_feeder.done.val()
                    & x.mtd_reader.done.val()
            },
            x,
        )?;
        wait_clock_cycle!(sim, clock, x);
        sim_assert!(sim, !x.dtm_reader.error.val(), x);
        sim_assert!(sim, !x.mtd_reader.error.val(), x);
        sim.done(x)
    });
    let mut vcd = vec![];
    let ret = sim.run_traced(Box::new(uut), 100_000, &mut vcd);
    std::fs::write(vcd_path!("bidi_stress.vcd"), vcd).unwrap();
    ret.unwrap()
}
