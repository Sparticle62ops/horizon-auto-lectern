//! 1:1 port of the `mixin` package: the six mixin classes as hook functions
//! over the seams (upstream capthehacker99/AutoLectern, CC-BY-NC-SA-4.0).
//!
//! - `ClientCommonNetworkHandlerMixin`: the send redirect.
//! - `GameRendererMixin`: the paused-render keep-alive.
//! - `MinecraftClientMixin`: tick head + stop save.
//! - `ClientPlayerInteractionManagerMixin`: breaking hooks.
//! - `VillagerEntityMixin`: already carried by `interfaces::ExtraVillagerData`
//!   and the `Villager` seam impl.
//! - `ClientPlayNetworkHandlerMixin`: the trade pipeline.

use crate::auto_lectern::{AutoLectern, SIGNAL_ITEM, SIGNAL_PROF, SIGNAL_TRADE, SIGNAL_TRADE_OK};
use crate::commands_seam::{self, CommandBuildContext, CommandDispatcher, StringReader};
use crate::goal::Identifier;
use crate::interfaces::ExtraVillagerData;
use crate::seams::*;
use crate::state::{ALAutoTrade, ALState};

// ----- ClientCommonNetworkHandlerMixin -----

/// Upstream `PlayerMoveC2SPacket` variants read by the send redirect.
#[derive(Clone, Debug, PartialEq)]
pub enum MovePacket {
    /// Upstream `class_2830` (`PlayerMoveC2SPacket.PositionAndRotation`).
    PositionAndRotation {
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool,
        horizontal_collision: bool,
    },
    /// Upstream `class_2831` (`PlayerMoveC2SPacket.LookAndOnGround`).
    LookAndOnGround {
        yaw: f32,
        pitch: f32,
        on_ground: bool,
        horizontal_collision: bool,
    },
    /// Any other packet (sent unchanged).
    Other,
}

/// Upstream `ClientCommonNetworkHandlerMixin.onSendPacket`, the redirect on
/// `ClientConnection.send`. Returns the packet the connection actually sends;
/// while AutoLectern runs, the player's own rotation is spoofed to the lectern
/// view (`getX(0.0)` etc. keep the position; yaw/pitch come from the mod).
pub fn client_common_network_handler_on_send_packet(
    al: &AutoLectern,
    packet: &MovePacket,
) -> MovePacket {
    if al.get_state() == ALState::Stopped {
        return packet.clone();
    }
    match packet {
        MovePacket::PositionAndRotation {
            x,
            y,
            z,
            on_ground,
            horizontal_collision,
            ..
        } => MovePacket::PositionAndRotation {
            x: *x,
            y: *y,
            z: *z,
            yaw: al.get_yaw(),
            pitch: al.get_pitch(),
            on_ground: *on_ground,
            horizontal_collision: *horizontal_collision,
        },
        MovePacket::LookAndOnGround {
            on_ground,
            horizontal_collision,
            ..
        } => MovePacket::LookAndOnGround {
            yaw: al.get_yaw(),
            pitch: al.get_pitch(),
            on_ground: *on_ground,
            horizontal_collision: *horizontal_collision,
        },
        other => other.clone(),
    }
}

// ----- GameRendererMixin -----

/// Upstream `GameRenderer` seam: only the `@Shadow`ed field the mixin touches.
#[derive(Clone, Copy, Debug)]
pub struct GameRendererSeam {
    /// Upstream `field_3998`.
    pub last_render_time: u64,
}

/// Upstream `GameRendererMixin.render` (HEAD on `render(tickCounter, tick)`).
/// `measuring_time_ms` is upstream `Util.getMeasuringTimeMs()`.
pub fn game_renderer_render_head(al: &AutoLectern, renderer: &mut GameRendererSeam, measuring_time_ms: u64) {
    if al.get_state() != ALState::Stopped {
        renderer.last_render_time = measuring_time_ms;
    }
}

// ----- MinecraftClientMixin -----

/// Upstream `MinecraftClientMixin.onClientTick` (HEAD on `tick()`).
pub fn minecraft_client_on_client_tick(al: &mut AutoLectern, mc: &mut MinecraftSeam) {
    al.minecraft_tick_head(mc);
}

/// Upstream `MinecraftClientMixin.onStop` (HEAD on `stop()`). `running` is
/// upstream `field_1698`.
pub fn minecraft_client_on_stop(al: &AutoLectern, running: bool) {
    if running {
        let _ = al.save_config();
    }
}

// ----- ClientPlayerInteractionManagerMixin -----

/// Upstream `ClientPlayerInteractionManager` seam: the two `@Shadow`ed fields.
#[derive(Clone, Copy, Debug)]
pub struct InteractionManagerSeam {
    /// Upstream `blockBreakingCooldown` (`field_3716`).
    pub block_breaking_cooldown: i32,
    /// Upstream `currentBreakingProgress` (`field_3715`).
    pub current_breaking_progress: f32,
}

/// Upstream `onUpdateBlockBreakingProgress` (HEAD on
/// `updateBlockBreakingProgress(pos, direction)`).
pub fn client_player_interaction_manager_on_update_block_breaking_progress(
    al: &AutoLectern,
    im: &mut InteractionManagerSeam,
) {
    if !al.break_cooldown && al.get_state() == ALState::Breaking {
        im.block_breaking_cooldown = 0;
    }
}

/// Upstream `antiBlockBreaking`, the redirect on
/// `BlockState.calcBlockBreakingDelta(player, view, pos)`. `before_ret` is the
/// delta the real game computes; the redirect only chooses what to return.
pub fn client_player_interaction_manager_anti_block_breaking(
    al: &AutoLectern,
    im: &InteractionManagerSeam,
    before_ret: f32,
) -> f32 {
    if im.current_breaking_progress + before_ret < 1.0 {
        return before_ret;
    }
    if al.pre_breaking {
        let state = al.get_state();
        if state != ALState::Stopped && state != ALState::Breaking {
            return 0.0;
        }
    }
    before_ret
}

// ----- ClientPlayNetworkHandlerMixin -----

/// Upstream `VillagerProfession.LIBRARIAN`, the `matchesKey` target.
use villager_profession_librarian as librarian;

/// Upstream `TradeOffer` surface used by the mixin.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeOffer {
    /// Upstream `getDisplayedFirstBuyItem()`.
    pub displayed_first_buy: ItemStack,
    /// Upstream `getDisplayedSecondBuyItem()`.
    pub displayed_second_buy: ItemStack,
    /// Upstream `getOriginalFirstBuyItem()`.
    pub original_first_buy: ItemStack,
    /// Upstream `getSellItem()`.
    pub sell: ItemStack,
}

/// Upstream `TradeOfferList`.
pub type TradeOfferList = Vec<TradeOffer>;

/// Upstream `ScreenHandlerType` (only `MERCHANT` is tested).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenHandlerType {
    Merchant,
    Other,
}

/// Upstream `SlotActionType` (only `PICKUP` is sent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotActionType {
    Pickup,
}

/// Upstream `OpenScreenS2CPacket` seam.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenScreenPacket {
    pub screen_handler_type: ScreenHandlerType,
    pub sync_id: i32,
}

/// Upstream `CloseScreenS2CPacket` seam.
#[derive(Clone, Debug, PartialEq)]
pub struct CloseScreenPacket {
    pub sync_id: i32,
}

/// Upstream `SetTradeOffersS2CPacket` seam.
#[derive(Clone, Debug, PartialEq)]
pub struct SetTradeOffersPacket {
    pub sync_id: i32,
    pub offers: TradeOfferList,
}

/// Upstream C2S packets the mixin sends through `sendPacket`
/// (`ClientCommonNetworkHandler.sendPacket`).
#[derive(Clone, Debug, PartialEq)]
pub enum C2sPacket {
    /// Upstream `SelectMerchantTradeC2SPacket`.
    SelectMerchantTrade {
        item_index: i32,
    },
    /// Upstream `ClickSlotC2SPacket(syncId, revision, slot, button, action,
    /// changedStacks, changedStack)`. The shared empty `Int2ObjectOpenHashMap`
    /// and the `ItemStackHash.fromItemStack` conversion stay implicit in this
    /// seam: the changed stack is carried as itself.
    ClickSlot {
        sync_id: i32,
        revision: i32,
        slot: i16,
        button: u8,
        action: SlotActionType,
        changed_stack: ItemStack,
    },
    /// Upstream `CloseHandledScreenC2SPacket`.
    CloseHandledScreen {
        sync_id: i32,
    },
}

/// Upstream `ClientPlayNetworkHandler` seam: the `@Shadow`/`@Unique` state the
/// mixin reads and writes.
pub struct ClientPlayNetworkHandlerSeam {
    /// Upstream `commandDispatcher`.
    pub dispatcher: CommandDispatcher,
    /// Upstream `merchantSyncId` (`@Unique`).
    pub merchant_sync_id: i32,
    /// Upstream `componentHasher` (`@Final`). `ItemStackHash.fromItemStack`
    /// keeps the stack itself in this seam, so the hasher is a marker.
    pub component_hasher: (),
    /// Packets passed to upstream `sendPacket`.
    pub sent_packets: Vec<C2sPacket>,
}

impl ClientPlayNetworkHandlerSeam {
    pub fn new(dispatcher: CommandDispatcher) -> ClientPlayNetworkHandlerSeam {
        ClientPlayNetworkHandlerSeam {
            dispatcher,
            merchant_sync_id: -1,
            component_hasher: (),
            sent_packets: Vec::new(),
        }
    }

    /// Upstream `ClientCommonNetworkHandler.sendPacket`.
    fn send_packet(&mut self, packet: C2sPacket) {
        self.sent_packets.push(packet);
    }
}

/// Upstream `CommandManager.createRegistryAccess(BuiltinRegistries.
/// createWrapperLookup())` — a constant registry access in this seam.
pub fn command_manager_create_registry_access() -> CommandBuildContext {
    CommandBuildContext
}

/// Upstream `onInit` (RETURN on `<init>`).
pub fn client_play_network_handler_on_init(h: &mut ClientPlayNetworkHandlerSeam) {
    AutoLectern::register_commands(&mut h.dispatcher, &command_manager_create_registry_access());
}

/// Upstream `onOnCommandTree` (TAIL on `onCommandTree`).
pub fn client_play_network_handler_on_command_tree(h: &mut ClientPlayNetworkHandlerSeam) {
    AutoLectern::register_commands(&mut h.dispatcher, &command_manager_create_registry_access());
}

/// Upstream `onOpenScreen` (HEAD on `onOpenScreen`, cancellable). Returns
/// whether the original handler call is cancelled.
pub fn client_play_network_handler_on_open_screen(
    h: &mut ClientPlayNetworkHandlerSeam,
    al: &AutoLectern,
    packet: &OpenScreenPacket,
) -> bool {
    if al.get_state() != ALState::WaitingTrade {
        return false;
    }
    if packet.screen_handler_type != ScreenHandlerType::Merchant {
        return false;
    }
    h.merchant_sync_id = packet.sync_id;
    true
}

/// Upstream `onCloseScreen` (HEAD on `onCloseScreen`, cancellable).
pub fn client_play_network_handler_on_close_screen(
    h: &mut ClientPlayNetworkHandlerSeam,
    packet: &CloseScreenPacket,
) -> bool {
    if packet.sync_id != h.merchant_sync_id {
        return false;
    }
    h.merchant_sync_id = -1;
    true
}

/// Upstream `onPreEntityTrackerUpdate` (at `trackedValues()`, villager branch).
pub fn client_play_network_handler_on_pre_entity_tracker_update(vil: &mut Villager) {
    ExtraVillagerData::autolec_set_prev_profession(vil, vil.profession.clone());
}

/// Upstream `onPostEntityTrackerUpdate` (after `writeUpdatedEntries`). Returns
/// whether the villager was taken as the updated one. Upstream's
/// `prevProfession` starts `null`, where `matchesKey` cannot be true; the seam
/// treats a missing previous profession the same way.
pub fn client_play_network_handler_on_post_entity_tracker_update(
    al: &mut AutoLectern,
    vil: &Villager,
) -> bool {
    if al.get_state() != ALState::WaitingProf {
        return false;
    }
    if al.get_updated_villager().is_some() {
        return false;
    }
    let lectern_pos = match al.get_lectern_pos() {
        Some(p) => p,
        // Upstream reads the live `lecternPos` field; unset here means the
        // state has not reached the lectern yet.
        None => return false,
    };
    if lectern_pos.get_squared_distance_vec3(&vil.position) > 8.0 {
        return false;
    }
    if vil.profession != librarian() {
        return false;
    }
    if let Some(prev) = vil.autolec_get_prev_profession() {
        if prev == librarian() {
            return false;
        }
    }
    al.set_updated_villager(Some(vil.clone()));
    al.signal(SIGNAL_PROF);
    true
}

/// Upstream `onItemPickupAnimation` (at `ItemEntity.getStack()`); the seam
/// passes the item entity's stack item. `item_entity_stack_item` is upstream
/// `itemEntity.getStack().getItem()`.
pub fn client_play_network_handler_on_item_pickup_animation(
    al: &mut AutoLectern,
    item_entity_stack_item: Item,
) {
    if al.get_state() != ALState::WaitingItem {
        return;
    }
    if item_entity_stack_item == Item::Lectern {
        al.signal(SIGNAL_ITEM);
    }
}

/// Upstream `EnchantmentHelper.canHaveEnchantments(stack)`: the seam keys it
/// off the item, and the only sell stack this path tests is an enchanted book.
pub fn enchantment_helper_can_have_enchantments(stack: &ItemStack) -> bool {
    stack.item == Item::EnchantedBook
}

/// Upstream `Enchantment.getName(holder, level)` rendered through the seam
/// registry: the enchantment description plus its level.
pub fn enchantment_get_name(level: &LevelSeam, enchant_id: &str, lvl: i32) -> Component {
    let description = level
        .enchantments
        .entries
        .get(enchant_id)
        .map(|e| e.description.clone())
        .unwrap_or_default();
    Component::literal(format!("{} {}", description, lvl))
}

impl BlockPos {
    /// Upstream `BlockPos.getSquaredDistance(Vec3)` (block-center distance).
    pub fn get_squared_distance_vec3(&self, v: &Vec3) -> f64 {
        let dx = self.x as f64 + 0.5 - v.x;
        let dy = self.y as f64 + 0.5 - v.y;
        let dz = self.z as f64 + 0.5 - v.z;
        dx * dx + dy * dy + dz * dz
    }
}

/// Upstream `PlayerInventory.getEmptySlot()`: the first empty main-inventory
/// slot index, or -1 (the seam's inventory is the main stack list).
fn inventory_get_empty_slot(plr: &PlayerSeam) -> i32 {
    for (i, stack) in plr.inventory.non_equipment_items().iter().enumerate() {
        if stack.is_empty() {
            return i as i32;
        }
    }
    -1
}

/// Upstream `findCheapestTrade(offers, plr)` (static `@Unique`).
pub(crate) fn find_cheapest_trade(offers: &TradeOfferList, plr: &PlayerSeam) -> i32 {
    let mut avail_emerald = 0;
    let mut avail_book = 0;
    let mut avail_paper = 0;

    for stack in plr.inventory.non_equipment_items() {
        if stack.is_empty() {
            continue;
        }
        let itm = stack.item;
        if itm == Item::Emerald {
            avail_emerald += stack.count;
        } else if itm == Item::Book {
            avail_book += stack.count;
        } else if itm == Item::Paper {
            avail_paper += stack.count;
        }
    }

    let mut min_idx = -1;
    let mut min_cost = f32::MAX;
    let mut cur_idx = min_idx;

    for offer in offers {
        cur_idx += 1;
        let mut cur_cost = 0.0f32;
        let first = &offer.displayed_first_buy;
        let first_item = first.item;
        if first_item == Item::Emerald {
            let count = first.count;
            if count > avail_emerald {
                continue;
            }
            cur_cost += count as f32;
        } else if first_item == Item::Paper {
            let count = first.count;
            if first.count > avail_paper {
                continue;
            }
            cur_cost += count as f32 * 0.015625;
        } else if first_item == Item::Book {
            let count = first.count;
            if first.count > avail_book {
                continue;
            }
            cur_cost += count as f32 * 0.146875;
        } else {
            continue;
        }

        if cur_cost < min_cost {
            min_idx = cur_idx;
            min_cost = cur_cost;
        }
    }

    min_idx
}

/// Upstream `checkTrades(AL, offers, plr)` (`@Unique`).
pub(crate) fn check_trades(
    al: &mut AutoLectern,
    offers: &TradeOfferList,
    plr: &mut PlayerSeam,
    level: &LevelSeam,
) -> i32 {
    for cur_idx in 0..offers.len() {
        let offer = &offers[cur_idx];
        let sell_item = &offer.sell;
        if sell_item.item != Item::EnchantedBook {
            continue;
        }
        if !enchantment_helper_can_have_enchantments(sell_item) {
            continue;
        }
        let enchantments = &sell_item.enchantments;
        if enchantments.is_empty() {
            continue;
        }
        for (enchant_id, lvl) in enchantments {
            let lvl = *lvl;
            if al.log_trade {
                let name = enchantment_get_name(level, enchant_id, lvl);
                plr.display_client_message(
                    Component::literal("[Auto Lectern] ")
                        .with_style(ChatFormatting::Yellow)
                        .append(
                            Component::literal(offer.original_first_buy.count.to_string())
                                .with_style(ChatFormatting::Green)
                                .append(
                                    Component::literal(" emeralds for ")
                                        .with_style(ChatFormatting::White)
                                        .append(name)
                                        .append(
                                            Component::literal(format!(" [{}]", al.attempts))
                                                .with_style(ChatFormatting::White),
                                        ),
                                ),
                        ),
                    false,
                );
            }

            // upstream `assert enchant.getKey().isPresent();` then `.get()`.
            let enchant = Identifier::parse(enchant_id).unwrap();
            let goal_met = al.get_goal_met(level, offer.original_first_buy.count, &enchant, lvl);
            if goal_met != -1 {
                al.set_last_goal_met(goal_met as usize);
                return cur_idx as i32;
            }
        }
    }

    -1
}

/// Upstream `attemptLockTrade(AL, offers, plr, tarIdx)` (`@Unique`).
pub(crate) fn attempt_lock_trade(
    al: &AutoLectern,
    h: &mut ClientPlayNetworkHandlerSeam,
    offers: &TradeOfferList,
    plr: &PlayerSeam,
    tar_idx: i32,
) {
    let empty_slot = inventory_get_empty_slot(plr);
    if empty_slot == -1 {
        return;
    }
    if al.auto_trade == ALAutoTrade::Enchant {
        let mut avail_emerald = 0;
        let mut avail_book = 0;

        for stack in plr.inventory.non_equipment_items() {
            if stack.is_empty() {
                continue;
            }
            let itm = stack.item;
            if itm == Item::Emerald {
                avail_emerald += stack.count;
            } else if itm == Item::Book {
                avail_book += stack.count;
            }
        }

        let tar_offer = &offers[tar_idx as usize];
        let first = &tar_offer.displayed_first_buy;
        if first.item != Item::Emerald {
            return;
        }
        let second = &tar_offer.displayed_second_buy;
        if second.item != Item::Book {
            return;
        }
        if first.count > avail_emerald || second.count > avail_book {
            return;
        }

        h.send_packet(C2sPacket::SelectMerchantTrade {
            item_index: tar_idx,
        });
        h.send_packet(C2sPacket::ClickSlot {
            sync_id: h.merchant_sync_id,
            revision: 6,
            slot: 2,
            button: 0,
            action: SlotActionType::Pickup,
            changed_stack: tar_offer.sell.clone(),
        });
        h.send_packet(C2sPacket::ClickSlot {
            sync_id: h.merchant_sync_id,
            revision: 6,
            slot: empty_slot as i16,
            button: 0,
            action: SlotActionType::Pickup,
            changed_stack: ItemStack::EMPTY.clone(),
        });
    } else {
        let min_idx = find_cheapest_trade(offers, plr);
        if min_idx == -1 {
            return;
        }

        h.send_packet(C2sPacket::SelectMerchantTrade { item_index: min_idx });
        h.send_packet(C2sPacket::ClickSlot {
            sync_id: h.merchant_sync_id,
            revision: 6,
            slot: 2,
            button: 0,
            action: SlotActionType::Pickup,
            changed_stack: offers[min_idx as usize].sell.clone(),
        });
        h.send_packet(C2sPacket::ClickSlot {
            sync_id: h.merchant_sync_id,
            revision: 6,
            slot: empty_slot as i16,
            button: 0,
            action: SlotActionType::Pickup,
            changed_stack: ItemStack::EMPTY.clone(),
        });
    }
}

/// Upstream `determineTrades(offers)` (`@Unique`).
fn determine_trades(
    h: &mut ClientPlayNetworkHandlerSeam,
    mc: &mut MinecraftSeam,
    al: &mut AutoLectern,
    offers: &TradeOfferList,
) {
    if al.get_state() != ALState::WaitingTrade {
        return;
    }
    let MinecraftSeam { player, level, .. } = mc;
    let Some(plr) = player.as_mut() else {
        return;
    };
    let Some(level) = level.as_ref() else {
        return;
    };

    al.attempts += 1;
    let tar_idx = check_trades(al, offers, plr, level);
    if tar_idx == -1 {
        al.signal(SIGNAL_TRADE);
        return;
    }

    al.signal(SIGNAL_TRADE | SIGNAL_TRADE_OK);
    if al.auto_trade != ALAutoTrade::Off {
        attempt_lock_trade(al, h, offers, plr, tar_idx);
    }
}

/// Upstream `onSetTradeOffers` (after the main-thread hop, cancellable).
/// Returns whether the original handler call is cancelled.
pub fn client_play_network_handler_on_set_trade_offers(
    h: &mut ClientPlayNetworkHandlerSeam,
    mc: &mut MinecraftSeam,
    al: &mut AutoLectern,
    packet: &SetTradeOffersPacket,
) -> bool {
    if packet.sync_id != h.merchant_sync_id {
        return false;
    }
    determine_trades(h, mc, al, &packet.offers);
    h.send_packet(C2sPacket::CloseHandledScreen {
        sync_id: h.merchant_sync_id,
    });
    true
}

/// Upstream `onSendChatCommand` (HEAD on `sendChatCommand(String)`,
/// cancellable).
pub fn client_play_network_handler_on_send_chat_command(
    h: &mut ClientPlayNetworkHandlerSeam,
    mc: &mut MinecraftSeam,
    al: &mut AutoLectern,
    message: &str,
) -> bool {
    intercept_client_side_command(h, mc, al, message)
}

/// Upstream `sendCommand` (HEAD on `runClickEventCommand(String, Screen)`,
/// cancellable).
pub fn client_play_network_handler_send_command(
    h: &mut ClientPlayNetworkHandlerSeam,
    mc: &mut MinecraftSeam,
    al: &mut AutoLectern,
    command: &str,
) -> bool {
    intercept_client_side_command(h, mc, al, command)
}

/// Shared body of `onSendChatCommand` / `sendCommand`.
fn intercept_client_side_command(
    h: &mut ClientPlayNetworkHandlerSeam,
    mc: &mut MinecraftSeam,
    al: &mut AutoLectern,
    message: &str,
) -> bool {
    let mut reader = StringReader::new(message);
    let command_name = if reader.can_read() {
        reader.read_unquoted_string()
    } else {
        String::new()
    };
    reader.set_cursor(0);
    if commands_seam::is_client_side_command(&command_name) {
        commands_seam::execute_command(mc, &mut h.dispatcher, reader, message, al);
        true
    } else {
        false
    }
}
