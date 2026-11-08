//! Where boxes, ports and wires are, in world units.

use egui::{Pos2, Rect, Vec2};

use super::model::{BlockRef, Builtin};

/// Vertical distance between neighbouring ports.
pub const PORT_PITCH: f32 = 20.0;
/// Hit radius of a port, in screen points.
pub const PORT_HIT: f32 = 8.0;

/// Pins expose only their inner side on the canvas: an input pin is a
/// source of light inside the schematic, an output pin a sink.
pub fn is_input_pin(block: &BlockRef) -> bool {
    matches!(block, BlockRef::Builtin(Builtin::InputPin { .. }))
}

pub fn is_output_pin(block: &BlockRef) -> bool {
    matches!(block, BlockRef::Builtin(Builtin::OutputPin { .. }))
}

/// Ports drawn on the canvas, given the block's real port counts.
pub fn visible_ports(block: &BlockRef, inputs: usize, outputs: usize) -> (usize, usize) {
    if is_input_pin(block) {
        (0, 1)
    } else if is_output_pin(block) {
        (1, 0)
    } else {
        (inputs, outputs)
    }
}

/// A placed box: its outline and the centres of its visible ports.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub rect: Rect,
    pub inputs: Vec<Pos2>,
    pub outputs: Vec<Pos2>,
}

pub fn box_size(inputs: usize, outputs: usize, pin: bool) -> Vec2 {
    if pin {
        Vec2::new(64.0, 24.0)
    } else {
        let rows = inputs.max(outputs).max(1) as f32;
        Vec2::new(72.0, rows * PORT_PITCH + 20.0)
    }
}

pub fn shape(top_left: Pos2, block: &BlockRef, inputs: usize, outputs: usize) -> Shape {
    let (inputs, outputs) = visible_ports(block, inputs, outputs);
    let pin = is_input_pin(block) || is_output_pin(block);
    let rect = Rect::from_min_size(top_left, box_size(inputs, outputs, pin));
    let column = |x: f32, count: usize| {
        (0..count)
            .map(|k| {
                Pos2::new(
                    x,
                    rect.center().y + (k as f32 - (count as f32 - 1.0) / 2.0) * PORT_PITCH,
                )
            })
            .collect()
    };
    Shape {
        inputs: column(rect.left(), inputs),
        outputs: column(rect.right(), outputs),
        rect,
    }
}

/// Control points of the cubic Bézier drawn for a wire.
pub fn wire_curve(from: Pos2, to: Pos2) -> [Pos2; 4] {
    let reach = ((to.x - from.x).abs() * 0.5).max(40.0);
    [
        from,
        from + Vec2::new(reach, 0.0),
        to - Vec2::new(reach, 0.0),
        to,
    ]
}

pub fn bezier(p: &[Pos2; 4], t: f32) -> Pos2 {
    let u = 1.0 - t;
    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
    Pos2::new(
        p.iter().zip(w).map(|(p, w)| p.x * w).sum(),
        p.iter().zip(w).map(|(p, w)| p.y * w).sum(),
    )
}

/// Points along the wire, for drawing and hit testing.
pub fn wire_polyline(from: Pos2, to: Pos2, segments: usize) -> Vec<Pos2> {
    let curve = wire_curve(from, to);
    (0..=segments)
        .map(|k| bezier(&curve, k as f32 / segments as f32))
        .collect()
}

pub fn wire_length(from: Pos2, to: Pos2) -> f32 {
    wire_polyline(from, to, 32)
        .windows(2)
        .map(|w| w[0].distance(w[1]))
        .sum()
}

pub fn distance_to_wire(p: Pos2, from: Pos2, to: Pos2) -> f32 {
    wire_polyline(from, to, 24)
        .windows(2)
        .map(|w| crate::panels::optics::render::distance_to_segment(p, w[0], w[1]))
        .fold(f32::INFINITY, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> BlockRef {
        BlockRef::Builtin(Builtin::Coupler { coupling: 0.5 })
    }

    #[test]
    fn ports_are_centred_on_the_edges() {
        let s = shape(Pos2::new(10.0, 20.0), &block(), 2, 3);
        assert_eq!(s.rect.size(), Vec2::new(72.0, 80.0));
        assert_eq!(s.inputs.len(), 2);
        assert_eq!(s.outputs.len(), 3);
        assert!(s.inputs.iter().all(|p| p.x == s.rect.left()));
        assert!(s.outputs.iter().all(|p| p.x == s.rect.right()));
        assert_eq!(s.outputs[1].y, s.rect.center().y);
        assert_eq!(s.inputs[1].y - s.inputs[0].y, PORT_PITCH);
        assert_eq!((s.inputs[0].y + s.inputs[1].y) / 2.0, s.rect.center().y);
    }

    #[test]
    fn pins_show_only_their_inner_port() {
        let input = BlockRef::Builtin(Builtin::InputPin { name: "a".into() });
        let output = BlockRef::Builtin(Builtin::OutputPin { name: "b".into() });
        let s = shape(Pos2::ZERO, &input, 1, 1);
        assert_eq!((s.inputs.len(), s.outputs.len()), (0, 1));
        assert_eq!(s.rect.size(), Vec2::new(64.0, 24.0));
        let s = shape(Pos2::ZERO, &output, 1, 1);
        assert_eq!((s.inputs.len(), s.outputs.len()), (1, 0));
        assert!(is_input_pin(&input) && !is_input_pin(&output) && is_output_pin(&output));
        assert_eq!(visible_ports(&block(), 3, 4), (3, 4));
    }

    #[test]
    fn empty_boxes_still_have_a_row() {
        assert_eq!(box_size(0, 0, false), Vec2::new(72.0, 40.0));
    }

    #[test]
    fn wire_curves() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(200.0, 0.0));
        assert_eq!(bezier(&wire_curve(a, b), 0.0), a);
        assert_eq!(bezier(&wire_curve(a, b), 1.0), b);
        assert!(
            (wire_length(a, b) - 200.0).abs() < 1e-3,
            "straight when aligned"
        );
        let c = Pos2::new(200.0, 100.0);
        assert!(wire_length(a, c) > a.distance(c));
        assert!(distance_to_wire(Pos2::new(100.0, 3.0), a, b) < 3.5);
        assert!(distance_to_wire(Pos2::new(100.0, 50.0), a, b) > 40.0);
        // Backwards wires bend out and come back.
        let back = wire_polyline(Pos2::new(100.0, 0.0), Pos2::new(0.0, 50.0), 10);
        assert!(back.iter().any(|p| p.x > 100.0));
    }
}
