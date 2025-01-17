use rust_hdl_lib_core::prelude::*;
use rust_hdl_lib_widgets::prelude::*;

#[derive(LogicBlock)]
pub struct EdgeTristateBufferDelayed<T: Synth> {
    pub to_pin: Signal<In, T>,
    pub from_pin: Signal<Out, T>,
    pub output_enable: Signal<In, Bit>,
    pub clock: Signal<In, Clock>,
    pub reset: Signal<In, Bit>,
    pub pin: Signal<InOut, T>,
    dff_out: DFF<T>,
    dff_in: DFF<T>,
    buffer: TristateBuffer<T>,
    _delay: u8,
}

impl<T: Synth> EdgeTristateBufferDelayed<T> {
    pub fn new(delay: u8) -> Self {
        Self {
            to_pin: Default::default(),
            from_pin: Default::default(),
            output_enable: Default::default(),
            clock: Default::default(),
            reset: Default::default(),
            pin: Default::default(),
            dff_out: Default::default(),
            dff_in: Default::default(),
            buffer: Default::default(),
            _delay: delay,
        }
    }
}

fn wrapper_once(delay: u8) -> String {
    format!(
        r##"
    wire bb_to_pin;
    wire bb_from_pin_a;
    wire bb_from_pin_z;

    OFS1P3DX obuf(.D(to_pin), .CD(reset), .SP(1'b1), .SCLK(clock), .Q(bb_to_pin));
    IFS1P3DX ibuf(.D(bb_from_pin_z), .CD(reset), .SP(1'b1), .SCLK(clock), .Q(from_pin));
    BB bb(.I(bb_to_pin), .O(bb_from_pin_a), .B(pin), .T(~output_enable));

    defparam dg.DEL_VALUE = {delay_from_pin};
    defparam dg.DEL_MODE = "USER_DEFINED";
    DELAYG dg(.A(bb_from_pin_a),.Z(bb_from_pin_z));
"##,
        delay_from_pin = delay
    )
}

fn wrapper_multiple(count: usize, delay: u8) -> String {
    let bufs = (0..count)
        .map(|x| {
            format!(
                r#"
    OFS1P3DX obuf_{x}(.D(to_pin[{x}]), .CD(reset), .SP(1'b1), .SCLK(clock), .Q(bb_to_pin[{x}]));
    IFS1P3DX ibuf_{x}(.D(bb_from_pin_z[{x}]), .CD(reset), .SP(1'b1), .SCLK(clock), .Q(from_pin[{x}]));
    BB bb_{x}(.I(bb_to_pin[{x}]), .O(bb_from_pin_a[{x}]), .B(pin[{x}]), .T(~output_enable));

    defparam dg_from_pin_{x}.DEL_VALUE = {delay_from_pin};
    defparam dg_from_pin_{x}.DEL_MODE = "USER_DEFINED";
    DELAYG dg_from_pin_{x}(.A(bb_from_pin_a[{x}]),.Z(bb_from_pin_z[{x}]));
        "#,
                x = x,
                delay_from_pin = delay
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r##"
wire [{B}:0] bb_to_pin;
wire [{B}:0] bb_from_pin_a;
wire [{B}:0] bb_from_pin_z;

{bufs}
    "##,
        B = count,
        bufs = bufs
    )
}

impl<T: Synth> Logic for EdgeTristateBufferDelayed<T> {
    fn update(&mut self) {
        dff_setup!(self, clock, dff_out, dff_in);
        self.buffer.write_enable.next = self.output_enable.val();
        self.dff_in.d.next = self.buffer.read_data.val();
        self.dff_out.d.next = self.to_pin.val();
        self.buffer.write_data.next = self.dff_out.q.val();
        self.from_pin.next = self.dff_in.q.val();
        Signal::<InOut, T>::link(&mut self.pin, &mut self.buffer.bus);
    }
    fn connect(&mut self) {
        self.dff_out.clock.connect();
        self.dff_in.clock.connect();
        self.buffer.write_enable.connect();
        self.dff_in.d.connect();
        self.dff_out.d.connect();
        self.buffer.write_data.connect();
        self.from_pin.connect();
    }
    fn hdl(&self) -> Verilog {
        Verilog::Wrapper(Wrapper {
            code: if T::BITS == 1 {
                wrapper_once(self._delay).to_string()
            } else {
                wrapper_multiple(T::BITS, self._delay)
            },
            cores: r##"
(* blackbox *)
module IFS1P3DX(input D, input SP, input SCLK, input CD, output Q);
endmodule

(* blackbox *)
module OFS1P3DX(input D, input SP, input SCLK, input CD, output Q);
endmodule

(* blackbox *)
module BB(input I, input T, output O, inout B);
endmodule

(* blackbox *)
module DELAYG(input A, output Z);
parameter DEL_MODE = "USER_DEFINED";
parameter DEL_VALUE = 0;
endmodule

            "##
            .into(),
        })
    }
}

#[test]
fn test_edge_buffer_synthesizes() {
    let mut uut = EdgeTristateBufferDelayed::<Bits<8>>::new(10);
    uut.connect_all();
    yosys_validate("edge_tristate_buffer_delayed", &generate_verilog(&uut)).unwrap();
}
