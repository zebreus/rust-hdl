use druid::kurbo::BezPath;
use druid::{kurbo::Line, Affine, AppLauncher, BoxConstraints, Color, Data, Env, Event, EventCtx, FontDescriptor, FontFamily, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, RenderContext, Size, TextAlignment, TextLayout, UpdateCtx, Widget, WidgetId, WindowDesc, KbKey, Cursor};
use rust_hdl_pcb::adc::make_ads868x;
use rust_hdl_pcb_core::prelude::*;
use std::sync::{Arc, Mutex};

#[derive(Data, Clone)]
struct Schematic {
    circuit: Arc<Circuit>,
    layout: Arc<Mutex<SchematicLayout>>,
    partial_net: Arc<Vec<(f64, f64)>>,
    center: (f64, f64),
    cursor: (f64, f64),
    size: Size,
    scale: f64,
    selected: Option<String>,
    snap_point: Option<(f64, f64)>,
    wire_mode: bool,
}

impl Schematic {
    // Map document coordinates to mouse coordinates
    pub fn to_screen(&self, pos: (f64, f64)) -> druid::kurbo::Point {
        let mut px = pos.0;
        let mut py = pos.1;
        // Flip the y axis
        py *= -1.0;
        // Scale by the scale factor
        px *= self.scale;
        py *= self.scale;
        // Translate to the center
        px += self.center.0;
        py += self.center.1;
        druid::kurbo::Point { x: px, y: py }
    }

    // Map mouse coordinates to document coordinates
    pub fn from_screen(&self, point: druid::kurbo::Point) -> (f64, f64) {
        let mut px = point.x;
        let mut py = point.y;
        px -= self.center.0;
        py -= self.center.1;
        px /= self.scale;
        py /= self.scale;
        py *= -1.0;
        (px, py)
    }

    pub fn shift_selected(&mut self, delta: (f64, f64)) {
        if let Some(id) = &self.selected {
            let mut layout = self.layout.lock().unwrap();
            let mut schematic_orientation = layout.part(id);
            schematic_orientation.center.0 += (delta.0 / self.scale) as i32;
            schematic_orientation.center.1 += (-delta.1 / self.scale) as i32;
            layout.set_part(id, schematic_orientation);
        }
    }

    pub fn snap_selected(&mut self) {
        if let Some(id) = &self.selected {
            let mut layout = self.layout.lock().unwrap();
            let mut schematic_orientation = layout.part(id);
            schematic_orientation.center.0 = (schematic_orientation.center.0 / 100) * 100;
            schematic_orientation.center.1 = (schematic_orientation.center.1 / 100) * 100;
            layout.set_part(id, schematic_orientation);
        }
    }

    pub fn orient_selected(&mut self, selector: &str) {
        if let Some(id) = &self.selected {
            let mut layout = self.layout.lock().unwrap();
            let mut schematic_orientation = layout.part(id);
            if selector == " " {
                schematic_orientation.rotation = if schematic_orientation.rotation == SchematicRotation::Vertical {
                    SchematicRotation::Horizontal
                } else {
                    SchematicRotation::Vertical
                };
            }
            if selector == "x" {
                schematic_orientation.flipped_lr = !schematic_orientation.flipped_lr;
            }
            if selector == "y" {
                schematic_orientation.flipped_ud = !schematic_orientation.flipped_ud;
            }
            layout.set_part(id, schematic_orientation);
        }
    }

    pub fn highlight_snap_points(& mut self, mouse: druid::kurbo::Point) -> Option<(f64, f64)> {
        for net in &self.circuit.nets {
            let ports = net.pins
                .iter()
                .map(|x| get_pin_net_location(&self.circuit, &self.layout.lock().unwrap(), x))
                .collect::<Vec<_>>();
            for p in ports {
                let port_point = (p.0 as f64, -p.1 as f64);
                let port_screen = self.to_screen(port_point);
                if (port_screen.x - mouse.x).abs().max(
                    (port_screen.y - mouse.y).abs()) < 10.0 {
                    return Some(port_point)
                }
            }
        }
        None
    }

    pub fn hit_test(&self, pos: (f64, f64)) -> Option<String> {
        let layout = self.layout.lock().unwrap();
        for instance in &self.circuit.nodes {
            let part = get_details_from_instance(instance, &layout);
            let outline = &part.outline;
            if outline.len() != 0 {
                if let Glyph::OutlineRect(r) = &outline[0] {
                    // Get the center of this part
                    let schematic_orientation = layout.part(&instance.id);
                    let cx = schematic_orientation.center.0 as f64;
                    let cy = schematic_orientation.center.1 as f64;
                    let corners = if schematic_orientation.rotation == SchematicRotation::Horizontal {
                        (
                            (r.p0.x as f64 + cx, r.p0.y as f64 + cy),
                            (r.p1.x as f64 + cx, r.p1.y as f64 + cy),
                        )
                    } else {
                        (
                            (-r.p0.y as f64 + cx, r.p0.x as f64 + cy),
                            (-r.p1.y as f64 + cx, r.p1.x as f64 + cy),
                        )
                    };
                    let p1 = self.to_screen(corners.0);
                    let p2 = self.to_screen(corners.1);
                    let dr = druid::kurbo::Rect::from((p1, p2));
                    if dr.contains(pos.into()) {
                        return Some(instance.id.clone());
                    }
                }
            }
        }
        None
    }
}

struct SchematicViewer;

impl Widget<Schematic> for SchematicViewer {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut Schematic, _env: &Env) {
        ctx.request_focus();
        match event {
            Event::MouseDown(mouse) => {
                ctx.set_active(true);
                data.cursor = (mouse.pos.x, mouse.pos.y);
                if !data.wire_mode {
                    data.selected = data.hit_test(mouse.pos.into());
                } else {
                    let mut y = data.partial_net.iter().map(|x| x.clone()).collect::<Vec<_>>();
                    if let Some(snap) = data.snap_point {
                        y.push(snap);
                    } else {
                        y.push(data.from_screen(mouse.pos));
                    }
                    data.partial_net = Arc::new(y);
                }
                ctx.request_paint();
            }
            Event::MouseUp(mouse) => {
                ctx.set_active(false);
                data.snap_selected();
                data.selected = None;
                ctx.request_paint();
            }
            Event::MouseMove(mouse) => {
                if ctx.is_active() {
                    if data.selected.is_none() {
                        data.center.0 += (mouse.pos.x - data.cursor.0);
                        data.center.1 += (mouse.pos.y - data.cursor.1);
                    } else {
                        data.shift_selected((
                            mouse.pos.x - data.cursor.0,
                            mouse.pos.y - data.cursor.1,
                        ));
                    }
                    data.cursor = (mouse.pos.x, mouse.pos.y);
                    ctx.request_paint();
                } else if data.wire_mode {
                    let pt = data.highlight_snap_points(mouse.pos);
                    if data.snap_point != pt {
                        data.snap_point = pt;
                        ctx.request_paint();
                    }
                }
            }
            Event::Wheel(mouse) => {
                let old_scale = data.scale;
                data.scale *= (1.0 - mouse.wheel_delta.y / 100.0).max(0.9).min(1.1);
                // Move the center of the page to zoom around the mouse
                data.center.0 += (data.center.0 - mouse.pos.x) * (data.scale - old_scale) / old_scale;
                data.center.1 += (data.center.1 - mouse.pos.y) * (data.scale - old_scale) / old_scale;
                ctx.request_paint();
            }
            Event::KeyDown(key) => {
                if ctx.is_active() && data.selected.is_some() {
                    data.orient_selected(&key.key.to_string());
                } else {
                    if key.key.to_string() == "w" {
                        data.wire_mode = true;
                    }
                    if key.key == KbKey::Escape {
                        data.wire_mode = false;
                    }
                }
                ctx.request_paint();
            }
            Event::Zoom(x) => {
                //dbg!(x);
            }
            _ => (),
        }
        ctx.set_cursor(if data.wire_mode {
            &Cursor::Crosshair
        } else {
            &Cursor::Arrow
        });
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &Schematic,
        env: &Env,
    ) {
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &Schematic, data: &Schematic, env: &Env) {}

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &Schematic,
        env: &Env,
    ) -> Size {
        if bc.is_width_bounded() && bc.is_height_bounded() {
            bc.max()
        } else {
            let size = Size::new(100.0, 100.0);
            bc.constrain(size)
        }
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &Schematic, env: &Env) {
        let size = ctx.size();
        let rect = size.to_rect();
        // Clear the canvas
        ctx.fill(rect, &Color::from_hex_str("FFFCF8").unwrap());
        //dbg!(data.cursor);
        ctx.transform(Affine::translate(data.center));
        ctx.transform(Affine::scale(data.scale));
        ctx.transform(Affine::scale_non_uniform(1.0, -1.0));
        let layout = data.layout.lock().unwrap();
        for instance in &data.circuit.nodes {
            let part = get_details_from_instance(instance, &layout);
            let schematic_orientation = layout.part(&instance.id);
            ctx.with_save(|ctx| {
                if schematic_orientation.rotation == SchematicRotation::Vertical {
                    ctx.transform(Affine::translate((
                        schematic_orientation.center.0 as f64,
                        schematic_orientation.center.1 as f64,
                    )));
                    ctx.transform(Affine::rotate(std::f64::consts::PI / 2.0));
                } else {
                    ctx.transform(Affine::translate((
                        schematic_orientation.center.0 as f64,
                        schematic_orientation.center.1 as f64,
                    )));
                }
                let is_selected = if let Some(k) = &data.selected {
                    k.eq(&instance.id)
                } else {
                    false
                };
                for path in &part.outline {
                    render_glyph(ctx, path, part.hide_part_outline, env, is_selected);
                }
                for (num, pin) in &part.pins {
                    render_pin(ctx, num, pin, &part.outline, part.hide_pin_designators, env);
                }
            });
            let mut path = BezPath::new();
            let mut rat_nest = false;
            for net in &data.circuit.nets {
                let ports = net
                    .pins
                    .iter()
                    .map(|x| get_pin_net_location(&data.circuit, &layout, x))
                    .collect::<Vec<_>>();
                // Walk the layout
                let mut net_layout = layout.net(&net.name);
                if net_layout.len() == 0 {
                    net_layout = make_rat_layout(ports.len());
                    rat_nest = true;
                }
                let mut lp = (0.0, 0.0);
                for cmd in net_layout {
                    match cmd {
                        NetLayoutCmd::MoveToPort(n) => {
                            lp = (ports[n - 1].0 as f64, -ports[n - 1].1 as f64);
                            path.move_to(lp);
                        }
                        NetLayoutCmd::LineToPort(n) => {
                            lp = (ports[n - 1].0 as f64, -ports[n - 1].1 as f64);
                            path.line_to(lp);
                        }
                        NetLayoutCmd::MoveToCoords(x, y) => {
                            lp = (x as f64, y as f64);
                            path.move_to(lp);
                        }
                        NetLayoutCmd::LineToCoords(x, y) => {
                            lp = (x as f64, y as f64);
                            path.line_to(lp);
                        }
                        NetLayoutCmd::Junction => {
                            let disk = druid::kurbo::Circle::new(lp, 25.0);
                            ctx.fill(disk, &Color::from_hex_str("000080").unwrap());
                        }
                    }
                }
            }
            ctx.stroke(
                path,
                &Color::from_hex_str("000080").unwrap(),
                if rat_nest { 1.0 } else { 10.0 },
            );
            let mut path = BezPath::new();
            if data.partial_net.len() != 0 {
                path.move_to(data.partial_net[0]);
                for n in 1..data.partial_net.len() {
                    path.line_to(data.partial_net[n]);
                }
            }
            ctx.stroke(
                path,
                &Color::from_hex_str("7F0000").unwrap(),
                10.0);
            if let Some(p) = data.snap_point {
                let disk = druid::kurbo::Circle::new(p, 20.0);
                ctx.stroke(disk, &Color::from_hex_str("101010").unwrap(), 1.0);
            }
        }
    }
}

fn map_point(p: &Point) -> druid::Point {
    druid::Point {
        x: p.x as f64,
        y: p.y as f64,
    }
}

const EM: i32 = 85;

fn render_pin(
    ctx: &mut PaintCtx,
    num: &u64,
    pin: &EPin,
    outline: &[Glyph],
    hide_pin_designators: bool,
    env: &Env,
) {
    if outline.len() == 0 {
        return;
    }
    if let Glyph::OutlineRect(r) = &outline[0] {
        if r.is_empty() {
            return;
        }
        match pin.location.edge {
            EdgeLocation::North => {
                if !hide_pin_designators {
                    render_text(
                        ctx,
                        &format!("{}", num),
                        "000000",
                        TextJustification::BottomLeft,
                        80.0,
                        Point {
                            x: pin.location.offset - 15,
                            y: r.p0.y.max(r.p1.y) + 50,
                        },
                        env,
                        true,
                    );
                }
                render_text(
                    ctx,
                    &pin.name,
                    "000000",
                    TextJustification::MiddleRight,
                    80.0,
                    Point {
                        x: pin.location.offset,
                        y: r.p1.y - EM,
                    },
                    env,
                    true,
                );
                render_line(
                    ctx,
                    Point {
                        x: pin.location.offset,
                        y: r.p1.y,
                    },
                    Point {
                        x: pin.location.offset,
                        y: r.p1.y + PIN_LENGTH,
                    },
                    "000000",
                    10.0,
                );
            }
            EdgeLocation::West => {
                if !hide_pin_designators {
                    render_text(
                        ctx,
                        &format!("{}", num),
                        "000000",
                        TextJustification::BottomRight,
                        80.0,
                        Point {
                            x: r.p0.x - 85,
                            y: pin.location.offset + 15,
                        },
                        env,
                        false,
                    );
                }
                render_text(
                    ctx,
                    &pin.name,
                    "000000",
                    TextJustification::MiddleLeft,
                    80.0,
                    Point {
                        x: r.p0.x + EM,
                        y: pin.location.offset,
                    },
                    env,
                    false,
                );
                render_line(
                    ctx,
                    Point {
                        x: r.p0.x,
                        y: pin.location.offset,
                    },
                    Point {
                        x: r.p0.x - PIN_LENGTH,
                        y: pin.location.offset,
                    },
                    "000000",
                    10.0,
                );
            }
            EdgeLocation::South => {
                if !hide_pin_designators {
                    render_text(
                        ctx,
                        &format!("{}", num),
                        "000000",
                        TextJustification::BottomRight,
                        80.0,
                        Point {
                            x: pin.location.offset - 15,
                            y: r.p0.y.min(r.p1.y) - 50,
                        },
                        env,
                        true,
                    );
                }
                render_text(
                    ctx,
                    &pin.name,
                    "000000",
                    TextJustification::MiddleLeft,
                    80.0,
                    Point {
                        x: pin.location.offset,
                        y: r.p0.y + EM,
                    },
                    env,
                    true,
                );
                render_line(
                    ctx,
                    Point {
                        x: pin.location.offset,
                        y: r.p0.y,
                    },
                    Point {
                        x: pin.location.offset,
                        y: r.p0.y - PIN_LENGTH,
                    },
                    "000000",
                    10.0,
                );
            }
            EdgeLocation::East => {
                if !hide_pin_designators {
                    render_text(
                        ctx,
                        &format!("{}", num),
                        "000000",
                        TextJustification::BottomLeft,
                        80.0,
                        Point {
                            x: r.p1.x + 85,
                            y: pin.location.offset + 15,
                        },
                        env,
                        false,
                    );
                }
                render_text(
                    ctx,
                    &pin.name,
                    "000000",
                    TextJustification::MiddleRight,
                    80.0,
                    Point {
                        x: r.p1.x - EM,
                        y: pin.location.offset,
                    },
                    env,
                    false,
                );
                render_line(
                    ctx,
                    Point {
                        x: r.p1.x,
                        y: pin.location.offset,
                    },
                    Point {
                        x: r.p1.x + PIN_LENGTH,
                        y: pin.location.offset,
                    },
                    "000000",
                    10.0,
                );
            }
        }
    }
}

fn render_line(ctx: &mut PaintCtx, start: Point, end: Point, color: &str, width: f64) {
    let line = druid::kurbo::Line {
        p0: map_point(&start),
        p1: map_point(&end),
    };
    let stroke_color = Color::from_hex_str(color).unwrap();
    ctx.stroke(line, &stroke_color, width);
}

fn render_text(
    ctx: &mut PaintCtx,
    t: &str,
    color: &str,
    justify: TextJustification,
    size: f64,
    at: Point,
    env: &Env,
    is_vert: bool,
) {
    let mut layout = TextLayout::<String>::from_text(t);
    match justify {
        TextJustification::BottomLeft
        | TextJustification::TopLeft
        | TextJustification::MiddleLeft => layout.set_text_alignment(TextAlignment::Start),
        TextJustification::BottomRight
        | TextJustification::TopRight
        | TextJustification::MiddleRight => layout.set_text_alignment(TextAlignment::End),
    }
    let stroke_color = Color::from_hex_str(color).unwrap();
    layout.set_text_color(stroke_color);
    layout.set_font(FontDescriptor::new(FontFamily::MONOSPACE).with_size(size));
    layout.rebuild_if_needed(ctx.text(), env);
    let baseline = layout.layout_metrics().size.height;
    let width = layout.layout_metrics().size.width;
    ctx.with_save(|ctx| {
        ctx.transform(Affine::scale_non_uniform(1.0, -1.0));
        if is_vert {
            ctx.transform(Affine::translate((at.x as f64, -at.y as f64)));
            ctx.transform(Affine::rotate(-std::f64::consts::PI / 2.0));
            ctx.transform(Affine::translate((-at.x as f64, at.y as f64)));
        }
        match justify {
            TextJustification::TopRight => ctx.transform(Affine::translate((-width, 0.0))),
            TextJustification::TopLeft => {}
            TextJustification::MiddleRight => {
                ctx.transform(Affine::translate((-width, 0.0)));
                ctx.transform(Affine::translate((0.0, -baseline / 2.0)))
            }
            TextJustification::MiddleLeft => {
                ctx.transform(Affine::translate((0.0, -baseline / 2.0)))
            }
            TextJustification::BottomLeft => ctx.transform(Affine::translate((0.0, -baseline))),
            TextJustification::BottomRight => {
                ctx.transform(Affine::translate((-width, 0.0)));
                ctx.transform(Affine::translate((0.0, -baseline)))
            }
        }
        layout.draw(
            ctx,
            druid::Point {
                x: at.x as f64,
                y: -at.y as f64,
            },
        );
    });
}

fn render_glyph(ctx: &mut PaintCtx, g: &Glyph, hide_outline: bool, env: &Env, is_selected: bool) {
    match g {
        Glyph::OutlineRect(r) => {
            if !hide_outline {
                let rect =
                    druid::Rect::new(r.p0.x as f64, r.p0.y as f64, r.p1.x as f64, r.p1.y as f64);
                let stroke_color = if !is_selected {
                    Color::from_hex_str("AE5E46").unwrap()
                } else {
                    Color::from_hex_str("00FF00").unwrap()
                };
                let fill_color = if !is_selected {
                    Color::from_hex_str("FFFDB0").unwrap()
                } else {
                    Color::from_hex_str("7F7D30").unwrap()
                };
                ctx.stroke(rect, &stroke_color, 5.0);
                ctx.fill(rect, &fill_color);
            }
        }
        Glyph::Line(l) => {
            render_line(ctx, l.p0, l.p1, "0433FF", 10.0);
        }
        Glyph::Text(t) => {
            render_text(ctx, &t.text, "0433FF", t.justify, 80.0, t.p0, env, false);
        }
        Glyph::Arc(_) => {}
        Glyph::Circle(_) => {}
    }
}

fn make_root() -> impl Widget<Schematic> {
    SchematicViewer {}
}

pub fn main() {
    let window = WindowDesc::new(make_root())
        .window_size(Size {
            width: 800.0,
            height: 800.0,
        })
        .resizable(true)
        .title("Schematic Viewer");
    let (mut circuit, mut layout) = rust_hdl_pcb::schematic_manual_layout::test_ldo_circuit();
    circuit
        .nodes
        .push(make_ads868x("ADS8681IPW").instance("adc"));
    layout.set_part("adc", orient().center(4000, 4000));
    AppLauncher::with_window(window)
        .log_to_console()
        .launch(Schematic {
            circuit: Arc::new(circuit),
            layout: Arc::new(Mutex::new(SchematicLayout::default())),
            partial_net: Arc::new(vec![]),
            center: (0.0, 0.0),
            cursor: (0.0, 0.0),
            size: Size {
                width: 800.0,
                height: 800.0,
            },
            scale: 0.2,
            selected: None,
            snap_point: None,
            wire_mode: false
        })
        .expect("launch failed");
}