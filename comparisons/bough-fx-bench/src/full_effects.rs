//! Fixture contract for the pending author seam. Backends must refuse the cell unless all operations are admitted.
#[allow(dead_code)] // Read by the adapter when the pending seam is implemented.
#[derive(Debug)]
pub enum Operation {
    CanvasPath {
        cubic: [[f32; 2]; 4],
        fill_even_odd: bool,
    },
    RoundedMask {
        radius: f32,
        opacity: f32,
    },
    SubtreeFilters {
        sigma: f32,
        saturate: f32,
        brightness: f32,
        shadow_sigma: f32,
        offset: [f32; 2],
    },
    AuthorPaint {
        entry: &'static str,
    },
    Spring {
        mass: f32,
        stiffness: f32,
        damping: f32,
    },
    Video {
        width: u16,
        height: u16,
        frames: u16,
        fps: u8,
    },
    Backdrop {
        size: [f32; 2],
        radius: f32,
        sigma: f32,
    },
}

pub const OPERATIONS: &[Operation] = &[
    Operation::CanvasPath {
        cubic: [[9.0, 9.0], [68.0, 2.0], [10.0, 92.0], [80.0, 70.0]],
        fill_even_odd: true,
    },
    Operation::RoundedMask {
        radius: 12.0,
        opacity: 0.7,
    },
    Operation::SubtreeFilters {
        sigma: 3.0,
        saturate: 1.4,
        brightness: 0.8,
        shadow_sigma: 4.0,
        offset: [3.0, 5.0],
    },
    Operation::AuthorPaint {
        entry: "hash_noise_plasma",
    },
    Operation::AuthorPaint {
        entry: "circle_difference_sdf",
    },
    Operation::AuthorPaint {
        entry: "time_sine_ramp",
    },
    Operation::Spring {
        mass: 1.0,
        stiffness: 260.0,
        damping: 23.0,
    },
    Operation::Video {
        width: 256,
        height: 144,
        frames: 120,
        fps: 30,
    },
    Operation::Backdrop {
        size: [112.0, 76.0],
        radius: 12.0,
        sigma: 6.0,
    },
];
