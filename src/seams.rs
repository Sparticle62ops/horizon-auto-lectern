//! Game-facing seams for the AutoLectern port, matching the surface upstream
//! touches (`Minecraft`, `LocalPlayer`, `ClientLevel`, `MultiPlayerGameMode`,
//! the enchantment registry, chat, sounds). The mod's own logic stays exact;
//! these types are the injected stand-ins, mirroring the vanilla calls the
//! upstream code makes (field names kept where they clarify the mapping).

use crate::dummy_input::{DummyInput, Vec2};
use std::collections::BTreeMap;

// ----- geometry / world primitives -----

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Upstream `net.minecraft.core.Direction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn subtract(&self, other: &Vec3) -> Vec3 {
        Vec3 { x: self.x - other.x, y: self.y - other.y, z: self.z - other.z }
    }
    /// Upstream `Vec3.scale`-style arithmetic used by the forced-position move.
    pub fn delta_to(&self, other: &Vec3) -> Vec3 {
        Vec3 { x: other.x - self.x, y: other.y - self.y, z: other.z - self.z }
    }
}

/// Upstream `MoverType.SELF` move: the mod only ever moves by
/// `(forcedPos.x - getX(), -0.00001, forcedPos.z - getZ())`.
pub const SELF_MOVE_DROP: f64 = -0.00001;

// ----- items / blocks -----

/// Upstream `net.minecraft.world.item.Items` (only lectern is referenced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Lectern,
    Emerald,
    Book,
    Paper,
    EnchantedBook,
    Other,
}

/// Upstream `net.minecraft.world.level.block.Blocks` (only lectern referenced).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Lectern,
    Other,
}

/// Upstream `BlockState` surface the mod reads.
#[derive(Clone, Copy, Debug)]
pub struct BlockState {
    pub block: Block,
    pub is_air: bool,
    pub can_be_replaced: bool,
}

impl BlockState {
    /// Upstream `blkState.is(Blocks.LECTERN)`.
    pub fn is_lectern(&self) -> bool {
        self.block == Block::Lectern
    }
    /// Upstream `blkState.isAir()`.
    pub fn is_air(&self) -> bool {
        self.is_air
    }
    /// Upstream `world.getBlockState(pos).canBeReplaced()`.
    pub fn can_be_replaced(&self) -> bool {
        self.can_be_replaced
    }
}

/// Upstream `DataComponents.TOOL` surface: damageable, damage value, max
/// damage, and the mining speed against a lectern block state.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolComponent {
    pub mining_speed_on_lectern: f32,
}

/// Upstream `net.minecraft.world.item.ItemStack` surface the mod reads.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemStack {
    pub item: Item,
    pub count: i32,
    /// Upstream `ItemEnchantments` surface: (enchantment identifier, level) pairs.
    pub enchantments: Vec<(String, i32)>,
    pub is_empty: bool,
    pub is_damageable: bool,
    pub damage_value: i32,
    pub max_damage: i32,
    pub tool: Option<ToolComponent>,
    /// Upstream `getAttackAnimation()` (swing) and `getInteractAnimation()`.
    pub attack_animation: f32,
    pub interact_animation: f32,
}

impl ItemStack {
    /// Upstream `ItemStack.EMPTY` (seam filler for out-of-range slots).
    pub const EMPTY: ItemStack = ItemStack {
        item: Item::Other,
        count: 0,
        enchantments: Vec::new(),
        is_empty: true,
        is_damageable: false,
        damage_value: 0,
        max_damage: 0,
        tool: None,
        attack_animation: 0.0,
        interact_animation: 0.0,
    };

    /// Upstream `stack.is(Items.LECTERN)`.
    pub fn is_lectern(&self) -> bool {
        self.item == Item::Lectern
    }
    pub fn is_empty(&self) -> bool {
        self.is_empty
    }
    /// Upstream `stack.get(DataComponents.TOOL)`.
    pub fn get_tool(&self) -> Option<&ToolComponent> {
        self.tool.as_ref()
    }
    /// Upstream `stack.has(DataComponents.TOOL)`.
    pub fn has_tool(&self) -> bool {
        self.tool.is_some()
    }
}

// ----- enchantments -----

/// Upstream `Holder<Enchantment>` surface: `value().getMaxLevel()`,
/// `is(EnchantmentTags.DOUBLE_TRADE_PRICE)` and `value().description()`.
#[derive(Clone, Debug)]
pub struct Enchantment {
    pub max_level: i32,
    pub double_trade_price: bool,
    pub description: String,
}

/// Upstream `VillagerProfession` holder, for `ExtraVillagerData`.
#[derive(Clone, Debug, PartialEq)]
pub struct VillagerProfessionHolder(pub String);

/// Upstream `VillagerProfession.LIBRARIAN` (the `matchesKey` target in the
/// profession-switch check).
pub fn villager_profession_librarian() -> VillagerProfessionHolder {
    VillagerProfessionHolder("librarian".into())
}

// ----- interactions -----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionHand {
    MainHand,
    OffHand,
}

/// Upstream `InteractionResult` / `InteractionResult.SwingSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionResult {
    Success { swing_source: SwingSource },
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwingSource {
    Predicted,
    Other,
}

/// Upstream `ServerboundPlayerActionPacket(Action.ABORT_DESTROY_BLOCK, pos, side)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerboundPlayerActionPacket {
    pub pos: BlockPos,
    pub side: Direction,
}

// ----- hit results -----

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHitResult {
    pub block_pos: BlockPos,
    pub direction: Direction,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityHitResult {
    /// Upstream `EntityHitResult(updatedVillager, villagePos)`.
    pub entity_id: u64,
    pub pos: Vec3,
}

/// Upstream `HitResult` with its `getType()`.
#[derive(Clone, Debug, PartialEq)]
pub enum HitResult {
    Block(BlockHitResult),
    Entity(EntityHitResult),
    Miss,
}

// ----- chat / components -----

/// Upstream `ChatFormatting` subset the mod uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatFormatting {
    Yellow,
    Red,
    Blue,
    White,
    Green,
    Gray,
}

/// Upstream `ClickEvent` seam (`RunCommand`, `SuggestCommand`).
#[derive(Clone, Debug, PartialEq)]
pub enum ClickEvent {
    RunCommand(String),
    SuggestCommand(String),
}

/// Upstream `Style.EMPTY.withClickEvent(...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub click_event: Option<ClickEvent>,
    /// Upstream `Formatting.UNDERLINE` (applied via `.formatted(..., UNDERLINE)`).
    pub underline: bool,
    /// Upstream `Formatting.ITALIC`.
    pub italic: bool,
    /// Upstream `Style.withHoverEvent(new HoverEvent.ShowText(text))`.
    pub hover_event: Option<Box<Component>>,
}

/// Upstream `Component.literal(text).withStyle(color).append(child)` chains,
/// built 1:1 (the structure is what the chat layer renders).
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    pub text: String,
    pub color: Option<ChatFormatting>,
    pub style: Style,
    pub children: Vec<Component>,
}

impl Component {
    pub fn literal(text: impl Into<String>) -> Component {
        Component { text: text.into(), color: None, style: Style::default(), children: Vec::new() }
    }
    pub fn with_style(mut self, color: ChatFormatting) -> Component {
        self.color = Some(color);
        self
    }
    pub fn with_click_event(mut self, event: ClickEvent) -> Component {
        self.style.click_event = Some(event);
        self
    }
    /// Upstream `Text.translatable(key)` (carried as the raw key).
    pub fn translatable(text: impl Into<String>) -> Component {
        Component { text: text.into(), color: None, style: Style::default(), children: Vec::new() }
    }
    pub fn with_underline(mut self) -> Component {
        self.style.underline = true;
        self
    }
    pub fn with_italic(mut self) -> Component {
        self.style.italic = true;
        self
    }
    pub fn with_hover_event(mut self, text: Component) -> Component {
        self.style.hover_event = Some(Box::new(text));
        self
    }
    pub fn append(mut self, child: Component) -> Component {
        self.children.push(child);
        self
    }
}

/// Upstream sound events referenced (`SoundEvents.EXPERIENCE_ORB_PICKUP`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundEvent {
    ExperienceOrbPickup,
}

/// Upstream SDL window flash flag: `SDL_FLASH_UNTIL_FOCUSED`.
pub const SDL_FLASH_UNTIL_FOCUSED: i32 = 0x00000002;

// ----- the seams themselves -----

/// Upstream `world.registryAccess().lookupOrThrow(Registries.ENCHANTMENT)`:
/// a lookup by identifier, `getValue` returns null on a miss.
#[derive(Clone, Debug, Default)]
pub struct EnchantmentRegistry {
    pub entries: BTreeMap<String, Enchantment>,
    /// Upstream `iterateEntries(EnchantmentTags.TRADEABLE)`: the entries in
    /// the tradeable tag, in registry iteration order.
    pub tradeable: Vec<String>,
}

/// Upstream `net.minecraft.world.entity.npc.villager.Villager` surface.
#[derive(Clone, Debug)]
pub struct Villager {
    pub entity_id: u64,
    pub position: Vec3,
    /// Upstream `vil.getVillagerData().profession()`.
    pub profession: VillagerProfessionHolder,
    /// Injected by the `VillagerMixin` (`ExtraVillagerData`).
    pub autolec_prev_profession: Option<VillagerProfessionHolder>,
}

impl crate::interfaces::ExtraVillagerData for Villager {
    fn autolec_get_prev_profession(&self) -> Option<VillagerProfessionHolder> {
        self.autolec_prev_profession.clone()
    }
    fn autolec_set_prev_profession(&mut self, prev_profession: VillagerProfessionHolder) {
        self.autolec_prev_profession = Some(prev_profession);
    }
}

/// Upstream `Inventory` surface: the hotbar slots the mod indexes
/// (`getItem(i)` for i in 0..9, `getNonEquipmentItems()`, selected slot).
#[derive(Clone, Debug)]
pub struct InventorySeam {
    pub items: Vec<ItemStack>,
    pub selected_slot: i32,
}

impl InventorySeam {
    /// Upstream `inventory.getItem(i)` (out-of-range slots read as EMPTY).
    pub fn get_item(&self, i: i32) -> &ItemStack {
        self.items.get(i as usize).unwrap_or_else(|| Self::empty_stack_ref())
    }

    fn empty_stack_ref() -> &'static ItemStack {
        use std::sync::OnceLock;
        static EMPTY: OnceLock<ItemStack> = OnceLock::new();
        EMPTY.get_or_init(|| ItemStack {
            item: Item::Other,
            count: 0,
            enchantments: Vec::new(),
            is_empty: true,
            is_damageable: false,
            damage_value: 0,
            max_damage: 0,
            tool: None,
            attack_animation: 1.0,
            interact_animation: 1.0,
        })
    }
    /// Upstream `plrInv.getSelectedSlot()` / `setSelectedSlot(idx)`.
    pub fn get_selected_slot(&self) -> i32 {
        self.selected_slot
    }
    pub fn set_selected_slot(&mut self, i: i32) {
        self.selected_slot = i;
    }
    /// Upstream `plrInv.getNonEquipmentItems()` — here the same backing list.
    pub fn non_equipment_items(&self) -> &[ItemStack] {
        &self.items
    }
    /// Upstream `Inventory.isHotbarSlot(idx)` (0..=8).
    pub fn is_hotbar_slot(i: i32) -> bool {
        (0..=8).contains(&i)
    }
}

/// Upstream `LocalPlayer` surface.
#[derive(Clone, Debug)]
pub struct PlayerSeam {
    pub inventory: InventorySeam,
    pub x_rot: f32,
    pub y_rot: f32,
    pub position: Vec3,
    pub entity_id: i32,
    pub main_hand_item: ItemStack,
    pub off_hand_item: ItemStack,
    /// Upstream `plr.containerMenu instanceof InventoryMenu`.
    pub container_menu_is_inventory_menu: bool,
    /// Upstream `plr.input` swap: `DummyInput` vs `new KeyboardInput(mc.options)`.
    pub input: PlayerInput,
    /// Packets sent through `plr.connection.send`.
    pub sent_packets: Vec<ServerboundPlayerActionPacket>,
    /// Upstream `plr.swing(hand, anim, from_server)` calls.
    pub swings: Vec<(InteractionHand, f32, bool)>,
    /// Upstream `plr.move(MoverType.SELF, vec)` calls.
    pub self_moves: Vec<Vec3>,
    /// The configured raycast result (`plr.pick`).
    pub pick_result: Option<HitResult>,
    /// Upstream `plr.displayClientMessage(message, overlay)` calls.
    pub client_messages: Vec<(Component, bool)>,
}

/// Which input implementation is installed on the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerInput {
    KeyboardInput,
    DummyInput,
}

impl PlayerSeam {
    pub fn get_x_rot(&self) -> f32 {
        self.x_rot
    }
    pub fn get_y_rot(&self) -> f32 {
        self.y_rot
    }
    pub fn set_x_rot(&mut self, v: f32) {
        self.x_rot = v;
    }
    pub fn set_y_rot(&mut self, v: f32) {
        self.y_rot = v;
    }
    /// Upstream `plr.position()`.
    pub fn position(&self) -> Vec3 {
        self.position
    }
    /// Upstream `plr.getEyePosition()`.
    pub fn get_eye_position(&self) -> Vec3 {
        self.position
    }
    /// Upstream `plr.getMainHandItem()` / `getOffhandItem()` /
    /// `getItemInHand(hand)`.
    pub fn get_main_hand_item(&self) -> &ItemStack {
        &self.main_hand_item
    }
    pub fn get_off_hand_item(&self) -> &ItemStack {
        &self.off_hand_item
    }
    pub fn get_item_in_hand(&self, hand: InteractionHand) -> &ItemStack {
        match hand {
            InteractionHand::MainHand => &self.main_hand_item,
            InteractionHand::OffHand => &self.off_hand_item,
        }
    }
    pub fn get_item_in_hand_mut(&mut self, hand: InteractionHand) -> &mut ItemStack {
        match hand {
            InteractionHand::MainHand => &mut self.main_hand_item,
            InteractionHand::OffHand => &mut self.off_hand_item,
        }
    }
    /// Upstream `plr.pick(4.5f, 1.0f, false)` (the raycast runs in the seam;
    /// the configured result stands in for the world hit).
    pub fn pick(&self, _distance: f32, _tick: f32, _fluids: bool) -> HitResult {
        self.pick_result.clone().unwrap_or(HitResult::Miss)
    }
    /// Upstream `plr.move(MoverType.SELF, vec)`.
    pub fn move_self(&mut self, vec: Vec3) {
        self.self_moves.push(vec);
        self.position.x += vec.x;
        self.position.y += vec.y;
        self.position.z += vec.z;
    }
    /// Upstream `plr.swing(hand, anim, from_server)`.
    pub fn swing(&mut self, hand: InteractionHand, animation: f32, from_server: bool) {
        self.swings.push((hand, animation, from_server));
    }
    /// Upstream `plr.displayClientMessage(message, overlay)`.
    pub fn display_client_message(&mut self, message: Component, overlay: bool) {
        self.client_messages.push((message, overlay));
    }
    /// Upstream `plr.connection.send(packet)`.
    pub fn send(&mut self, packet: ServerboundPlayerActionPacket) {
        self.sent_packets.push(packet);
    }
    /// Upstream `new DummyInput()` swap.
    pub fn set_dummy_input(&mut self) {
        self.input = PlayerInput::DummyInput;
    }
    /// Upstream `new KeyboardInput(mc.options)` swap.
    pub fn set_keyboard_input(&mut self) {
        self.input = PlayerInput::KeyboardInput;
    }
    /// Upstream `plr.getBoundingBox().inflate(20.0, 20.0, 20.0)` feed into
    /// `ProjectileUtil.getEntityHitResult(...)`; the util's ray-vs-box pick
    /// runs in the seam and returns the villager hit when the ray reaches it.
    pub fn entity_hit_result(
        &self,
        _from: Vec3,
        to: Vec3,
        villager: &Villager,
        max_distance: f64,
    ) -> Option<EntityHitResult> {
        // Distance check mirrors the util's clamp: the villager is hit when the
        // target point is within maxDistance of the ray end.
        let d = ((to.x - villager.position.x).powi(2)
            + (to.y - villager.position.y).powi(2)
            + (to.z - villager.position.z).powi(2))
        .sqrt();
        if d <= max_distance {
            Some(EntityHitResult { entity_id: villager.entity_id, pos: villager.position })
        } else {
            None
        }
    }
}

/// Upstream `ClientLevel` surface.
#[derive(Clone, Debug, Default)]
pub struct LevelSeam {
    /// `world.getBlockState(pos)` by packed key.
    pub block_states: BTreeMap<(i32, i32, i32), BlockState>,
    /// `world.registryAccess().lookupOrThrow(Registries.ENCHANTMENT)`.
    pub enchantments: EnchantmentRegistry,
    /// `world.addBreakingBlockEffects(pos, side, false)` calls.
    pub breaking_effects: Vec<(BlockPos, Direction, bool)>,
    /// `world.destroyBlockProgress(plr.getId(), pos, -1)` calls.
    pub destroy_progress: Vec<(i32, BlockPos, i32)>,
}

impl LevelSeam {
    pub fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.block_states
            .get(&(pos.x, pos.y, pos.z))
            .copied()
            .unwrap_or(BlockState { block: Block::Other, is_air: false, can_be_replaced: false })
    }
    pub fn add_breaking_block_effects(&mut self, pos: BlockPos, side: Direction, value: bool) {
        self.breaking_effects.push((pos, side, value));
    }
    pub fn destroy_block_progress(&mut self, breaker_id: i32, pos: BlockPos, stage: i32) {
        self.destroy_progress.push((breaker_id, pos, stage));
    }
}

/// Upstream `MultiPlayerGameMode` surface.
#[derive(Clone, Debug)]
pub struct GameModeSeam {
    /// Upstream `interactionManager.isDestroying()`.
    pub is_destroying: bool,
}

impl GameModeSeam {
    pub fn is_destroying(&self) -> bool {
        self.is_destroying
    }
    pub fn continue_destroy_block(&mut self, _pos: BlockPos, _side: Direction) {}
    pub fn start_destroy_block(&mut self, _pos: BlockPos, _side: Direction) {}
    /// Upstream `interactionManager.useItemOn(plr, hand, hitResult)`.
    pub fn use_item_on(
        &mut self,
        _plr: &mut PlayerSeam,
        _hand: InteractionHand,
        _hit: &BlockHitResult,
    ) -> InteractionResult {
        InteractionResult::Success { swing_source: SwingSource::Predicted }
    }
    /// Upstream `interactionManager.interact(plr, villager, hit, hand)`.
    pub fn interact(
        &mut self,
        _plr: &mut PlayerSeam,
        _villager: &Villager,
        _hit: &EntityHitResult,
        _hand: InteractionHand,
    ) -> InteractionResult {
        InteractionResult::Success { swing_source: SwingSource::Predicted }
    }
}

/// Upstream chat HUD: `mc.gui.hud.getChat().addClientSystemMessage(component)`.
#[derive(Clone, Debug, Default)]
pub struct ChatSeam {
    pub messages: Vec<Component>,
    /// Upstream `chatHud.resetScroll()` calls (the `removeGoal` paths).
    pub scroll_resets: u32,
}

impl ChatSeam {
    pub fn add_client_system_message(&mut self, component: Component) {
        self.messages.push(component);
    }
    /// Upstream `ChatHud.resetScroll()`.
    pub fn reset_scroll(&mut self) {
        self.scroll_resets += 1;
    }
}

/// Upstream `mc.getSoundManager().play(SimpleSoundInstance.forUI(event, pitch))`.
#[derive(Clone, Debug, Default)]
pub struct SoundSeam {
    pub ui_plays: Vec<(SoundEvent, f32)>,
}

impl SoundSeam {
    pub fn play_for_ui(&mut self, event: SoundEvent, pitch: f32) {
        self.ui_plays.push((event, pitch));
    }
}

/// Upstream `Minecraft` surface the mod touches.
#[derive(Clone, Debug, Default)]
pub struct MinecraftSeam {
    pub player: Option<PlayerSeam>,
    pub level: Option<LevelSeam>,
    pub game_mode: Option<GameModeSeam>,
    /// Upstream `mc.hitResult`.
    pub hit_result: Option<HitResult>,
    pub chat: ChatSeam,
    pub sound: SoundSeam,
    /// Upstream `SDL_FlashWindow(mc.getWindow().handle(), flag)` calls.
    pub window_flashes: Vec<i32>,
    /// Upstream `mc.networkHandler.getPlayerList()` profile names (empty when
    /// the handler is absent).
    pub player_list_names: Vec<String>,
    /// The currently interacted villager is held by the caller; upstream keeps
    /// it on `AutoLectern.updatedVillager`.
    pub updated_villager: Option<Villager>,
}

impl MinecraftSeam {
    pub fn flash_window(&mut self, flag: i32) {
        self.window_flashes.push(flag);
    }
}

/// Upstream `DummyInput` value constructor used by the state machine.
pub fn dummy_input() -> DummyInput {
    DummyInput { move_vector: Vec2::ZERO }
}
