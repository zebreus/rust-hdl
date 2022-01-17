use rand::Rng;
use rust_hdl::core::prelude::*;
use rust_hdl::hls::prelude::*;
use rust_hdl::widgets::prelude::*;

mod test_common;
use crate::test_common::fifo_tester::bursty_vec;
use test_common::fifo_tester::{LazyFIFOFeeder, LazyFIFOReader};

#[derive(LogicBlock)]
struct ReducerTestFixture {
    feeder: LazyFIFOFeeder<Bits<16>, 10>,
    wide_fifo: SyncFIFO<Bits<16>, 4, 5, 1>,
    reducer: Reducer<16, 4>,
    narrow_fifo: SyncFIFO<Bits<4>, 4, 5, 1>,
    reader: LazyFIFOReader<Bits<4>, 12>,
    clock: Signal<In, Clock>,
}

impl Logic for ReducerTestFixture {
    #[hdl_gen]
    fn update(&mut self) {
        self.feeder.clock.next = self.clock.val();
        self.wide_fifo.clock.next = self.clock.val();
        self.reducer.clock.next = self.clock.val();
        self.narrow_fifo.clock.next = self.clock.val();
        self.reader.clock.next = self.clock.val();
        self.feeder.bus.join(&mut self.wide_fifo.bus_write);
        self.reducer.bus_read.join(&mut self.wide_fifo.bus_read);
        self.narrow_fifo.bus_write.join(&mut self.reducer.bus_write);
        self.narrow_fifo.bus_read.join(&mut self.reader.bus);
    }
}

impl Default for ReducerTestFixture {
    fn default() -> Self {
        let data1 = (0..256)
            .map(|_| Bits::<16>::from(rand::thread_rng().gen::<u16>()))
            .collect::<Vec<_>>();
        let mut data2 = vec![];
        for x in &data1 {
            for offset in &[0, 4, 8, 12] {
                data2.push(x.get_bits::<4>(*offset));
            }
        }
        Self {
            feeder: LazyFIFOFeeder::new(&data1, &bursty_vec(256)),
            wide_fifo: Default::default(),
            reducer: Reducer::new(WordOrder::LeastSignificantFirst),
            narrow_fifo: Default::default(),
            reader: LazyFIFOReader::new(&data2, &bursty_vec(1024)),
            clock: Default::default(),
        }
    }
}

#[test]
fn test_reducer_test_fixture_synthesizes() {
    let mut uut = ReducerTestFixture::default();
    uut.clock.connect();
    uut.feeder.start.connect();
    uut.reader.start.connect();
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("reducer_hls_test", &vlog).unwrap();
}

#[test]
fn test_reducer_test_fixture_operation() {
    let mut uut = ReducerTestFixture::default();
    uut.clock.connect();
    uut.feeder.start.connect();
    uut.reader.start.connect();
    uut.connect_all();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<ReducerTestFixture>| {
        x.clock.next = !x.clock.val()
    });
    sim.add_testbench(move |mut sim: Sim<ReducerTestFixture>| {
        let mut x = sim.init()?;
        wait_clock_true!(sim, clock, x);
        x.feeder.start.next = true;
        x.reader.start.next = true;
        wait_clock_cycle!(sim, clock, x);
        x.feeder.start.next = false;
        x.reader.start.next = false;
        x = sim.watch(|x| x.feeder.done.val() & x.reader.done.val(), x)?;
        wait_clock_cycle!(sim, clock, x);
        sim_assert!(sim, !x.reader.error.val(), x);
        sim.done(x)
    });
    let mut vcd = vec![];
    let ret = sim.run_traced(Box::new(uut), 100_000, &mut vcd);
    std::fs::write(vcd_path!("reducer_hls.vcd"), vcd).unwrap();
    ret.unwrap();
}

#[derive(LogicBlock)]
struct ExpanderTestFixture {
    feeder: LazyFIFOFeeder<Bits<4>, 12>,
    nibble_fifo: SyncFIFO<Bits<4>, 4, 5, 1>,
    expander: Expander<4, 16>,
    word_fifo: SyncFIFO<Bits<16>, 4, 5, 1>,
    reader: LazyFIFOReader<Bits<16>, 10>,
    clock: Signal<In, Clock>,
}

impl Logic for ExpanderTestFixture {
    #[hdl_gen]
    fn update(&mut self) {
        self.feeder.clock.next = self.clock.val();
        self.nibble_fifo.clock.next = self.clock.val();
        self.expander.clock.next = self.clock.val();
        self.word_fifo.clock.next = self.clock.val();
        self.reader.clock.next = self.clock.val();
        self.feeder.bus.join(&mut self.nibble_fifo.bus_write);
        self.expander.bus_read.join(&mut self.nibble_fifo.bus_read);
        self.word_fifo.bus_write.join(&mut self.expander.bus_write);
        self.word_fifo.bus_read.join(&mut self.reader.bus);
    }
}

impl Default for ExpanderTestFixture {
    fn default() -> Self {
        let data1 = (0..256)
            .map(|_| Bits::<16>::from(rand::thread_rng().gen::<u16>()))
            .collect::<Vec<_>>();
        let mut data2 = vec![];
        for x in &data1 {
            for offset in &[0, 4, 8, 12] {
                data2.push(x.get_bits::<4>(*offset));
            }
        }
        Self {
            feeder: LazyFIFOFeeder::new(&data2, &bursty_vec(1024)),
            nibble_fifo: Default::default(),
            expander: Expander::new(WordOrder::LeastSignificantFirst),
            word_fifo: Default::default(),
            reader: LazyFIFOReader::new(&data1, &bursty_vec(256)),
            clock: Default::default(),
        }
    }
}

#[test]
fn test_expander_test_fixture() {
    let mut uut = ExpanderTestFixture::default();
    uut.clock.connect();
    uut.feeder.start.connect();
    uut.reader.start.connect();
    uut.connect_all();
    let vlog = generate_verilog(&uut);
    yosys_validate("expander_hls_test", &vlog).unwrap();
}

#[test]
fn test_expander_test_fixture_operation() {
    let mut uut = ExpanderTestFixture::default();
    uut.clock.connect();
    uut.feeder.start.connect();
    uut.reader.start.connect();
    uut.connect_all();
    let mut sim = Simulation::new();
    sim.add_clock(5, |x: &mut Box<ExpanderTestFixture>| {
        x.clock.next = !x.clock.val()
    });
    sim.add_testbench(move |mut sim: Sim<ExpanderTestFixture>| {
        let mut x = sim.init()?;
        wait_clock_true!(sim, clock, x);
        x.feeder.start.next = true;
        x.reader.start.next = true;
        wait_clock_cycle!(sim, clock, x);
        x.feeder.start.next = false;
        x.reader.start.next = false;
        x = sim.watch(|x| x.feeder.done.val() & x.reader.done.val(), x)?;
        wait_clock_cycle!(sim, clock, x);
        sim_assert!(sim, !x.reader.error.val(), x);
        sim.done(x)
    });
    let mut vcd = vec![];
    let ret = sim.run_traced(Box::new(uut), 100_000, &mut vcd);
    std::fs::write(vcd_path!("expander_hls.vcd"), vcd).unwrap();
    ret.unwrap();
}