//! AutoLectern (Horizon Rust port), slice 1: the core.
//!
//! 1:1 port of upstream `src/main/java/sys/exe/al/` (capthehacker99/AutoLectern,
//! CC-BY-NC-SA-4.0). The upstream license and credit carry with this port.
//! Game-facing calls (world access, interactions, chat, sounds) land behind the
//! same seams upstream uses, injected so the logic itself stays exact.
//!
//! Slice 1: `ALState`, `ALGoal`, `ALAutoTrade`, `DummyInput`,
//! `interfaces/ExtraVillagerData`, and the `AutoLectern` main class (state
//! machine, goal matching, config load/save).
//! Slice 2 (next): `commands/AutoLec`, `commands/ClientCommandManager`,
//! `commands/FakeCommandSource`, and the five mixins.
pub mod state;
pub mod goal;
pub mod dummy_input;
pub mod interfaces;
pub mod seams;
pub mod auto_lectern;
pub mod commands_seam;
pub mod mixins;
#[cfg(test)]
mod tests;

pub use auto_lectern::AutoLectern;
pub use goal::ALGoal;
pub use state::{ALAutoTrade, ALState};
