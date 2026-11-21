//! AutoLectern (Horizon Rust port).
//!
//! 1:1 port of upstream `src/main/java/sys/exe/al/` (capthehacker99/AutoLectern,
//! CC-BY-NC-SA-4.0). The upstream license and credit carry with this port.
//! Game-facing calls (world access, interactions, chat, sounds) land behind the
//! same seams upstream uses, injected so the logic itself stays exact.
//!
//! The mod API wires what the pinned SDK (api 0.2, f00125f61f) reaches:
//! - `client_setup` runs `on_initialize_client` (defaults, then the
//!   `autolec.txt` config load, bad lines skipped per line as upstream does);
//! - `client_tick_pre` runs the `MinecraftTickHead` state machine every client
//!   tick through the seam;
//! - `register_client_commands` pushes the `/autolec` tree's usages, and
//!   `command` dispatches a typed line through the same Brigadier-shaped
//!   seam (`AutoLec.register` + `ClientCommandManager.executeCommand`).
//!
//! Gaps, tracked: the mod API's tick hook carries no world/player handles, so
//! the state machine's world-facing calls (lectern scan, block break/replace,
//! villager data) run through the stub seam until the carrier feeds them; the
//! command tree's registry-entry-reference argument resolves enchantments via
//! the seam's fall-through, not the carrier's registry.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use mod_api::{
    ClientHost, ClientSetupEvent, CommandEvent, Flow, RegisterClientCommandsEvent,
    RegistrationContext, Registrar,
};

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

/// The live mod state: the instance (`AutoLectern.getInstance()` on upstream)
/// and the `/autolec` dispatcher, registered once at init.
struct State {
    al: AutoLectern,
    dispatcher: commands_seam::CommandDispatcher,
}

// The state holds seam types (game handles behind the seams) and the
// dispatcher's non-Send command closures; every hook that touches it runs on
// the client thread the SDK drives, so sending it into the static is sound.
unsafe impl Send for State {}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

/// Java: `FabricLoader.getInstance().getConfigDir()` - the loader's config
/// dir, from AUTOLEC_CONFIG_DIR, defaulting to `./config`.
fn fabric_loader_get_config_dir() -> PathBuf {
    std::env::var_os("AUTOLEC_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config"))
}

/// The `autolec.txt` config file path (upstream `config/autolec.txt`).
fn config_path() -> PathBuf {
    fabric_loader_get_config_dir().join("autolec.txt")
}

/// The `/autolec` tree's usages, in registration order.
const USAGES: &[&str] = &[
    "autolec start",
    "autolec stop",
    "autolec sync item",
    "autolec breakCooldown",
    "autolec autoTrade ENCHANT",
    "autolec autoTrade CHEAPEST",
    "autolec autoTrade OFF",
    "autolec log",
    "autolec preBreak",
    "autolec preserveTool",
    "autolec clear",
    "autolec remove <index>",
    "autolec remove <index> <uuid>",
    "autolec autoRemove",
    "autolec list",
    "autolec add min",
    "autolec add max",
    "autolec add any",
    "autolec add <minPrice> <maxPrice>",
    "autolec add <enchantment> min",
    "autolec add <enchantment> max",
    "autolec add <enchantment> any",
    "autolec add <enchantment> <minPrice> <maxPrice>",
];

#[derive(Default)]
struct Mod;

#[mod_api::horizon_mod]
impl mod_api::Hooks for Mod {
    /// No settings: the original's config is the `autolec.txt` file, loaded
    /// and saved 1:1; its toggles are the `/autolec` toggles.
    fn register(&self, _registrar: &mut dyn Registrar, _context: &RegistrationContext) {}

    /// 1:1 of AutoLectern.onInitializeClient: defaults, then config load,
    /// and the `/autolec` dispatcher registered once.
    fn client_setup(&self, _host: &mut dyn ClientHost, _event: &mut ClientSetupEvent) {
        let _ = STATE.get_or_init(|| {
            let mut al = AutoLectern::new();
            al.on_initialize_client(config_path());
            let mut dispatcher = commands_seam::CommandDispatcher::new();
            commands_seam::AutoLec::register(&mut dispatcher, &commands_seam::CommandBuildContext);
            Mutex::new(State { al, dispatcher })
        });
    }

    /// The `MinecraftTickHead` state machine, every client tick, through the
    /// seam (world-facing calls no-op until the carrier feeds the handles).
    fn client_tick_pre(&self, _host: &mut dyn ClientHost, _event: &mut mod_api::ClientTickPreEvent) {
        if let Some(state) = STATE.get() {
            let mut state = state.lock().unwrap();
            let mut mc = seams::MinecraftSeam::default();
            mixins::minecraft_client_on_client_tick(&mut state.al, &mut mc);
        }
    }

    /// The `/autolec` tree's usages, in registration order.
    fn register_client_commands(
        &self,
        _host: &mut dyn ClientHost,
        event: &mut RegisterClientCommandsEvent,
    ) {
        for usage in USAGES {
            event.commands.push(mod_api::Command::new(usage));
        }
    }

    /// Dispatch a typed `/autolec ...` line through the same Brigadier-shaped
    /// seam (ClientCommandManager.executeCommand), 1:1.
    fn command(&self, _host: &mut dyn ClientHost, event: &mut CommandEvent) -> Flow {
        if !event.command.starts_with("autolec") {
            return Flow::Continue;
        }
        let Some(state) = STATE.get() else {
            return Flow::Cancel;
        };
        let mut state = state.lock().unwrap();
        let State { al, dispatcher } = &mut *state;
        let mut mc = seams::MinecraftSeam::default();
        let reader = commands_seam::StringReader::new(event.command.clone());
        commands_seam::execute_command(&mut mc, dispatcher, reader, "autolec", al);
        Flow::Cancel
    }
}
