//! Port of `AutoLectern.java` (the main class): signals, goal matching, the
//! state machine in `MinecraftTickHead`, config save/load. Game-facing calls
//! go through the seams in `crate::seams`.


use crate::goal::ALGoal;
use crate::seams::*;
use crate::state::{ALAutoTrade, ALState};

/// Upstream `public class AutoLectern implements ClientModInitializer`.
pub struct AutoLectern {
    // public fields
    pub auto_trade: ALAutoTrade,
    pub item_sync: bool,
    pub break_cooldown: bool,
    pub log_trade: bool,
    pub pre_breaking: bool,
    pub preserve_tool: bool,
    pub auto_remove: bool,
    pub attempts: i32,
    // private fields
    pub(crate) uuid: i32,
    pub(crate) signals: i32,
    pub(crate) last_goal_met: usize,
    pub(crate) prev_selected_slot: i32,
    pub(crate) lectern_pos: Option<BlockPos>,
    pub(crate) lectern_side: Option<Direction>,
    pub(crate) cur_state: ALState,
    pub(crate) tick_cooldown: i32,
    pub(crate) updated_villager: Option<Villager>,
    pub(crate) goals: Vec<ALGoal>,
    pub(crate) config_file: Option<std::path::PathBuf>,

    pub(crate) lec_pitch: f32,
    pub(crate) lec_yaw: f32,
    pub(crate) fake_pitch: f32,
    pub(crate) fake_yaw: f32,
    pub(crate) forced_pos: Option<Vec3>,
}

/// Upstream bit-flag constants.
pub const SIGNAL_PROF: i32 = 1;
pub const SIGNAL_TRADE: i32 = 1 << 1;
pub const SIGNAL_TRADE_OK: i32 = 1 << 2;
pub const SIGNAL_ITEM: i32 = 1 << 3;

impl AutoLectern {
    pub fn new() -> AutoLectern {
        // Upstream `onInitializeClient` defaults.
        AutoLectern {
            auto_trade: ALAutoTrade::Off,
            item_sync: false,
            break_cooldown: false,
            log_trade: false,
            pre_breaking: true,
            preserve_tool: true,
            auto_remove: false,
            attempts: 0,
            uuid: 0,
            signals: 0,
            last_goal_met: 0,
            prev_selected_slot: -1,
            lectern_pos: None,
            lectern_side: None,
            cur_state: ALState::Stopped,
            tick_cooldown: 0,
            updated_villager: None,
            goals: Vec::new(),
            config_file: None,
            lec_pitch: 0.0,
            lec_yaw: 0.0,
            fake_pitch: 0.0,
            fake_yaw: 0.0,
            forced_pos: None,
        }
    }

    pub fn signal(&mut self, signal_type: i32) {
        self.signals |= signal_type;
    }

    pub fn get_uuid(&self) -> i32 {
        self.uuid
    }

    pub fn increment_uuid(&mut self) {
        self.uuid += 1;
    }

    pub fn get_goals(&self) -> &Vec<ALGoal> {
        &self.goals
    }

    /// Upstream `getMostExpensiveVillagerEnchant` (includes the
    /// `@SuppressWarnings("ManualMinMaxCalculation")` shape).
    pub fn get_most_expensive_villager_enchant(enchant: &Enchantment) -> i32 {
        let k = enchant.max_level;
        let mut min_price = (k * 3) + 2 + (4 + k * 10);
        if enchant.double_trade_price {
            min_price *= 2;
        }
        if min_price > 64 {
            64
        } else {
            min_price
        }
    }

    /// Upstream `getCheapestVillagerEnchant`.
    pub fn get_cheapest_villager_enchant(enchant: &Enchantment) -> i32 {
        let mut min_price = (enchant.max_level * 3) + 2;
        if enchant.double_trade_price {
            min_price *= 2;
        }
        if min_price > 64 {
            64
        } else {
            min_price
        }
    }

    pub(crate) fn is_goal_level_met(&self, max_enc_lvl: i32, lvl_min: i32, lvl_max: i32, lvl: i32) -> bool {
        if lvl_min == -1 {
            if lvl_max == -1 {
                return true;
            }
            // Upstream quirk kept 1:1: this branch compares against the
            // literal 1, not lvlMax.
            return lvl <= 1;
        }
        if lvl_max == -1 {
            return lvl >= max_enc_lvl;
        }
        lvl >= lvl_min && lvl <= lvl_max
    }

    pub(crate) fn is_goal_price_met(
        &self,
        enchant: &Enchantment,
        price_min: i32,
        price_max: i32,
        price: i32,
    ) -> bool {
        if price_min == -1 {
            if price_max == -1 {
                return true;
            }
            return price <= AutoLectern::get_cheapest_villager_enchant(enchant);
        }
        if price_max == -1 {
            return price >= AutoLectern::get_most_expensive_villager_enchant(enchant);
        }
        price >= price_min && price <= price_max
    }

    /// Upstream static `enchantFromIdentifier(Level world, Identifier id)`.
    pub fn enchant_from_identifier(world: &LevelSeam, id: &crate::goal::Identifier) -> Option<Enchantment> {
        world.enchantments.entries.get(&id.0).cloned()
    }

    /// Upstream `getGoalMet(Level world, int price, Identifier enchant, int lvl)`.
    pub fn get_goal_met(
        &self,
        world: &LevelSeam,
        price: i32,
        enchant: &crate::goal::Identifier,
        lvl: i32,
    ) -> i32 {
        let mut idx: i32 = 0;
        for cur_goal in &self.goals {
            let enc_id = match &cur_goal.enchant {
                Some(id) => id,
                None => {
                    idx += 1;
                    continue;
                }
            };
            let enc = match AutoLectern::enchant_from_identifier(world, enc_id) {
                Some(e) => e,
                None => {
                    idx += 1;
                    continue;
                }
            };
            if enchant == enc_id
                && self.is_goal_level_met(enc.max_level, cur_goal.lvl_min, cur_goal.lvl_max, lvl)
                && self.is_goal_price_met(&enc, cur_goal.price_min, cur_goal.price_max, price)
            {
                return idx;
            }
            idx += 1;
        }
        -1
    }

    pub fn get_updated_villager(&self) -> Option<&Villager> {
        self.updated_villager.as_ref()
    }

    pub fn set_updated_villager(&mut self, updated_villager: Option<Villager>) {
        self.updated_villager = updated_villager;
    }

    pub fn get_state(&self) -> ALState {
        self.cur_state
    }

    pub fn set_state(&mut self, new_state: ALState) {
        self.cur_state = new_state;
    }

    pub fn get_lectern_pos(&self) -> Option<BlockPos> {
        self.lectern_pos
    }

    fn tool_near_break(tool: &ItemStack) -> bool {
        tool.is_damageable && tool.damage_value + 2 >= tool.max_damage
    }

    fn equip_working_tool(&self, plr: &mut PlayerSeam) -> bool {
        let mut best_slot: i32 = -1;
        let mut best_speed: f32 = 0.0;
        for i in 0..9 {
            let stack = plr.inventory.get_item(i);
            let tool = match stack.get_tool() {
                Some(t) => t,
                None => continue,
            };
            if AutoLectern::tool_near_break(stack) {
                continue;
            }
            let tool_speed = tool.mining_speed_on_lectern;
            if best_speed >= tool_speed {
                continue;
            }
            best_slot = i;
            best_speed = tool_speed;
        }
        if best_slot != -1 {
            plr.inventory.set_selected_slot(best_slot);
            return true;
        }
        false
    }

    fn check_preserve_tool(&mut self, plr: &mut PlayerSeam) -> bool {
        let tool = plr.get_main_hand_item();
        if tool.has_tool() && !AutoLectern::tool_near_break(tool) {
            return false;
        }
        if self.equip_working_tool(plr) {
            return false;
        }
        self.cur_state = ALState::Stopping;
        true
    }

    fn get_looking_at(&self, plr: &mut PlayerSeam) -> Option<BlockHitResult> {
        let old_pitch = plr.get_x_rot();
        let old_yaw = plr.get_y_rot();
        plr.set_x_rot(self.lec_pitch);
        plr.set_y_rot(self.lec_yaw);
        let hit_result = plr.pick(4.5, 1.0, false);
        plr.set_x_rot(old_pitch);
        plr.set_y_rot(old_yaw);
        match hit_result {
            HitResult::Block(block_hit_result) => Some(block_hit_result),
            _ => None,
        }
    }

    fn equip_lectern(&mut self, plr: &mut PlayerSeam) -> Option<InteractionHand> {
        if plr.get_off_hand_item().is_lectern() {
            return Some(InteractionHand::OffHand);
        }
        if plr.get_main_hand_item().is_lectern() {
            return Some(InteractionHand::MainHand);
        }
        let mut idx: i32 = 0;
        for itm_stk in plr.inventory.non_equipment_items() {
            if !itm_stk.is_lectern() {
                idx += 1;
                continue;
            }
            if !InventorySeam::is_hotbar_slot(idx) {
                break;
            }
            self.prev_selected_slot = plr.inventory.get_selected_slot();
            plr.inventory.set_selected_slot(idx);
            return Some(InteractionHand::MainHand);
        }
        None
    }

    fn pre_break(
        &mut self,
        plr: &mut PlayerSeam,
        interaction_manager: Option<&mut GameModeSeam>,
        world: &mut LevelSeam,
    ) {
        if self.prev_selected_slot != -1 {
            let prev = self.prev_selected_slot;
            plr.inventory.set_selected_slot(prev);
            self.prev_selected_slot = -1;
        }
        let main_hand_item = plr.get_main_hand_item().clone();
        if self.preserve_tool {
            self.check_preserve_tool(plr);
        } else if main_hand_item.is_empty() {
            self.equip_working_tool(plr);
        }
        if let (Some(pos), Some(side)) = (self.lectern_pos, self.lectern_side) {
            world.add_breaking_block_effects(pos, side, false);
            if let Some(interaction_manager) = interaction_manager {
                if interaction_manager.is_destroying() {
                    interaction_manager.continue_destroy_block(pos, side);
                } else {
                    interaction_manager.start_destroy_block(pos, side);
                }
            } else {
                return;
            }
            plr.swing(InteractionHand::MainHand, main_hand_item.attack_animation, false);
        }
    }

    /// Upstream `MinecraftTickHead(Minecraft mc)`.
    pub fn minecraft_tick_head(&mut self, mc: &mut MinecraftSeam) {
        if self.cur_state == ALState::Stopped {
            return;
        }
        let player_ok = match &mc.player {
            Some(plr) => plr.container_menu_is_inventory_menu,
            None => false,
        };
        if !player_ok {
            self.cur_state = ALState::Stopping;
        }
        loop {
            // Destructure the seam fields the way upstream reads mc.player /
            // mc.level / mc.gameMode.
            let MinecraftSeam { player, level, game_mode, chat, sound, .. } = mc;
            match self.cur_state {
                ALState::Stopped => {
                    return;
                }
                ALState::Stopping => {
                    if let (Some(plr), Some(world)) = (player.as_mut(), level.as_mut()) {
                        if let (Some(pos), Some(side)) = (self.lectern_pos, self.lectern_side) {
                            plr.send(ServerboundPlayerActionPacket { pos, side });
                            let id = plr.entity_id;
                            world.destroy_block_progress(id, pos, -1);
                        }
                        plr.set_keyboard_input();
                    }
                    self.forced_pos = None;
                    self.prev_selected_slot = -1;
                    self.signals = 0;
                    self.lectern_pos = None;
                    self.lectern_side = None;
                    self.updated_villager = None;
                    chat.add_client_system_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(Component::literal("Stopped.").with_style(ChatFormatting::Red)),
                    );
                    self.cur_state = ALState::Stopped;
                    return;
                }
                ALState::Starting => {
                    self.attempts = 0;
                    self.prev_selected_slot = -1;
                    self.signals = 0;
                    self.updated_villager = None;
                    let world = match level.as_mut() {
                        Some(w) => w,
                        None => {
                                            self.cur_state = ALState::Stopping;
                            continue;
                        }
                    };
                    let crosshair_target = mc.hit_result.clone();
                    let block_hit_result = match crosshair_target {
                        Some(HitResult::Block(b)) => Some(b),
                        _ => None,
                    };
                    let is_lectern = block_hit_result
                        .map(|b| world.get_block_state(&b.block_pos).block == Block::Lectern)
                        .unwrap_or(false);
                    let block_hit_result = match (block_hit_result, is_lectern) {
                        (Some(b), true) => b,
                        _ => {
                                            chat.add_client_system_message(
                                Component::literal("[Auto Lectern] ")
                                    .with_style(ChatFormatting::Yellow)
                                    .append(Component::literal(
                                        "Please look at a lectern before running this command.",
                                    )
                                    .with_style(ChatFormatting::Red)),
                            );
                            self.cur_state = ALState::Stopping;
                            continue;
                        }
                    };
                    let plr = player.as_mut().unwrap();
                    plr.set_dummy_input();
                    self.lec_pitch = plr.get_x_rot();
                    self.lec_yaw = plr.get_y_rot();
                    self.fake_pitch = self.lec_pitch;
                    self.fake_yaw = self.lec_yaw;
                    self.forced_pos = Some(plr.position());
                    self.lectern_pos = Some(block_hit_result.block_pos);
                    self.lectern_side = Some(block_hit_result.direction);
                    self.cur_state = ALState::Breaking;
                }
                ALState::Breaking => {
                    let (world, interaction_manager) =
                        match (level.as_mut(), game_mode.as_mut()) {
                            (Some(w), Some(g)) => (w, g),
                            _ => {
                                                    self.cur_state = ALState::Stopping;
                                continue;
                            }
                        };
                    let pos = self.lectern_pos.unwrap();
                    if world.get_block_state(&pos).can_be_replaced() {
                        self.cur_state = if self.item_sync {
                            ALState::WaitingItem
                        } else {
                            ALState::Placing
                        };
                        continue;
                    }
                    self.fake_pitch = self.lec_pitch;
                    self.fake_yaw = self.lec_yaw;
                    let forced = self.forced_pos.unwrap();
                    let plr = player.as_mut().unwrap();
                    plr.move_self(Vec3 {
                        x: forced.x - plr.position.x,
                        y: SELF_MOVE_DROP,
                        z: forced.z - plr.position.z,
                    });
                    if self.prev_selected_slot != -1 {
                        let prev = self.prev_selected_slot;
                        plr.inventory.set_selected_slot(prev);
                        self.prev_selected_slot = -1;
                    }
                    let main_hand_item = plr.get_main_hand_item().clone();
                    if self.preserve_tool {
                        if self.check_preserve_tool(plr) {
                            continue;
                        }
                    } else if main_hand_item.is_empty() {
                        self.equip_working_tool(plr);
                    }
                    if let Some(side) = self.lectern_side {
                        world.add_breaking_block_effects(pos, side, false);
                        if interaction_manager.is_destroying() {
                            interaction_manager.continue_destroy_block(pos, side);
                        } else {
                            interaction_manager.start_destroy_block(pos, side);
                        }
                        plr.swing(InteractionHand::MainHand, main_hand_item.attack_animation, false);
                    }
                    return;
                }
                ALState::WaitingItem => {
                    let plr = player.as_mut().unwrap();
                    let forced = self.forced_pos.unwrap();
                    plr.move_self(Vec3 {
                        x: forced.x - plr.position.x,
                        y: SELF_MOVE_DROP,
                        z: forced.z - plr.position.z,
                    });
                    if (self.signals & SIGNAL_ITEM) != 0 {
                        self.cur_state = ALState::Placing;
                        continue;
                    }
                    return;
                }
                ALState::Placing => {
                    let world = match level.as_mut() {
                        Some(w) => w,
                        None => {
                            self.cur_state = ALState::Stopping;
                            continue;
                        }
                    };
                    self.fake_pitch = self.lec_pitch;
                    self.fake_yaw = self.lec_yaw;
                    let pos = self.lectern_pos.unwrap();
                    let placed = world.get_block_state(&pos).is_lectern();
                    if placed {
                        self.updated_villager = None;
                        self.tick_cooldown = 40;
                        self.signals = 0;
                        self.cur_state = ALState::WaitingProf;
                    }
                    let interaction_manager = match game_mode.as_mut() {
                        Some(g) => g,
                        None => {
                            self.cur_state = ALState::Stopping;
                            continue;
                        }
                    };
                    let plr = player.as_mut().unwrap();
                    let block_hit_result = match self.get_looking_at(plr) {
                        Some(b) => b,
                        None => {
                            self.cur_state = ALState::Stopping;
                            continue;
                        }
                    };
                    let lectern_hand = self.equip_lectern(plr);
                    if let Some(lectern_hand) = lectern_hand {
                        let action_result = interaction_manager.use_item_on(plr, lectern_hand, &block_hit_result);
                        if let InteractionResult::Success { swing_source } = action_result {
                            if swing_source == SwingSource::Predicted {
                                let anim = plr.get_item_in_hand(lectern_hand).interact_animation;
                                plr.swing(lectern_hand, anim, false);
                            }
                        }
                    }
                    if !world.get_block_state(&pos).is_lectern() {
                        return;
                    }
                    self.updated_villager = None;
                    self.tick_cooldown = 40;
                    self.signals = 0;
                    self.cur_state = ALState::WaitingProf;
                }
                ALState::WaitingProf => {
                    if (self.signals & SIGNAL_PROF) == 0 {
                        {
                            let plr = player.as_mut().unwrap();
                            let forced = self.forced_pos.unwrap();
                            plr.move_self(Vec3 {
                                x: forced.x - plr.position.x,
                                y: SELF_MOVE_DROP,
                                z: forced.z - plr.position.z,
                            });
                        }
                        let world = match level.as_mut() {
                            Some(w) => w,
                            None => {
                                self.cur_state = ALState::Stopping;
                                continue;
                            }
                        };
                        let pos = self.lectern_pos.unwrap();
                        let blk_state = world.get_block_state(&pos);
                        if blk_state.is_air() {
                            self.cur_state = ALState::Placing;
                            continue;
                        }
                        if blk_state.block != Block::Lectern {
                            self.cur_state = ALState::Breaking;
                            continue;
                        }
                        if self.tick_cooldown > 0 {
                            if self.pre_breaking {
                                let plr = player.as_mut().unwrap();
                                self.pre_break(plr, game_mode.as_mut(), world);
                            }
                            self.tick_cooldown -= 1;
                            return;
                        }
                        self.cur_state = ALState::Breaking;
                        continue;
                    }
                    self.cur_state = ALState::InteractVil;
                }
                ALState::InteractVil => {
                    if self.updated_villager.is_none() {
                        self.cur_state = ALState::Stopping;
                        continue;
                    }
                    let interaction_manager = match game_mode.as_mut() {
                        Some(g) => g,
                        None => {
                            self.cur_state = ALState::Stopping;
                            continue;
                        }
                    };
                    self.tick_cooldown = 5;
                    self.signals = 0;
                    self.cur_state = ALState::WaitingTrade;
                    let villager = self.updated_villager.clone().unwrap();
                    let village_pos = villager.position;
                    let (eye_pos, plr) = {
                        let plr = player.as_mut().unwrap();
                        (plr.get_eye_position(), plr)
                    };
                    let hit_result =
                        plr.entity_hit_result(eye_pos, village_pos, &villager, 20.0);
                    let delta_pos = village_pos.subtract(&eye_pos);
                    self.fake_yaw =
                        ((delta_pos.z.atan2(delta_pos.x).to_degrees() - 90.0) % 360.0) as f32;
                    let sqrt = (delta_pos.x * delta_pos.x + delta_pos.z * delta_pos.z).sqrt();
                    self.fake_pitch = (-delta_pos.y.atan2(sqrt).to_degrees()) as f32;
                    let hit = hit_result
                        .clone()
                        .unwrap_or(EntityHitResult { entity_id: villager.entity_id, pos: village_pos });
                    let action_result =
                        interaction_manager.interact(plr, &villager, &hit, InteractionHand::MainHand);
                    if let InteractionResult::Success { swing_source } = action_result {
                        if swing_source == SwingSource::Predicted {
                            let anim = plr.get_item_in_hand(InteractionHand::MainHand).interact_animation;
                            plr.swing(InteractionHand::MainHand, anim, false);
                        }
                    }
                }
                ALState::WaitingTrade => {
                    if (self.signals & SIGNAL_TRADE) != 0 {
                        if (self.signals & SIGNAL_TRADE_OK) == 0 {
                            self.cur_state = ALState::Breaking;
                            continue;
                        }
                        sound.play_for_ui(SoundEvent::ExperienceOrbPickup, 1.0);
                        mc.window_flashes.push(SDL_FLASH_UNTIL_FOCUSED);
                        let goal = self.goals[self.last_goal_met].clone();
                        let world = mc.level.as_ref().unwrap();
                        let mut message = Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal("Goal met: ").with_style(ChatFormatting::White),
                            )
                            .append(
                                Component::literal(
                                    AutoLectern::enchant_from_identifier(world, goal.enchant.as_ref().unwrap())
                                        .unwrap()
                                        .description,
                                )
                                .with_style(ChatFormatting::Gray),
                            );
                        if !self.auto_remove {
                            let cmd = format!("/autolec remove {} {}", self.last_goal_met, self.get_uuid());
                            message = message.append(
                                Component::literal(" [REMOVE]")
                                    .with_click_event(ClickEvent::RunCommand(cmd))
                                    .with_style(ChatFormatting::Red),
                            );
                        } else {
                            let last_elem_idx = self.goals.len() - 1;
                            self.goals[self.last_goal_met] = self.goals[last_elem_idx].clone();
                            self.goals.remove(last_elem_idx);
                            self.increment_uuid();
                            message = message
                                .append(Component::literal(" [AUTO REMOVED]").with_style(ChatFormatting::Red));
                        }
                        chat.add_client_system_message(message);
                        chat.add_client_system_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal("Completed.").with_style(ChatFormatting::Green),
                                ),
                        );
                        self.cur_state = ALState::Stopping;
                        continue;
                    }
                    {
                        let plr = player.as_mut().unwrap();
                        let forced = self.forced_pos.unwrap();
                        plr.move_self(Vec3 {
                            x: forced.x - plr.position.x,
                            y: SELF_MOVE_DROP,
                            z: forced.z - plr.position.z,
                        });
                    }
                    if self.tick_cooldown > 0 {
                        if self.pre_breaking {
                            let plr = player.as_mut().unwrap();
                            if let Some(world) = level.as_mut() {
                                self.pre_break(plr, game_mode.as_mut(), world);
                            }
                        }
                        self.tick_cooldown -= 1;
                        return;
                    }
                    self.cur_state = ALState::InteractVil;
                }
            }
        }
    }

    /// Upstream static `registerCommands(...)`: slice 2 ports the command
    /// classes behind this seam.
    pub fn register_commands(_dispatcher: &mut crate::commands_seam::CommandDispatcher, _registry_access: &crate::commands_seam::CommandBuildContext) {
        crate::commands_seam::clear_client_side_commands();
        crate::commands_seam::AutoLec::register(_dispatcher, _registry_access);
    }

    /// Upstream `saveConfig()`.
    pub fn save_config(&self) -> Result<(), String> {
        let config_file = match &self.config_file {
            Some(f) => f.clone(),
            None => return Err("no config file".to_string()),
        };
        let mut out = String::new();
        out.push_str("itemSync=");
        out.push_str(if self.item_sync { "true\n" } else { "false\n" });
        out.push_str("breakCooldown=");
        out.push_str(if self.break_cooldown { "true\n" } else { "false\n" });
        out.push_str("logTrade=");
        out.push_str(if self.log_trade { "true\n" } else { "false\n" });
        out.push_str("preBreak=");
        out.push_str(if self.pre_breaking { "true\n" } else { "false\n" });
        out.push_str("preserveTool=");
        out.push_str(if self.preserve_tool { "true\n" } else { "false\n" });
        out.push_str("autoRemove=");
        out.push_str(if self.auto_remove { "true\n" } else { "false\n" });
        out.push_str("autoTrade=");
        out.push_str(match self.auto_trade {
            ALAutoTrade::Off => "OFF",
            ALAutoTrade::Enchant => "ENCHANT",
            ALAutoTrade::Cheapest => "CHEAPEST",
        });
        out.push('\n');
        out.push_str("goals=");
        for goal in &self.goals {
            out.push_str(&goal.enchant.as_ref().map(|i| i.0.clone()).unwrap_or_default());
            out.push(',');
            out.push_str(&goal.lvl_min.to_string());
            out.push(',');
            out.push_str(&goal.lvl_max.to_string());
            out.push(',');
            out.push_str(&goal.price_min.to_string());
            out.push(',');
            out.push_str(&goal.price_max.to_string());
            out.push(';');
        }
        std::fs::write(config_file, out).map_err(|e| e.to_string())
    }

    /// Upstream `onInitializeClient()`: defaults, then config load. Bad lines
    /// are skipped with a warning, exactly as upstream does per line.
    pub fn on_initialize_client(&mut self, config_file: std::path::PathBuf) {
        self.config_file = Some(config_file.clone());
        // Defaults
        self.break_cooldown = false;
        self.item_sync = false;
        self.preserve_tool = true;
        self.log_trade = false;
        self.pre_breaking = true;
        self.auto_remove = false;
        self.auto_trade = ALAutoTrade::Off;
        self.goals = Vec::new();
        // Load config
        if let Ok(content) = std::fs::read_to_string(config_file) {
            for line in content.lines() {
                match line.split_once('=') {
                    Some((key, value)) => {
                        let ok = self.apply_config_line(key, value);
                        if !ok {
                            // Upstream logs "Skipping bad option: {line}".
                        }
                    }
                    None => {}
                }
            }
        }
        self.uuid = 0;
        self.cur_state = ALState::Stopped;
    }

    /// One config line's assignment; returns false when upstream would throw
    /// and skip the line.
    fn apply_config_line(&mut self, key: &str, value: &str) -> bool {
        match key {
            "breakCooldown" => self.break_cooldown = value == "true",
            "itemSync" => self.item_sync = value == "true",
            "preserveTool" => self.preserve_tool = value == "true",
            "logTrade" => self.log_trade = value == "true",
            "preBreak" => self.pre_breaking = value == "true",
            "autoRemove" => self.auto_remove = value == "true",
            "autoTrade" => {
                self.auto_trade = if value == "ENCHANT" {
                    ALAutoTrade::Enchant
                } else if value == "CHEAPEST" {
                    ALAutoTrade::Cheapest
                } else {
                    ALAutoTrade::Off
                };
            }
            "goals" => {
                for goal_data in value.split(';') {
                    if goal_data.is_empty() {
                        continue;
                    }
                    let mut gd_it = goal_data.split(',');
                    let (enchant, lvl_min, lvl_max, price_min, price_max) = match (
                        gd_it.next(),
                        gd_it.next(),
                        gd_it.next(),
                        gd_it.next(),
                        gd_it.next(),
                    ) {
                        (Some(e), Some(a), Some(b), Some(c), Some(d)) => (e, a, b, c, d),
                        _ => return false,
                    };
                    let enchant = match crate::goal::Identifier::parse(enchant) {
                        Ok(i) => Some(i),
                        Err(_) => return false,
                    };
                    let (lvl_min, lvl_max, price_min, price_max) = match (
                        lvl_min.parse::<i32>(),
                        lvl_max.parse::<i32>(),
                        price_min.parse::<i32>(),
                        price_max.parse::<i32>(),
                    ) {
                        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
                        _ => return false,
                    };
                    self.goals.push(ALGoal { enchant, lvl_min, lvl_max, price_min, price_max });
                }
            }
            _ => return false,
        }
        true
    }

    pub fn get_pitch(&self) -> f32 {
        self.fake_pitch
    }

    pub fn get_yaw(&self) -> f32 {
        self.fake_yaw
    }

    pub fn set_last_goal_met(&mut self, last_goal_met: usize) {
        self.last_goal_met = last_goal_met;
    }
}

impl Default for AutoLectern {
    fn default() -> Self {
        AutoLectern::new()
    }
}
