//! Port-logic tests: goal math, config round-trip, and the state machine.

use crate::auto_lectern::*;
use crate::commands_seam::*;
use crate::goal::{ALGoal, Identifier};
use crate::interfaces::ExtraVillagerData;
use crate::mixins::*;
use crate::seams::*;
use crate::state::{ALAutoTrade, ALState};

fn ench(max_level: i32, double_price: bool) -> Enchantment {
    Enchantment { max_level, double_trade_price: double_price, description: "Sharpness".into() }
}

fn ident(s: &str) -> Identifier {
    Identifier::parse(s).unwrap()
}

fn lectern_state() -> BlockState {
    BlockState { block: Block::Lectern, is_air: false, can_be_replaced: false }
}

fn air_state() -> BlockState {
    BlockState { block: Block::Other, is_air: true, can_be_replaced: true }
}

fn lectern_stack() -> ItemStack {
    ItemStack {
        item: Item::Lectern,
        count: 0,
        enchantments: Vec::new(),
        is_empty: false,
        is_damageable: false,
        damage_value: 0,
        max_damage: 0,
        tool: None,
        attack_animation: 1.0,
        interact_animation: 1.0,
    }
}

fn pickaxe_stack(damage: i32, max: i32, speed: f32) -> ItemStack {
    ItemStack {
        item: Item::Other,
        count: 0,
        enchantments: Vec::new(),
        is_empty: false,
        is_damageable: true,
        damage_value: damage,
        max_damage: max,
        tool: Some(ToolComponent { mining_speed_on_lectern: speed }),
        attack_animation: 1.0,
        interact_animation: 1.0,
    }
}

fn player_with(items: Vec<ItemStack>) -> PlayerSeam {
    PlayerSeam {
        inventory: InventorySeam { items, selected_slot: 0 },
        x_rot: 30.0,
        y_rot: 90.0,
        position: Vec3 { x: 1.0, y: 64.0, z: 2.0 },
        entity_id: 7,
        main_hand_item: ItemStack {
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
        },
        off_hand_item: ItemStack {
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
        },
        container_menu_is_inventory_menu: true,
        input: PlayerInput::KeyboardInput,
        pick_result: None,
        client_messages: Vec::new(),
        sent_packets: Vec::new(),
        swings: Vec::new(),
        self_moves: Vec::new(),
    }
}

#[test]
fn most_expensive_enchant_price() {
    // k=5: (5*3)+2+(4+5*10) = 69 -> clamped to 64.
    assert_eq!(AutoLectern::get_most_expensive_villager_enchant(&ench(5, false)), 64);
    // k=2: (2*3)+2+(4+2*10) = 32.
    assert_eq!(AutoLectern::get_most_expensive_villager_enchant(&ench(2, false)), 32);
    // k=2 with double trade price: 32*2 = 64.
    assert_eq!(AutoLectern::get_most_expensive_villager_enchant(&ench(2, true)), 64);
}

#[test]
fn cheapest_enchant_price() {
    // k=5: 5*3+2 = 17.
    assert_eq!(AutoLectern::get_cheapest_villager_enchant(&ench(5, false)), 17);
    // k=0: 2.
    assert_eq!(AutoLectern::get_cheapest_villager_enchant(&ench(0, false)), 2);
    // k=11: 35, doubled 70 -> clamp 64.
    assert_eq!(AutoLectern::get_cheapest_villager_enchant(&ench(11, true)), 64);
}

#[test]
fn goal_level_met_including_upstream_quirk() {
    let al = AutoLectern::new();
    // lvlMin == -1, lvlMax == -1 -> true.
    assert!(al.is_goal_level_met(5, -1, -1, 0));
    // lvlMin == -1, lvlMax set -> upstream compares lvl <= 1 (the literal 1).
    assert!(al.is_goal_level_met(5, -1, 4, 1));
    assert!(!al.is_goal_level_met(5, -1, 4, 2));
    // lvlMax == -1 -> lvl >= maxEncLvl.
    assert!(al.is_goal_level_met(5, 2, -1, 5));
    assert!(!al.is_goal_level_met(5, 2, -1, 4));
    // Range.
    assert!(al.is_goal_level_met(5, 2, 4, 3));
    assert!(!al.is_goal_level_met(5, 2, 4, 5));
}

#[test]
fn goal_price_met() {
    let al = AutoLectern::new();
    // priceMin == -1, priceMax == -1 -> true.
    assert!(al.is_goal_price_met(&ench(5, false), -1, -1, 50));
    // priceMin == -1 -> price <= cheapest (k=5 cheapest is 17).
    assert!(al.is_goal_price_met(&ench(5, false), -1, 30, 17));
    assert!(!al.is_goal_price_met(&ench(5, false), -1, 30, 18));
    // priceMax == -1 -> price >= most expensive (k=5 most expensive is 64).
    assert!(al.is_goal_price_met(&ench(5, false), 40, -1, 64));
    // Range.
    assert!(al.is_goal_price_met(&ench(5, false), 10, 20, 15));
}

#[test]
fn goal_met_finds_index() {
    let mut al = AutoLectern::new();
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:sharpness")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:protection")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    let mut world = LevelSeam::default();
    world
        .enchantments
        .entries
        .insert("minecraft:sharpness".into(), ench(5, false));
    world
        .enchantments
        .entries
        .insert("minecraft:protection".into(), ench(5, false));
    assert_eq!(al.get_goal_met(&world, 30, &ident("minecraft:protection"), 3), 1);
    assert_eq!(al.get_goal_met(&world, 30, &ident("minecraft:mending"), 3), -1);
    // A goal whose enchant is missing from the registry is skipped.
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:unregistered")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    assert_eq!(al.get_goal_met(&world, 30, &ident("minecraft:unregistered"), 3), -1);
}

#[test]
fn identifier_parse() {
    assert!(Identifier::parse("minecraft:sharpness").is_ok());
    assert!(Identifier::parse("sharpness").is_ok());
    assert!(Identifier::parse("Bad Namespace:x").is_err());
}

#[test]
fn config_round_trip() {
    let mut al = AutoLectern::new();
    al.item_sync = true;
    al.break_cooldown = false;
    al.log_trade = true;
    al.pre_breaking = false;
    al.preserve_tool = false;
    al.auto_remove = true;
    al.auto_trade = ALAutoTrade::Cheapest;
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:sharpness")),
        lvl_min: -1,
        lvl_max: 3,
        price_min: 10,
        price_max: -1,
    });
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:mending")),
        lvl_min: 1,
        lvl_max: -1,
        price_min: -1,
        price_max: 40,
    });
    let path = std::env::temp_dir().join(format!("autolec-test-{}.txt", std::process::id()));
    al.config_file = Some(path.clone());
    al.save_config().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("itemSync=true\nbreakCooldown=false\n"));
    let mut reloaded = AutoLectern::new();
    reloaded.on_initialize_client(path.clone());
    assert_eq!(reloaded.item_sync, true);
    assert_eq!(reloaded.auto_trade, ALAutoTrade::Cheapest);
    assert_eq!(reloaded.goals, al.goals);
    assert_eq!(reloaded.get_uuid(), 0);
    assert_eq!(reloaded.get_state(), ALState::Stopped);
    let _ = std::fs::remove_file(path);
}

#[test]
fn config_bad_line_skipped() {
    let path = std::env::temp_dir().join(format!("autolec-bad-{}.txt", std::process::id()));
    std::fs::write(
        &path,
        "itemSync=true\nfoo=bar\nautolec=BROKEN\nautoTrade=ENCHANT\ngoals=minecraft:sharpness,1,2,3,4;\n",
    )
    .unwrap();
    let mut al = AutoLectern::new();
    al.on_initialize_client(path.clone());
    assert!(al.item_sync);
    assert_eq!(al.auto_trade, ALAutoTrade::Enchant);
    assert_eq!(al.goals.len(), 1);
    assert_eq!(al.goals[0].price_max, 4);
    let _ = std::fs::remove_file(path);
}

#[test]
fn tick_stopped_noop_and_no_player_stops() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
    // Not stopped but no player -> STOPPING branch, then STOPPED.
    al.set_state(ALState::Starting);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
    assert_eq!(mc.chat.messages.len(), 1);
}

#[test]
fn tick_starting_without_level_goes_stopping() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    al.set_state(ALState::Starting);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
}

#[test]
fn tick_starting_not_looking_at_lectern() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    let mut world = LevelSeam::default();
    world.block_states.insert((0, 64, 0), lectern_state());
    mc.level = Some(world);
    mc.hit_result = Some(HitResult::Block(BlockHitResult {
        block_pos: BlockPos { x: 1, y: 64, z: 0 },
        direction: Direction::North,
    }));
    // Crosshair points at a non-lectern position.
    al.set_state(ALState::Starting);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
    assert!(mc.chat.messages[0].children[0].text.contains("Please look at a lectern"));
}

#[test]
fn tick_starting_to_breaking_to_placing() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(vec![pickaxe_stack(0, 200, 5.0)]));
    let mut world = LevelSeam::default();
    world.block_states.insert((1, 64, 0), lectern_state());
    mc.level = Some(world);
    mc.hit_result = Some(HitResult::Block(BlockHitResult {
        block_pos: BlockPos { x: 1, y: 64, z: 0 },
        direction: Direction::North,
    }));
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    mc.player.as_mut().unwrap().pick_result =
        Some(HitResult::Block(BlockHitResult { block_pos: BlockPos { x: 1, y: 64, z: 0 }, direction: Direction::North }));
    al.set_state(ALState::Starting);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Breaking);
    let plr = mc.player.as_ref().unwrap();
    assert!(matches!(plr.input, PlayerInput::DummyInput));
    assert_eq!(al.get_pitch(), 30.0);
    assert_eq!(al.get_yaw(), 90.0);
    // Break until the block is gone -> PLACING (itemSync off).
    for _ in 0..50 {
        if al.get_state() != ALState::Breaking {
            break;
        }
        al.minecraft_tick_head(&mut mc);
    }
    assert_eq!(al.get_state(), ALState::Breaking);
    mc.level.as_mut().unwrap().block_states.insert((1, 64, 0), air_state());
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Placing);
}

#[test]
fn breaking_uses_item_sync() {
    let mut al = AutoLectern::new();
    al.item_sync = true;
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(vec![pickaxe_stack(0, 200, 5.0)]));
    let mut world = LevelSeam::default();
    world.block_states.insert((0, 0, 0), lectern_state());
    mc.level = Some(world);
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    mc.player.as_mut().unwrap().pick_result =
        Some(HitResult::Block(BlockHitResult { block_pos: BlockPos { x: 0, y: 0, z: 0 }, direction: Direction::North }));
    al.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    al.lectern_side = Some(Direction::North);
    al.forced_pos = Some(Vec3 { x: 1.0, y: 64.0, z: 2.0 });
    al.set_state(ALState::Breaking);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Breaking);
    // Tool equipped from empty main hand, swing fired, destroy started.
    assert_eq!(mc.player.as_ref().unwrap().inventory.get_selected_slot(), 0);
    assert!(!mc.player.as_ref().unwrap().swings.is_empty());
    mc.level.as_mut().unwrap().block_states.insert((0, 0, 0), air_state());
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::WaitingItem);
    // SIGNAL_ITEM moves on to PLACING.
    al.signal(SIGNAL_ITEM);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Placing);
}

#[test]
fn preserve_tool_stops_when_no_replacement() {
    let mut al = AutoLectern::new();
    al.preserve_tool = true;
    let mut mc = MinecraftSeam::default();
    // Main hand: a near-broken tool; hotbar has no other tool.
    let near_broken = pickaxe_stack(199, 200, 8.0);
    let mut plr = player_with(vec![near_broken.clone(), lectern_stack()]);
    plr.main_hand_item = near_broken;
    mc.player = Some(plr);
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    al.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    al.lectern_side = Some(Direction::North);
    al.forced_pos = Some(Vec3::default());
    al.set_state(ALState::Breaking);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
}

#[test]
fn waiting_prof_signal_flows_to_interact_vil() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    // A hotbar tool keeps preserveTool from stopping (upstream 1:1).
    mc.player = Some(player_with(vec![pickaxe_stack(0, 200, 5.0)]));
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    let mut world = LevelSeam::default();
    world.block_states.insert((0, 0, 0), lectern_state());
    mc.level = Some(world);
    al.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    al.forced_pos = Some(Vec3::default());
    // Pre-break cooldown path (preBreaking on) burns a tick first.
    al.tick_cooldown = 1;
    al.set_state(ALState::WaitingProf);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::WaitingProf);
    assert_eq!(mc.player.as_ref().unwrap().self_moves.len(), 1);
    assert_eq!(al.tick_cooldown, 0);
    // Signal: -> INTERACT_VIL (same call).
    al.set_updated_villager(Some(Villager {
        entity_id: 42,
        position: Vec3 { x: 1.0, y: 64.0, z: 1.0 },
        profession: VillagerProfessionHolder(String::new()),
        autolec_prev_profession: None,
    }));
    al.signal(SIGNAL_PROF);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::WaitingTrade);
    assert_eq!(al.tick_cooldown, 4);
    assert_eq!(al.signals, 0);
}

#[test]
fn waiting_prof_block_replaced_transitions() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    mc.player.as_mut().unwrap().pick_result =
        Some(HitResult::Block(BlockHitResult { block_pos: BlockPos { x: 0, y: 0, z: 0 }, direction: Direction::North }));
    al.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    al.forced_pos = Some(Vec3::default());
    // Air -> PLACING.
    let mut world = LevelSeam::default();
    world.block_states.insert((0, 0, 0), air_state());
    mc.level = Some(world);
    al.set_state(ALState::WaitingProf);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Placing);
    // Non-lectern solid -> BREAKING (a hotbar tool keeps preserveTool happy).
    mc.player = Some(player_with(vec![pickaxe_stack(0, 200, 5.0)]));
    let mut world = LevelSeam::default();
    world.block_states.insert(
        (0, 0, 0),
        BlockState { block: Block::Other, is_air: false, can_be_replaced: false },
    );
    mc.level = Some(world);
    al.set_state(ALState::WaitingProf);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Breaking);
    // No level -> STOPPING.
    mc.level = None;
    al.set_state(ALState::WaitingProf);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
}

#[test]
fn interact_vil_sets_rotation_and_trades() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    mc.updated_villager = Some(Villager {
        entity_id: 42,
        position: Vec3 { x: 4.0, y: 64.0, z: 6.0 },
        profession: VillagerProfessionHolder(String::new()),
        autolec_prev_profession: None,
    });
    al.set_updated_villager(mc.updated_villager.clone());
    al.forced_pos = Some(Vec3::default());
    al.set_state(ALState::InteractVil);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::WaitingTrade);
    assert_eq!(al.signals, 0);
    assert_eq!(al.tick_cooldown, 4);
    // fakeYaw/fakePitch point at the villager from the eye at (1,64,2).
    let dy: f64 = 64.0 - 64.0;
    let dx: f64 = 4.0 - 1.0;
    let dz: f64 = 6.0 - 2.0;
    let want_yaw = (((dz.atan2(dx).to_degrees()) - 90.0) % 360.0) as f32;
    assert!((al.get_yaw() - want_yaw).abs() < 0.001);
    let sqrt = (dx * dx + dz * dz).sqrt();
    let want_pitch = (-dy.atan2(sqrt).to_degrees()) as f32;
    assert!((al.get_pitch() - want_pitch).abs() < 0.001);
    // Interact fired on main hand, swing predicted.
    assert!(!mc.player.as_ref().unwrap().swings.is_empty());
    // No villager -> STOPPING.
    al.set_updated_villager(None);
    al.set_state(ALState::InteractVil);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
}

#[test]
fn waiting_trade_ok_completes_and_splices_goal() {
    let mut al = AutoLectern::new();
    al.auto_remove = true;
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:sharpness")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:mending")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    al.set_last_goal_met(0);
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    let mut world = LevelSeam::default();
    world
        .enchantments
        .entries
        .insert("minecraft:sharpness".into(), ench(5, false));
    world
        .enchantments
        .entries
        .insert("minecraft:mending".into(), ench(5, false));
    mc.level = Some(world);
    al.lectern_pos = Some(BlockPos::default());
    al.forced_pos = Some(Vec3::default());
    al.set_state(ALState::WaitingTrade);
    al.signal(SIGNAL_TRADE | SIGNAL_TRADE_OK);
    al.minecraft_tick_head(&mut mc);
    // Goal spliced out, uuid incremented, sound + flash + two chat messages.
    assert_eq!(al.goals.len(), 1);
    assert_eq!(al.goals[0].enchant.as_ref().unwrap().0, "minecraft:mending");
    assert_eq!(al.get_uuid(), 1);
    assert_eq!(mc.sound.ui_plays.len(), 1);
    assert_eq!(mc.window_flashes, vec![SDL_FLASH_UNTIL_FOCUSED]);
    assert_eq!(mc.chat.messages.len(), 3);
    assert_eq!(al.get_state(), ALState::Stopped);
}

#[test]
fn waiting_trade_without_ok_goes_breaking() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(vec![pickaxe_stack(0, 200, 5.0)]));
    mc.game_mode = Some(GameModeSeam { is_destroying: false });
    mc.level = Some(LevelSeam::default());
    al.lectern_pos = Some(BlockPos::default());
    al.forced_pos = Some(Vec3::default());
    al.set_state(ALState::WaitingTrade);
    al.signal(SIGNAL_TRADE);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Breaking);
}

#[test]
fn waiting_trade_keep_message_has_remove_click_event() {
    let mut al = AutoLectern::new();
    al.auto_remove = false;
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:sharpness")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    al.set_last_goal_met(0);
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    let mut world = LevelSeam::default();
    world
        .enchantments
        .entries
        .insert("minecraft:sharpness".into(), ench(5, false));
    mc.level = Some(world);
    al.lectern_pos = Some(BlockPos::default());
    al.forced_pos = Some(Vec3::default());
    al.set_state(ALState::WaitingTrade);
    al.signal(SIGNAL_TRADE | SIGNAL_TRADE_OK);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.goals.len(), 1);
    let msg = &mc.chat.messages[0];
    assert!(msg.text.contains("[Auto Lectern]"));
    // The REMOVE child carries the click command with the live uuid.
    let remove = msg.children[2].clone();
    assert_eq!(remove.style.click_event, Some(ClickEvent::RunCommand("/autolec remove 0 0".into())));
}

#[test]
fn stopping_sends_abort_and_restores_input() {
    let mut al = AutoLectern::new();
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(Vec::new()));
    mc.player.as_mut().unwrap().set_dummy_input();
    mc.level = Some(LevelSeam::default());
    al.lectern_pos = Some(BlockPos { x: 3, y: 64, z: 5 });
    al.lectern_side = Some(Direction::East);
    al.forced_pos = Some(Vec3::default());
    al.set_state(ALState::Stopping);
    al.minecraft_tick_head(&mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);
    assert_eq!(al.get_lectern_pos(), None);
    assert_eq!(al.signals, 0);
    let plr = mc.player.as_ref().unwrap();
    assert_eq!(plr.sent_packets.len(), 1);
    assert_eq!(plr.sent_packets[0].pos, BlockPos { x: 3, y: 64, z: 5 });
    assert!(matches!(plr.input, PlayerInput::KeyboardInput));
    assert_eq!(mc.chat.messages[0].children[0].text, "Stopped.");
}

// ----- slice 2: the commands package (AutoLec + ClientCommandManager) -----

fn commands_mc() -> MinecraftSeam {
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(vec![]));
    mc.level = Some(LevelSeam::default());
    mc
}

fn chat_texts(chat: &ChatSeam) -> Vec<String> {
    chat.messages
        .iter()
        .map(|c| {
            let mut t = c.text.clone();
            for child in &c.children {
                t.push_str(&child.text);
            }
            t
        })
        .collect()
}

fn player_message_texts(source: &FakeCommandSource) -> Vec<String> {
    source
        .player
        .client_messages
        .iter()
        .map(|(c, _)| {
            let mut t = c.text.clone();
            for child in &c.children {
                t.push_str(&child.text);
            }
            t
        })
        .collect()
}

#[test]
fn commands_start_stop_and_registration() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    assert!(is_client_side_command("autolec"));

    let mut al = AutoLectern::new();
    let mc = commands_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    assert_eq!(
        dispatcher
            .execute(StringReader::new("autolec start"), &mut source, &mut al)
            .unwrap(),
        0
    );
    assert_eq!(al.get_state(), ALState::Starting);

    // start while running → chat-hud error
    al.set_state(ALState::Breaking);
    dispatcher
        .execute(StringReader::new("autolec start"), &mut source, &mut al)
        .unwrap();
    let texts = chat_texts(&source.mc.chat);
    let last = texts.last().unwrap();
    assert!(last.contains("Please stop before starting again."));
    let msg = source.mc.chat.messages.last().unwrap();
    assert_eq!(msg.text, "[Auto Lectern] ");
    assert_eq!(msg.color, Some(ChatFormatting::Yellow));
    assert_eq!(msg.children[0].color, Some(ChatFormatting::Blue));

    // stop while running
    dispatcher
        .execute(StringReader::new("autolec stop"), &mut source, &mut al)
        .unwrap();
    assert_eq!(al.get_state(), ALState::Stopping);

    // stop while stopped → "Already stopped."
    al.set_state(ALState::Stopped);
    dispatcher
        .execute(StringReader::new("autolec stop"), &mut source, &mut al)
        .unwrap();
    let texts = player_message_texts(&source);
    assert!(texts.last().unwrap().contains("Already stopped."));
}

#[test]
fn commands_toggles() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();
    let mc = commands_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    for (cmd, _field_text) in [
        ("autolec sync item", "Item Sync is now ON"),
        ("autolec breakCooldown", "Break cooldown is now ON"),
        ("autolec log", "Logging is now ON"),
        ("autolec preBreak", "Pre breaking is now OFF"),
        ("autolec preserveTool", "Tool preserving is now OFF"),
        ("autolec autoRemove", "Auto remove is now ON"),
    ] {
        dispatcher
            .execute(StringReader::new(cmd), &mut source, &mut al)
            .unwrap();
    }
    assert!(al.item_sync);
    assert!(al.break_cooldown);
    assert!(al.log_trade);
    // upstream default was true → now toggled off
    assert!(!al.pre_breaking);
    assert!(!al.preserve_tool);
    assert!(al.auto_remove);

    let texts = player_message_texts(&source);
    for (i, (_, expected)) in [
        ("autolec sync item", "Item Sync is now ON"),
        ("autolec breakCooldown", "Break cooldown is now ON"),
        ("autolec log", "Logging is now ON"),
        ("autolec preBreak", "Pre breaking is now OFF"),
        ("autolec preserveTool", "Tool preserving is now OFF"),
        ("autolec autoRemove", "Auto remove is now ON"),
    ]
    .iter()
    .enumerate()
    {
        assert!(texts[i].contains(expected), "{} vs {}", texts[i], expected);
    }
    // every toggle message: [Auto Lectern] prefix YELLOW, detail WHITE
    for m in &source.player.client_messages {
        assert_eq!(m.0.color, Some(ChatFormatting::Yellow));
        assert_eq!(m.0.children[0].color, Some(ChatFormatting::White));
    }

    // toggling again flips back
    dispatcher
        .execute(StringReader::new("autolec sync item"), &mut source, &mut al)
        .unwrap();
    assert!(!al.item_sync);
    let texts = player_message_texts(&source);
    assert!(texts.last().unwrap().contains("Item Sync is now OFF"));
}

#[test]
fn commands_auto_trade_modes() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();
    let mc = commands_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    for (cmd, expected) in [
        ("autolec autoTrade ENCHANT", ALAutoTrade::Enchant),
        ("autolec autoTrade CHEAPEST", ALAutoTrade::Cheapest),
        ("autolec autoTrade OFF", ALAutoTrade::Off),
    ] {
        dispatcher
            .execute(StringReader::new(cmd), &mut source, &mut al)
            .unwrap();
        assert_eq!(al.auto_trade, expected);
    }
    let texts = player_message_texts(&source);
    assert!(texts[0].contains("Auto Trading Mode is now set to ENCHANT."));
    assert!(texts[1].contains("Auto Trading Mode is now set to CHEAPEST."));
    assert!(texts[2].contains("Auto Trading Mode is now set to OFF."));
}

fn goals_mc() -> MinecraftSeam {
    let mut mc = commands_mc();
    let mut world = LevelSeam::default();
    world
        .enchantments
        .entries
        .insert("minecraft:sharpness".into(), ench(5, false));
    world
        .enchantments
        .entries
        .insert("minecraft:protection".into(), ench(5, false));
    world.enchantments.tradeable = vec![
        "minecraft:sharpness".into(),
        "minecraft:protection".into(),
    ];
    mc.level = Some(world);
    mc
}

#[test]
fn commands_add_with_enchantment() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();
    let mc = goals_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    // /autolec add <ench> max any → addToGoal(ench, 0, -1, -1, -1)
    dispatcher
        .execute(
            StringReader::new("autolec add minecraft:sharpness max any"),
            &mut source,
            &mut al,
        )
        .unwrap();
    assert_eq!(al.goals.len(), 1);
    assert_eq!(al.goals[0].enchant, Some(ident("minecraft:sharpness")));
    assert_eq!(al.goals[0].lvl_min, 0);
    assert_eq!(al.goals[0].lvl_max, -1);
    assert_eq!(al.goals[0].price_min, -1);
    assert_eq!(al.goals[0].price_max, -1);
    assert_eq!(al.get_uuid(), 1);

    // explicit min/max level: /autolec add <ench> 3 5 max → (3, 5, 0, -1)
    dispatcher
        .execute(
            StringReader::new("autolec add minecraft:protection 3 5 max"),
            &mut source,
            &mut al,
        )
        .unwrap();
    assert_eq!(al.goals[1].enchant, Some(ident("minecraft:protection")));
    assert_eq!(al.goals[1].lvl_min, 3);
    assert_eq!(al.goals[1].lvl_max, 5);
    assert_eq!(al.goals[1].price_min, 0);
    assert_eq!(al.goals[1].price_max, -1);

    // explicit prices: /autolec add <ench> max 10 20 → (0, -1, 10, 20)
    dispatcher
        .execute(
            StringReader::new("autolec add minecraft:sharpness max 10 20"),
            &mut source,
            &mut al,
        )
        .unwrap();
    assert_eq!(al.goals[2].lvl_min, 0);
    assert_eq!(al.goals[2].lvl_max, -1);
    assert_eq!(al.goals[2].price_min, 10);
    assert_eq!(al.goals[2].price_max, 20);

    // unknown enchantment falls through to the sibling literals:
    // "min" parses as an identifier (minecraft:min) but is not in the
    // registry, so it resolves to the no-enchant level subcommand instead.
    dispatcher
        .execute(StringReader::new("autolec add min any"), &mut source, &mut al)
        .unwrap();
    assert_eq!(al.goals.len(), 5);
}

#[test]
fn commands_add_all_tradeable_and_clear() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();
    let mc = goals_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    // /autolec add any any → one goal per tradeable enchantment
    dispatcher
        .execute(StringReader::new("autolec add any any"), &mut source, &mut al)
        .unwrap();
    assert_eq!(al.goals.len(), 2);
    assert_eq!(al.goals[0].enchant, Some(ident("minecraft:sharpness")));
    assert_eq!(al.goals[1].enchant, Some(ident("minecraft:protection")));
    assert_eq!(al.goals[0].lvl_min, -1);
    assert_eq!(al.goals[1].price_max, -1);

    dispatcher
        .execute(StringReader::new("autolec clear"), &mut source, &mut al)
        .unwrap();
    assert!(al.goals.is_empty());
    assert_eq!(al.get_uuid(), 2);
    let texts = player_message_texts(&source);
    assert!(texts.last().unwrap().contains("All goals removed."));
    let msg = &source.player.client_messages.last().unwrap().0;
    assert_eq!(msg.children[0].color, Some(ChatFormatting::Green));
}

#[test]
fn commands_remove_and_list() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();
    let mc = goals_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    dispatcher
        .execute(
            StringReader::new("autolec add minecraft:sharpness any any"),
            &mut source,
            &mut al,
        )
        .unwrap();
    dispatcher
        .execute(
            StringReader::new("autolec add minecraft:protection any any"),
            &mut source,
            &mut al,
        )
        .unwrap();
    let uuid = al.get_uuid();

    // expired uuid → Command Expired + scroll reset
    dispatcher
        .execute(
            StringReader::new(format!("autolec remove 0 {}", uuid + 7)),
            &mut source,
            &mut al,
        )
        .unwrap();
    assert_eq!(al.goals.len(), 2);
    let texts = player_message_texts(&source);
    assert!(texts.last().unwrap().contains("Command Expired."));
    assert!(source.mc.chat.scroll_resets >= 1);

    // valid remove → swap-remove, uuid bump, list re-render
    let uuid = al.get_uuid();
    dispatcher
        .execute(
            StringReader::new(format!("autolec remove 0 {}", uuid)),
            &mut source,
            &mut al,
        )
        .unwrap();
    assert_eq!(al.goals.len(), 1);
    assert_eq!(al.goals[0].enchant, Some(ident("minecraft:protection")));
    assert_eq!(al.get_uuid(), uuid + 1);
    let texts = player_message_texts(&source);
    // listGoals ran: "[Auto Lectern] Goals:" then one entry
    let n = texts.len();
    assert!(texts[n - 2].contains("Goals:"));
    assert!(texts[n - 1].contains("[0] Sharpness"));
    let entry = &source.player.client_messages.last().unwrap().0;
    assert_eq!(entry.children[0].children[1].text, " [REMOVE]");
    assert_eq!(
        entry.children[0].children[1].style.click_event,
        Some(ClickEvent::RunCommand(format!(
            "/autolec remove 0 {}",
            uuid + 1
        )))
    );

    // index out of bounds
    dispatcher
        .execute(StringReader::new("autolec remove 9"), &mut source, &mut al)
        .unwrap();
    let texts = player_message_texts(&source);
    assert!(texts.last().unwrap().contains("Index out of bounds."));

    // plain list
    dispatcher
        .execute(StringReader::new("autolec list"), &mut source, &mut al)
        .unwrap();
    let texts = player_message_texts(&source);
    assert!(texts.last().unwrap().contains("[0] Sharpness"));
}

#[test]
fn commands_execute_command_error_paths() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();

    // unknown command at the root
    let mut mc = commands_mc();
    execute_command(
        &mut mc,
        &mut dispatcher,
        StringReader::new("nosuchcmd foo"),
        "nosuchcmd foo",
        &mut al,
    );
    let texts = chat_texts(&mc.chat);
    // the raw error, then its cursor-context line
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0], "Unknown command");
    assert!(texts[1].contains("command.context.here"));
    assert_eq!(mc.chat.messages[0].color, Some(ChatFormatting::Blue));
    assert_eq!(mc.chat.messages[1].color, Some(ChatFormatting::Gray));

    // bad argument → raw message + cursor context, synced back onto mc
    let mut mc = commands_mc();
    execute_command(
        &mut mc,
        &mut dispatcher,
        StringReader::new("autolec add minecraft:sharpness xyz"),
        "autolec add minecraft:sharpness xyz",
        &mut al,
    );
    let texts = chat_texts(&mc.chat);
    assert_eq!(texts.len(), 2);
    assert!(texts[0].contains("Expected integer"));
    let ctx_msg = &mc.chat.messages[1];
    // prefix part is GRAY with a SuggestCommand click event
    assert_eq!(ctx_msg.color, Some(ChatFormatting::Gray));
    assert_eq!(
        ctx_msg.style.click_event,
        Some(ClickEvent::SuggestCommand(
            "autolec add minecraft:sharpness xyz".to_string()
        ))
    );
    // the trailing remainder is RED + UNDERLINE, then the translatable marker
    assert!(texts[1].contains("xyz"));
    assert!(texts[1].contains("command.context.here"));
    let last = &ctx_msg.children.last().unwrap().clone();
    assert_eq!(last.style.italic, true);

    // a runtime failure lands in the generic-Exception branch:
    // "autolec add any" with no world → panic → command.failed + hover
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(vec![]));
    execute_command(
        &mut mc,
        &mut dispatcher,
        StringReader::new("autolec add any any"),
        "autolec add any any",
        &mut al,
    );
    let texts = chat_texts(&mc.chat);
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0], "command.failed");
    let msg = mc.chat.messages.last().unwrap();
    assert!(msg.style.hover_event.is_some());

    // no player → no dispatch, no messages
    let mut mc = MinecraftSeam::default();
    execute_command(
        &mut mc,
        &mut dispatcher,
        StringReader::new("autolec list"),
        "autolec list",
        &mut al,
    );
    assert!(mc.chat.messages.is_empty());
}

#[test]
fn commands_unknown_bare_autolec() {
    clear_client_side_commands();
    let mut dispatcher = CommandDispatcher::new();
    AutoLec::register(&mut dispatcher, &CommandBuildContext);
    let mut al = AutoLectern::new();
    let mc = commands_mc();
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());

    let err = dispatcher
        .execute(StringReader::new("autolec"), &mut source, &mut al)
        .unwrap_err();
    assert!(err.message.starts_with("Incorrect argument for command"));

    let err = dispatcher
        .execute(StringReader::new("autolec bogus"), &mut source, &mut al)
        .unwrap_err();
    assert!(err.message.starts_with("Incorrect argument for command"));

    let err = dispatcher
        .execute(StringReader::new("autolec remove 0 0 extra"), &mut source, &mut al)
        .unwrap_err();
    assert!(err.message.starts_with("Incorrect argument for command"));

    // integer parse errors carry the cursor
    let err = dispatcher
        .execute(StringReader::new("autolec remove abc"), &mut source, &mut al)
        .unwrap_err();
    assert_eq!(err.message, "Expected integer");
    assert_eq!(err.cursor, Some("autolec remove ".len()));
}

// ----- mixin package (slice 2) -----

fn plain_stack(item: Item, count: i32) -> ItemStack {
    ItemStack {
        item,
        count,
        enchantments: Vec::new(),
        is_empty: false,
        is_damageable: false,
        damage_value: 0,
        max_damage: 0,
        tool: None,
        attack_animation: 1.0,
        interact_animation: 1.0,
    }
}

fn enchanted_book(count: i32, enchant_id: &str, lvl: i32) -> ItemStack {
    let mut stack = plain_stack(Item::EnchantedBook, count);
    stack.enchantments = vec![(enchant_id.to_string(), lvl)];
    stack
}

fn sharpness_world() -> LevelSeam {
    let mut world = LevelSeam::default();
    world
        .enchantments
        .entries
        .insert("minecraft:sharpness".into(), ench(5, false));
    world
}

fn offer_for(sell: ItemStack, first_buy: ItemStack) -> TradeOffer {
    TradeOffer {
        displayed_first_buy: first_buy.clone(),
        displayed_second_buy: ItemStack::EMPTY.clone(),
        original_first_buy: first_buy,
        sell,
    }
}

#[test]
fn mixin_send_redirect_spoofs_rotation() {
    let mut al = AutoLectern::new();
    let pos_packet = MovePacket::PositionAndRotation {
        x: 1.0,
        y: 2.0,
        z: 3.0,
        yaw: 10.0,
        pitch: 20.0,
        on_ground: false,
        horizontal_collision: true,
    };
    // Stopped: passthrough.
    assert_eq!(
        client_common_network_handler_on_send_packet(&al, &pos_packet),
        pos_packet
    );
    assert_eq!(
        client_common_network_handler_on_send_packet(&al, &MovePacket::Other),
        MovePacket::Other
    );

    al.set_state(ALState::WaitingTrade);
    let out = client_common_network_handler_on_send_packet(&al, &pos_packet);
    match out {
        MovePacket::PositionAndRotation {
            x,
            y,
            z,
            yaw,
            pitch,
            on_ground,
            horizontal_collision,
        } => {
            assert_eq!(x, 1.0);
            assert_eq!(y, 2.0);
            assert_eq!(z, 3.0);
            assert_eq!(yaw, al.get_yaw());
            assert_eq!(pitch, al.get_pitch());
            assert!(!on_ground);
            assert!(horizontal_collision);
        }
        _ => panic!("expected position packet"),
    }

    let look_packet = MovePacket::LookAndOnGround {
        yaw: 10.0,
        pitch: 20.0,
        on_ground: true,
        horizontal_collision: false,
    };
    match client_common_network_handler_on_send_packet(&al, &look_packet) {
        MovePacket::LookAndOnGround {
            yaw,
            pitch,
            on_ground,
            horizontal_collision,
        } => {
            assert_eq!(yaw, al.get_yaw());
            assert_eq!(pitch, al.get_pitch());
            assert!(on_ground);
            assert!(!horizontal_collision);
        }
        _ => panic!("expected look packet"),
    }
    // Other packets are untouched while running.
    assert_eq!(
        client_common_network_handler_on_send_packet(&al, &MovePacket::Other),
        MovePacket::Other
    );
}

#[test]
fn mixin_game_renderer_and_client_stop() {
    let mut al = AutoLectern::new();
    al.set_state(ALState::Placing);
    let mut renderer = GameRendererSeam { last_render_time: 100 };
    game_renderer_render_head(&al, &mut renderer, 500);
    assert_eq!(renderer.last_render_time, 500);
    al.set_state(ALState::Stopped);
    game_renderer_render_head(&al, &mut renderer, 900);
    assert_eq!(renderer.last_render_time, 500);

    // Tick head is a straight delegation.
    let mut mc = MinecraftSeam::default();
    minecraft_client_on_client_tick(&mut al, &mut mc);
    assert_eq!(al.get_state(), ALState::Stopped);

    // Stop: saveConfig only when running.
    minecraft_client_on_stop(&al, false);
    assert_eq!(al.get_state(), ALState::Stopped);
    minecraft_client_on_stop(&al, true);
    assert_eq!(al.get_state(), ALState::Stopped);
}

#[test]
fn mixin_interaction_manager_hooks() {
    let mut al = AutoLectern::new();
    al.break_cooldown = false;
    al.set_state(ALState::Breaking);
    let mut im = InteractionManagerSeam {
        block_breaking_cooldown: 7,
        current_breaking_progress: 0.5,
    };

    client_player_interaction_manager_on_update_block_breaking_progress(&al, &mut im);
    assert_eq!(im.block_breaking_cooldown, 0);

    // progress + delta < 1 -> passthrough.
    assert_eq!(client_player_interaction_manager_anti_block_breaking(&al, &im, 0.2), 0.2);
    // >= 1 with preBreaking but state BREAKING -> passthrough.
    al.pre_breaking = true;
    assert_eq!(client_player_interaction_manager_anti_block_breaking(&al, &im, 0.6), 0.6);
    // preBreaking with a non-breaking state -> 0.0.
    al.set_state(ALState::WaitingTrade);
    assert_eq!(client_player_interaction_manager_anti_block_breaking(&al, &im, 0.6), 0.0);
    // preBreaking off -> passthrough.
    al.pre_breaking = false;
    assert_eq!(client_player_interaction_manager_anti_block_breaking(&al, &im, 0.6), 0.6);

    // breakCooldown on -> no zeroing.
    al.break_cooldown = true;
    al.set_state(ALState::Breaking);
    im.block_breaking_cooldown = 9;
    client_player_interaction_manager_on_update_block_breaking_progress(&al, &mut im);
    assert_eq!(im.block_breaking_cooldown, 9);
}

#[test]
fn mixin_villager_prev_profession_and_librarian_switch() {
    let mut al = AutoLectern::new();
    al.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    al.set_state(ALState::WaitingProf);

    let farmer = VillagerProfessionHolder("farmer".into());
    let mut vil = Villager {
        entity_id: 5,
        position: Vec3 { x: 0.5, y: 0.5, z: 0.5 },
        profession: farmer.clone(),
        autolec_prev_profession: None,
    };

    // Pre-update: snapshot the old profession.
    client_play_network_handler_on_pre_entity_tracker_update(&mut vil);
    assert_eq!(vil.autolec_get_prev_profession(), Some(farmer.clone()));

    // Post-update: now a librarian close to the lectern -> taken.
    vil.profession = villager_profession_librarian();
    assert!(client_play_network_handler_on_post_entity_tracker_update(&mut al, &vil));
    assert!(al.get_updated_villager().is_some());

    // Wrong state -> not taken.
    let mut al_wrong_state = AutoLectern::new();
    al_wrong_state.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    assert!(!client_play_network_handler_on_post_entity_tracker_update(
        &mut al_wrong_state,
        &vil
    ));

    // Non-librarian -> not taken.
    let mut al2 = AutoLectern::new();
    al2.lectern_pos = Some(BlockPos { x: 0, y: 0, z: 0 });
    al2.set_state(ALState::WaitingProf);
    let vil2 = Villager {
        entity_id: 6,
        position: Vec3 { x: 0.5, y: 0.5, z: 0.5 },
        profession: farmer.clone(),
        autolec_prev_profession: None,
    };
    assert!(!client_play_network_handler_on_post_entity_tracker_update(&mut al2, &vil2));

    // Already a librarian before the update -> not taken.
    let vil_prev_librarian = Villager {
        entity_id: 8,
        position: Vec3 { x: 0.5, y: 0.5, z: 0.5 },
        profession: villager_profession_librarian(),
        autolec_prev_profession: Some(villager_profession_librarian()),
    };
    assert!(!client_play_network_handler_on_post_entity_tracker_update(
        &mut al2,
        &vil_prev_librarian
    ));

    // Too far from the lectern -> not taken.
    let vil3 = Villager {
        entity_id: 7,
        position: Vec3 { x: 100.5, y: 0.5, z: 0.5 },
        profession: villager_profession_librarian(),
        autolec_prev_profession: None,
    };
    assert!(!client_play_network_handler_on_post_entity_tracker_update(&mut al2, &vil3));
}

#[test]
fn mixin_open_close_screen_tracks_merchant_sync_id() {
    let mut h = ClientPlayNetworkHandlerSeam::new(CommandDispatcher::new());
    let mut al = AutoLectern::new();
    al.set_state(ALState::WaitingTrade);

    // Non-merchant screen while WAITING_TRADE -> no capture.
    assert!(!client_play_network_handler_on_open_screen(
        &mut h,
        &al,
        &OpenScreenPacket { screen_handler_type: ScreenHandlerType::Other, sync_id: 3 }
    ));
    assert_eq!(h.merchant_sync_id, -1);

    // Merchant screen -> capture + cancel.
    assert!(client_play_network_handler_on_open_screen(
        &mut h,
        &al,
        &OpenScreenPacket { screen_handler_type: ScreenHandlerType::Merchant, sync_id: 3 }
    ));
    assert_eq!(h.merchant_sync_id, 3);

    // Wrong state -> no capture.
    let al2 = AutoLectern::new();
    assert!(!client_play_network_handler_on_open_screen(
        &mut h,
        &al2,
        &OpenScreenPacket { screen_handler_type: ScreenHandlerType::Merchant, sync_id: 4 }
    ));
    assert_eq!(h.merchant_sync_id, 3);

    // Close: non-matching sync id is ignored, matching one resets and cancels.
    assert!(!client_play_network_handler_on_close_screen(
        &mut h,
        &CloseScreenPacket { sync_id: 9 }
    ));
    assert_eq!(h.merchant_sync_id, 3);
    assert!(client_play_network_handler_on_close_screen(
        &mut h,
        &CloseScreenPacket { sync_id: 3 }
    ));
    assert_eq!(h.merchant_sync_id, -1);
}

#[test]
fn mixin_find_cheapest_trade_picks_available_offer() {
    let plr = player_with(vec![
        plain_stack(Item::Emerald, 10),
        plain_stack(Item::Book, 3),
        plain_stack(Item::Paper, 20),
    ]);
    let offers = vec![
        offer_for(plain_stack(Item::Other, 1), plain_stack(Item::Paper, 10)),
        offer_for(plain_stack(Item::Other, 1), plain_stack(Item::Emerald, 20)),
        offer_for(plain_stack(Item::Other, 1), plain_stack(Item::Book, 2)),
        offer_for(plain_stack(Item::Other, 1), plain_stack(Item::Other, 1)),
    ];
    // Paper offer costs 10 * 0.015625 = 0.15625, the cheapest affordable.
    assert_eq!(find_cheapest_trade(&offers, &plr), 0);

    // No paper or emeralds -> the book offer (2 * 0.146875 = 0.29375).
    let plr2 = player_with(vec![plain_stack(Item::Book, 3)]);
    assert_eq!(find_cheapest_trade(&offers, &plr2), 2);

    // Nothing affordable -> -1.
    let plr3 = player_with(vec![]);
    assert_eq!(find_cheapest_trade(&offers, &plr3), -1);
}

#[test]
fn mixin_check_trades_matches_goal_and_logs() {
    let mut al = AutoLectern::new();
    al.log_trade = true;
    al.attempts = 2;
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:sharpness")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    let world = sharpness_world();
    let mut plr = player_with(vec![]);

    let offers = vec![
        offer_for(enchanted_book(1, "minecraft:sharpness", 3), plain_stack(Item::Emerald, 1)),
    ];
    assert_eq!(check_trades(&mut al, &offers, &mut plr, &world), 0);
    assert_eq!(al.last_goal_met, 0);

    // Log line: prefix yellow, count green, " emeralds for " white, attempts white.
    let (msg, overlay) = &plr.client_messages[0];
    assert!(!*overlay);
    assert_eq!(msg.text, "[Auto Lectern] ");
    assert_eq!(msg.color, Some(ChatFormatting::Yellow));
    assert_eq!(msg.children[0].text, "1");
    assert_eq!(msg.children[0].color, Some(ChatFormatting::Green));
    let for_text = &msg.children[0].children[0];
    assert_eq!(for_text.text, " emeralds for ");
    assert_eq!(for_text.color, Some(ChatFormatting::White));
    assert_eq!(for_text.children[0].text, "Sharpness 3");
    assert_eq!(for_text.children[1].text, " [2]");
    assert_eq!(for_text.children[1].color, Some(ChatFormatting::White));

    // No goal met -> -1, no message.
    let mut al2 = AutoLectern::new();
    al2.log_trade = false;
    let mut plr2 = player_with(vec![]);
    assert_eq!(check_trades(&mut al2, &offers, &mut plr2, &world), -1);
    assert!(plr2.client_messages.is_empty());
}

#[test]
fn mixin_trade_pipeline_locks_and_signals() {
    let mut h = ClientPlayNetworkHandlerSeam::new(CommandDispatcher::new());
    h.merchant_sync_id = 3;
    let mut al = AutoLectern::new();
    al.auto_trade = ALAutoTrade::Enchant;
    al.goals.push(ALGoal {
        enchant: Some(ident("minecraft:sharpness")),
        lvl_min: -1,
        lvl_max: -1,
        price_min: -1,
        price_max: -1,
    });
    al.set_state(ALState::WaitingTrade);

    let mut mc = MinecraftSeam::default();
    let mut plr = player_with(vec![
        plain_stack(Item::Emerald, 10),
        plain_stack(Item::Book, 5),
        ItemStack::EMPTY.clone(),
    ]);
    plr.position = Vec3 { x: 1.0, y: 64.0, z: 2.0 };
    mc.player = Some(plr);
    mc.level = Some(sharpness_world());

    let packet = SetTradeOffersPacket {
        sync_id: 3,
        offers: vec![TradeOffer {
            displayed_first_buy: plain_stack(Item::Emerald, 1),
            displayed_second_buy: plain_stack(Item::Book, 1),
            original_first_buy: plain_stack(Item::Emerald, 1),
            sell: enchanted_book(1, "minecraft:sharpness", 3),
        }],
    };

    assert!(client_play_network_handler_on_set_trade_offers(&mut h, &mut mc, &mut al, &packet));
    assert_eq!(al.attempts, 1);
    assert_eq!(al.signals, SIGNAL_TRADE | SIGNAL_TRADE_OK);
    assert_eq!(h.sent_packets.len(), 4);
    assert_eq!(h.sent_packets[0], C2sPacket::SelectMerchantTrade { item_index: 0 });
    match &h.sent_packets[1] {
        C2sPacket::ClickSlot { sync_id, revision, slot, button, action, changed_stack } => {
            assert_eq!(*sync_id, 3);
            assert_eq!(*revision, 6);
            assert_eq!(*slot, 2);
            assert_eq!(*button, 0);
            assert_eq!(*action, SlotActionType::Pickup);
            assert_eq!(changed_stack.item, Item::EnchantedBook);
        }
        _ => panic!("expected click slot packet"),
    }
    match &h.sent_packets[2] {
        C2sPacket::ClickSlot { slot, changed_stack, .. } => {
            assert_eq!(*slot, 2); // first empty inventory slot
            assert!(changed_stack.is_empty);
        }
        _ => panic!("expected click slot packet"),
    }
    assert_eq!(h.sent_packets[3], C2sPacket::CloseHandledScreen { sync_id: 3 });

    // No goal met: signals TRADE only, no lock-in packets, screen still closed.
    let mut h2 = ClientPlayNetworkHandlerSeam::new(CommandDispatcher::new());
    h2.merchant_sync_id = 3;
    let mut al2 = AutoLectern::new();
    al2.set_state(ALState::WaitingTrade);
    let mut mc2 = MinecraftSeam::default();
    mc2.player = Some(player_with(vec![plain_stack(Item::Emerald, 1)]));
    mc2.level = Some(sharpness_world());
    assert!(client_play_network_handler_on_set_trade_offers(
        &mut h2, &mut mc2, &mut al2, &packet
    ));
    assert_eq!(al2.signals, SIGNAL_TRADE);
    assert_eq!(h2.sent_packets.len(), 1);
    assert_eq!(h2.sent_packets[0], C2sPacket::CloseHandledScreen { sync_id: 3 });

    // Sync id mismatch -> untouched.
    let mut h3 = ClientPlayNetworkHandlerSeam::new(CommandDispatcher::new());
    let mut al3 = AutoLectern::new();
    let mut mc3 = MinecraftSeam::default();
    assert!(!client_play_network_handler_on_set_trade_offers(
        &mut h3, &mut mc3, &mut al3, &packet
    ));
    assert!(h3.sent_packets.is_empty());
}

#[test]
fn mixin_client_command_intercept() {
    let mut h = ClientPlayNetworkHandlerSeam::new(CommandDispatcher::new());
    AutoLec::register(&mut h.dispatcher, &CommandBuildContext);
    let mut mc = MinecraftSeam::default();
    mc.player = Some(player_with(vec![pickaxe_stack(0, 200, 5.0)]));
    let mut al = AutoLectern::new();

    // Not a client-side command -> not cancelled.
    assert!(!client_play_network_handler_on_send_chat_command(
        &mut h, &mut mc, &mut al, "say hi"
    ));
    // Client-side command -> cancelled and executed.
    assert!(client_play_network_handler_on_send_chat_command(
        &mut h, &mut mc, &mut al, "autolec list"
    ));
    // Click-event path behaves the same.
    assert!(client_play_network_handler_send_command(
        &mut h, &mut mc, &mut al, "autolec list"
    ));
}

#[test]
fn mixin_item_pickup_signal() {
    let mut al = AutoLectern::new();
    al.set_state(ALState::WaitingItem);
    client_play_network_handler_on_item_pickup_animation(&mut al, Item::Lectern);
    assert_eq!(al.signals, SIGNAL_ITEM);

    let mut al2 = AutoLectern::new();
    al2.set_state(ALState::WaitingItem);
    client_play_network_handler_on_item_pickup_animation(&mut al2, Item::Other);
    assert_eq!(al2.signals, 0);

    let mut al3 = AutoLectern::new();
    client_play_network_handler_on_item_pickup_animation(&mut al3, Item::Lectern);
    assert_eq!(al3.signals, 0);
}
