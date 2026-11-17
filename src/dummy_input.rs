//! Port of `DummyInput.java`: a `ClientInput` whose moveVector is `Vec2.ZERO`.

/// Upstream seam: `net.minecraft.world.phys.Vec2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
}

/// Upstream `DummyInput extends ClientInput`: only `moveVector` is set
/// (to `Vec2.ZERO`); the rest stays at the base defaults.
#[derive(Clone, Debug)]
pub struct DummyInput {
    pub move_vector: Vec2,
}

impl Default for DummyInput {
    fn default() -> Self {
        DummyInput { move_vector: Vec2::ZERO }
    }
}
