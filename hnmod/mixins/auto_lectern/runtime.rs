// Auto Lectern for Horizon: farms librarian trades by breaking and
// re-placing a lectern in front of a villager until one offer matches a
// goal, trades it, and stops.
//
// A Rust port of AutoLectern (sys.exe.al, capthehacker99, CC BY-NC-SA 4.0,
// https://github.com/capthehacker99/AutoLectern). The original's behavior
// is kept:
//
// - `/autolec` drives everything; the command tree is upstream's AutoLec
//   verbatim: start, stop, sync item, breakCooldown, autoTrade
//   ENCHANT|CHEAPEST|OFF, log, preBreak, preserveTool, autoRemove, add,
//   clear, remove, list, with upstream's chat lines, the [REMOVE] click-run
//   on list rows and the Command Expired uuid check on remove.
// - Goals are upstream's ALGoal records: an enchantment id plus level and
//   price windows with -1 as the extreme-bound sentinel. The matcher keeps
//   upstream's quirks, including a -1 level minimum with a set maximum
//   accepting only level 1 or below, and a level maximum of -1 meaning "at
//   the enchantment's own max level".
// - The villager price helpers are upstream's: cheapest is maxLevel*3+2 and
//   dearest is maxLevel*3+2+(4+maxLevel*10), both doubled for the
//   double-trade-price tag and then capped at 64.
// - The config file is autolec.txt in upstream's exact format.
//
// The pure logic (goals, matching, config, the command tree) compiles
// standalone so its tests run anywhere; the game-touchpoint half (the tick
// loop and its host seams) is compiled out under `--features standalone`
// and lives in the bundle build, where the game crate is in scope.
//
// SPDX-License-Identifier: CC-BY-NC-SA-4.0
// Port of AutoLectern by capthehacker99 (CC BY-NC-SA 4.0).

/// The mod's id.
pub const ID: &str = "auto_lectern";
/// The command word the mod answers to (upstream registers only `autolec`).
pub const HEADS: [&str; 1] = ["autolec"];

/// `ALAutoTrade`'s modes: off, the first matching goal, or the cheapest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AutoTrade {
    Off,
    /// Trade the first goal that matches.
    Enchant,
    /// Trade the cheapest matching book.
    Cheapest,
}

/// `ALState`: the farm's phases.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Stopped,
    Stopping,
    Starting,
    Breaking,
    WaitingItem,
    Placing,
    WaitingProf,
    InteractVil,
    WaitingTrade,
}

/// The original's signal bits, ticked together and cleared on use.
pub const PROF: i32 = 1;
pub const TRADE: i32 = 2;
pub const TRADE_OK: i32 = 4;
pub const ITEM: i32 = 8;

/// `ALGoal`: one farming goal. `-1` is the extreme-bound sentinel.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Goal {
    pub enchant: String,
    pub lvl_min: i32,
    pub lvl_max: i32,
    pub price_min: i32,
    pub price_max: i32,
}

/// The enchantment facts the registry resolves for one id: its max level
/// and whether the villager's double-trade-price tag applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnchantSpec {
    pub max_level: i32,
    pub double_price: bool,
}

/// The registry seam the game side fills in: enchantment lookup, the
/// tradeable tag's members, and the display name for chat.
pub trait Registry {
    fn resolve(&self, id: &str) -> Option<EnchantSpec>;
    fn tradeable(&self) -> Vec<String>;
    fn display(&self, id: &str) -> Option<String>;
}

/// The store: one of upstream's AutoLectern singletons. Field names follow
/// the original; the defaults are onInitializeClient's.
#[derive(Clone, Debug)]
pub struct Store {
    pub auto_trade: AutoTrade,
    pub item_sync: bool,
    pub break_cooldown: bool,
    pub log_trade: bool,
    pub pre_breaking: bool,
    pub preserve_tool: bool,
    pub auto_remove: bool,
    pub attempts: i32,
    pub uuid: i32,
    pub signals: i32,
    pub last_goal_met: i32,
    pub prev_selected_slot: i32,
    /// The lectern block, as `[x, y, z]`, plus the face the crosshair hit
    /// (upstream's lecternSide, the abort-dig packet's face).
    pub lectern_pos: Option<[i32; 3]>,
    pub lectern_side: Option<u8>,
    pub cur_state: State,
    pub tick_cooldown: i32,
    /// The villager the PROF signal picked (its mob id in the host).
    pub updated_villager: Option<u32>,
    pub goals: Vec<Goal>,
    pub lec_pitch: f32,
    pub lec_yaw: f32,
    pub fake_pitch: f32,
    pub fake_yaw: f32,
    pub forced_pos: Option<[f64; 3]>,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            auto_trade: AutoTrade::Off,
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
            prev_selected_slot: 0,
            lectern_pos: None,
            lectern_side: None,
            cur_state: State::Stopped,
            tick_cooldown: 0,
            updated_villager: None,
            goals: Vec::new(),
            lec_pitch: 0.0,
            lec_yaw: 0.0,
            fake_pitch: 0.0,
            fake_yaw: 0.0,
            forced_pos: None,
        }
    }
}

impl Store {
    /// `signal`: mark one signal bit for the tick to read.
    pub fn signal(&mut self, signal_type: i32) {
        self.signals |= signal_type;
    }
}

/// `getCheapestVillagerEnchant`: `maxLevel*3+2`, doubled for the
/// double-trade-price tag, then capped at 64 (the cap comes last).
pub fn cheapest_enchant(spec: &EnchantSpec) -> i32 {
    let mut price = (spec.max_level * 3) + 2;
    if spec.double_price {
        price *= 2;
    }
    if price > 64 { 64 } else { price }
}

/// `getMostExpensiveVillagerEnchant`: `maxLevel*3+2+(4+maxLevel*10)`,
/// doubled for the tag, then capped at 64.
pub fn most_expensive_enchant(spec: &EnchantSpec) -> i32 {
    let k = spec.max_level;
    let mut price = (k * 3) + 2 + (4 + k * 10);
    if spec.double_price {
        price *= 2;
    }
    if price > 64 { 64 } else { price }
}

/// `isGoalLevelMet`, quirks included: a -1 minimum with a set maximum
/// accepts only level 1 or below, and a -1 maximum with a set minimum means
/// "at least the enchantment's own max level".
pub fn is_goal_level_met(max_enc_lvl: i32, lvl_min: i32, lvl_max: i32, lvl: i32) -> bool {
    if lvl_min == -1 {
        if lvl_max == -1 {
            return true;
        }
        return lvl <= 1;
    }
    if lvl_max == -1 {
        return lvl >= max_enc_lvl;
    }
    lvl >= lvl_min && lvl <= lvl_max
}

/// `isGoalPriceMet`: a -1 minimum with a set maximum means "at or under the
/// enchantment's cheapest villager price", and a -1 maximum with a set
/// minimum means "at or over its most expensive".
pub fn is_goal_price_met(spec: &EnchantSpec, price_min: i32, price_max: i32, price: i32) -> bool {
    if price_min == -1 {
        if price_max == -1 {
            return true;
        }
        return price <= cheapest_enchant(spec);
    }
    if price_max == -1 {
        return price >= most_expensive_enchant(spec);
    }
    price >= price_min && price <= price_max
}

/// `getGoalMet`: the first goal the offer meets, counted over the goals
/// whose enchantment resolves (upstream's index skips unresolved ones).
pub fn goal_met(
    goals: &[Goal],
    registry: &dyn Registry,
    offer_enchant: &str,
    offer_lvl: i32,
    offer_price: i32,
) -> Option<usize> {
    let mut idx = 0usize;
    for goal in goals {
        let Some(spec) = registry.resolve(&goal.enchant) else {
            continue;
        };
        if offer_enchant == goal.enchant
            && is_goal_level_met(spec.max_level, goal.lvl_min, goal.lvl_max, offer_lvl)
            && is_goal_price_met(&spec, goal.price_min, goal.price_max, offer_price)
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

/// The item kinds upstream's trade-cost checks branch on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CostItem {
    Emerald,
    Paper,
    Book,
    Other,
}

/// What the inventory seam counts from the player's non-equipment stacks:
/// upstream's availEmerald / availBook / availPaper.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counts {
    pub emerald: i32,
    pub book: i32,
    pub paper: i32,
}

/// One merchant offer as the menu carries it. The cost stacks keep the
/// item kind so the trade checks branch exactly as upstream does on
/// EMERALD / PAPER / BOOK.
#[derive(Clone, Debug)]
pub struct Offer {
    /// The result book's enchantment id (`minecraft:sharpness`).
    pub enchant: String,
    /// The result book's level.
    pub level: i32,
    /// The cost before demand is applied (`getBaseCostA().getCount()`).
    pub base_cost: i32,
    /// The cost as the menu shows it.
    pub cost: i32,
    /// The offer's first cost stack: the item kind and its count.
    pub cost_a: (CostItem, i32),
    /// The offer's second cost stack, when it has one.
    pub cost_b: Option<(CostItem, i32)>,
    /// The result is an enchanted book (`offer.getResult()` is
    /// Items.ENCHANTED_BOOK).
    pub result_book: bool,
    /// `EnchantmentHelper.canStoreEnchantments(result)`.
    pub can_store: bool,
    /// The result's enchantments as (id, level) pairs, empty when none.
    pub result_enchants: Vec<(String, i32)>,
}

/// Upstream's findCheapestTrade: the first affordable offer with the
/// smallest weighted first cost. Weights: an emerald 1.0, paper 0.015625,
/// a book 0.146875; any other first cost is skipped. Ties keep the
/// earlier index (upstream's strict `<`).
pub fn find_cheapest_trade(offers: &[Offer], counts: &Counts) -> i32 {
    let mut min_idx: i32 = -1;
    let mut min_cost = f32::MAX;
    let mut cur_idx: i32 = -1;
    for offer in offers {
        cur_idx += 1;
        let mut cur_cost = 0.0f32;
        let (item, count) = offer.cost_a;
        match item {
            CostItem::Emerald => {
                if count > counts.emerald {
                    continue;
                }
                cur_cost += count as f32;
            }
            CostItem::Paper => {
                if count > counts.paper {
                    continue;
                }
                cur_cost += count as f32 * 0.015625;
            }
            CostItem::Book => {
                if count > counts.book {
                    continue;
                }
                cur_cost += count as f32 * 0.146875;
            }
            CostItem::Other => continue,
        }
        if cur_cost < min_cost {
            min_idx = cur_idx;
            min_cost = cur_cost;
        }
    }
    min_idx
}

/// Vanilla's enchantment fullname level suffix: a roman numeral from 1 to
/// 10, the plain number past that (RomanNumeral.getOrFull). Shown when the
/// level is not 1 or the enchant's own max level is not 1.
pub fn roman_numeral(level: i32) -> String {
    match level {
        1 => "I".to_string(),
        2 => "II".to_string(),
        3 => "III".to_string(),
        4 => "IV".to_string(),
        5 => "V".to_string(),
        6 => "VI".to_string(),
        7 => "VII".to_string(),
        8 => "VIII".to_string(),
        9 => "IX".to_string(),
        10 => "X".to_string(),
        other => other.to_string(),
    }
}

/// The fullname one enchantment entry logs with, upstream's
/// `Enchantment.getFullname(enchant, lvl)`: the registry display plus the
/// level's numeral whenever the level is not 1 or the enchant's own max
/// level is not 1.
fn enchant_fullname(registry: &dyn Registry, enchant: &str, lvl: i32) -> String {
    let name = registry.display(enchant).unwrap_or_else(|| enchant.to_string());
    let max = registry.resolve(enchant).map(|s| s.max_level).unwrap_or(1);
    if lvl != 1 || max != 1 {
        format!("{name} {}", roman_numeral(lvl))
    } else {
        name
    }
}

/// Upstream's checkTrades: the first offer whose result book carries an
/// enchantment meeting a goal wins; every scanned entry logs when
/// log_trade is on, in upstream's colors. Returns the winning offer index
/// or -1, with the log messages that were sent.
pub fn check_trades(
    store: &mut Store,
    registry: &dyn Registry,
    offers: &[Offer],
) -> (i32, Vec<Msg>) {
    let mut msgs = Vec::new();
    for (cur_idx, offer) in offers.iter().enumerate() {
        if !offer.result_book || !offer.can_store || offer.result_enchants.is_empty() {
            continue;
        }
        for (enchant, lvl) in &offer.result_enchants {
            if store.log_trade {
                msgs.push(msg(vec![
                    part(Color::Green, format!("{}", offer.base_cost), None),
                    part(Color::White, " emeralds for ", None),
                    part(
                        Color::White,
                        enchant_fullname(registry, enchant, *lvl),
                        None,
                    ),
                    part(
                        Color::White,
                        format!(" [{}]", store.attempts),
                        None,
                    ),
                ]));
            }
            if let Some(gi) = goal_met(&store.goals, registry, enchant, *lvl, offer.base_cost) {
                store.last_goal_met = gi as i32;
                return (cur_idx as i32, msgs);
            }
        }
    }
    (-1, msgs)
}

/// One packet upstream sends to lock a trade in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TradeAction {
    /// ServerboundSelectTradePacket(index).
    SelectTrade(i32),
    /// ServerboundContainerClickPacket onto the merchant's result slot 2
    /// (stateId 6, button 0, ContainerInput.PICKUP), first carrying the
    /// offer's result and then, on an empty inventory slot, carrying
    /// nothing.
    Click { slot: i32, carried_empty: bool },
}

/// Upstream's attemptLockTrade: the packet sequence that buys the picked
/// offer's book and drops it into a free inventory slot. Returns nothing
/// to send when the inventory has no free slot or the offer's costs
/// exceed what it holds.
pub fn attempt_lock_trade(
    store: &Store,
    offers: &[Offer],
    counts: &Counts,
    tar_idx: i32,
    free_slot: i32,
) -> Vec<TradeAction> {
    if free_slot == -1 {
        return Vec::new();
    }
    let click = |slot: i32, carried_empty: bool| TradeAction::Click { slot, carried_empty };
    if store.auto_trade == AutoTrade::Enchant {
        let Some(tar) = offers.get(tar_idx as usize) else {
            return Vec::new();
        };
        let (first_item, first_count) = tar.cost_a;
        if first_item != CostItem::Emerald {
            return Vec::new();
        }
        let Some((second_item, second_count)) = tar.cost_b else {
            return Vec::new();
        };
        if second_item != CostItem::Book {
            return Vec::new();
        }
        if first_count > counts.emerald || second_count > counts.book {
            return Vec::new();
        }
        vec![
            TradeAction::SelectTrade(tar_idx),
            click(2, false),
            click(free_slot, true),
        ]
    } else {
        let min_idx = find_cheapest_trade(offers, counts);
        if min_idx == -1 {
            return Vec::new();
        }
        vec![
            TradeAction::SelectTrade(min_idx),
            click(2, false),
            click(free_slot, true),
        ]
    }
}

/// Upstream's determineTrades: the merchant-offers packet's decision step.
/// Counts the attempt, checks the offers for a goal, signals TRADE with
/// or without TRADE_OK, and, when auto-trade is on, sends the lock-in
/// packets. The caller gates on WAITING_TRADE and a player being present,
/// then sends the merchant container close afterwards.
pub fn determine_trades(
    store: &mut Store,
    registry: &dyn Registry,
    offers: &[Offer],
    counts: &Counts,
    free_slot: i32,
) -> (i32, Vec<TradeAction>, Vec<Msg>) {
    store.attempts += 1;
    let (tar_idx, msgs) = check_trades(store, registry, offers);
    if tar_idx == -1 {
        store.signal(TRADE);
        return (tar_idx, Vec::new(), msgs);
    }
    store.signal(TRADE);
    store.signal(TRADE_OK);
    if store.auto_trade != AutoTrade::Off {
        let actions = attempt_lock_trade(store, offers, counts, tar_idx, free_slot);
        (tar_idx, actions, msgs)
    } else {
        (tar_idx, Vec::new(), msgs)
    }
}

/// Upstream's MinecraftMixin tick-head keepalive: while the farm runs the
/// client's idle timer is kept fed (`lastActiveTime = Util.getMillis()`).
pub fn keeps_client_active(state: State) -> bool {
    state != State::Stopped
}

/// Upstream's MinecraftMixin onStop: the config is written on a clean
/// client stop (`running` true).
pub fn saves_on_stop(running: bool) -> bool {
    running
}

/// Upstream's MinecraftMixin continueAttack wrap: while the farm runs the
/// client's own stopDestroyBlock call is skipped.
pub fn skips_stop_destroy(state: State) -> bool {
    state != State::Stopped
}

/// Upstream's onUpdateBlockBreakingProgress gate: zero the dig delay
/// while the farm is breaking (or pre-breaking toward a villager state)
/// and the break-cooldown flag is off.
pub fn zero_destroy_delay(break_cooldown: bool, pre_breaking: bool, state: State) -> bool {
    !break_cooldown
        && (state == State::Breaking
            || (pre_breaking && (state == State::WaitingProf || state == State::WaitingTrade)))
}

/// Upstream's antiBlockBreaking: while pre-breaking, a dig that would
/// finish this tick (the kept progress plus this tick's share reaching
/// 1.0) is held at zero progress in every state except stopped and
/// breaking.
pub fn anti_block_breaking(
    before: f32,
    destroy_progress: f32,
    pre_breaking: bool,
    state: State,
) -> f32 {
    if destroy_progress + before < 1.0 {
        return before;
    }
    if pre_breaking && state != State::Stopped && state != State::Breaking {
        return 0.0;
    }
    before
}

/// The rotation override upstream's mixins apply on the position-send,
/// changes-send, use-item and move-player paths: while the farm runs every
/// rotation read returns the farm's fake angles instead of the player's
/// (`AL.getYaw()` / `AL.getPitch()` are fakeYaw / fakePitch).
pub fn rotation_override(state: State, fake_pitch: f32, fake_yaw: f32) -> Option<(f32, f32)> {
    (state != State::Stopped).then_some((fake_pitch, fake_yaw))
}

/// Upstream's onOpenScreen gate: in WAITING_TRADE a merchant menu is
/// captured (its container id stored) and its screen suppressed.
pub fn captures_merchant(state: State, menu_is_merchant: bool) -> bool {
    state == State::WaitingTrade && menu_is_merchant
}

/// Upstream's onCloseScreen gate: the server-side close of the captured
/// merchant menu is swallowed (and the capture dropped).
pub fn swallows_merchant_close(captured: bool, closing_id: i32, captured_id: i32) -> bool {
    captured && closing_id == captured_id
}

/// The villager-profession pick, upstream's entity-data post-hook: in
/// WAITING_PROF with no villager picked yet, a villager within a squared
/// distance of 8 from the lectern's center that just became a librarian
/// without having been one before wins.
pub fn picks_prof_villager(
    state: State,
    updated: Option<u32>,
    lectern_pos: [i32; 3],
    villager_pos: [f64; 3],
    is_librarian: bool,
    was_librarian: bool,
) -> bool {
    if state != State::WaitingProf || updated.is_some() {
        return false;
    }
    // BlockPos.distToCenterSqr: the squared distance to the block's center.
    let dx = villager_pos[0] - (lectern_pos[0] as f64 + 0.5);
    let dy = villager_pos[1] - (lectern_pos[1] as f64 + 0.5);
    let dz = villager_pos[2] - (lectern_pos[2] as f64 + 0.5);
    if dx * dx + dy * dy + dz * dz > 8.0 {
        return false;
    }
    is_librarian && !was_librarian
}

/// The chat model one upstream message becomes: the yellow `[Auto Lectern]`
/// prefix plus colored parts, with an optional click-run command on a part
/// (upstream's `ClickEvent.RunCommand` on the [REMOVE] pieces).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Yellow,
    Red,
    Green,
    White,
    Gray,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Part {
    pub color: Color,
    pub text: String,
    pub run: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Msg {
    pub parts: Vec<Part>,
}

const PREFIX: &str = "[Auto Lectern] ";

fn part(color: Color, text: impl Into<String>, run: Option<String>) -> Part {
    Part {
        color,
        text: text.into(),
        run,
    }
}

/// One message: the yellow prefix, then the parts.
fn msg(parts: Vec<Part>) -> Msg {
    let mut all = vec![part(Color::Yellow, PREFIX, None)];
    all.extend(parts);
    Msg { parts: all }
}

/// `enchantLvlInfo`, verbatim including the leading space and comma.
pub fn enchant_lvl_info(min: i32, max: i32) -> String {
    if min == -1 {
        if max == -1 {
            return " any level,".into();
        }
        return " lowest level,".into();
    }
    if max == -1 {
        return " max level,".into();
    }
    format!(" level from {min} to {max},")
}

/// `priceInfo`, verbatim.
pub fn price_info(min: i32, max: i32) -> String {
    if min == -1 {
        if max == -1 {
            return " any price.".into();
        }
        return " cheapest.".into();
    }
    if max == -1 {
        return " most expensive.".into();
    }
    format!(" from {min} to {max} emeralds.")
}

/// `loadConfig`: upstream's autolec.txt reader. Keys not in the switch and
/// lines that fail to parse are skipped, like the original's try/catch.
pub fn load_config(store: &mut Store, text: &str) {
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "breakCooldown" => store.break_cooldown = value == "true",
            "itemSync" => store.item_sync = value == "true",
            "preserveTool" => store.preserve_tool = value == "true",
            "logTrade" => store.log_trade = value == "true",
            "preBreak" => store.pre_breaking = value == "true",
            "autoRemove" => store.auto_remove = value == "true",
            "autoTrade" => {
                store.auto_trade = match value {
                    "ENCHANT" => AutoTrade::Enchant,
                    "CHEAPEST" => AutoTrade::Cheapest,
                    _ => AutoTrade::Off,
                }
            }
            "goals" => {
                let mut record: Vec<Goal> = Vec::new();
                for goal_data in value.split(';') {
                    if goal_data.is_empty() {
                        continue;
                    }
                    let mut fields = goal_data.split(',');
                    let enchant = fields.next();
                    let nums: Vec<Result<i32, _>> = fields.map(|f| f.parse::<i32>()).collect();
                    // Upstream reads exactly five fields and ignores extras;
                    // a short record throws and drops the whole line.
                    if let (
                        Some(enchant),
                        [Ok(lvl_min), Ok(lvl_max), Ok(price_min), Ok(price_max)],
                    ) = (enchant, nums.as_slice())
                    {
                        record.push(Goal {
                            enchant: enchant.to_string(),
                            lvl_min: *lvl_min,
                            lvl_max: *lvl_max,
                            price_min: *price_min,
                            price_max: *price_max,
                        });
                    } else {
                        // Upstream's catch drops the whole line, including
                        // the records it added before the bad one.
                        record.clear();
                        break;
                    }
                }
                store.goals.extend(record);
            }
            _ => {}
        }
    }
}

/// `saveConfig`: upstream's writer, its field order kept.
pub fn save_config(store: &Store) -> String {
    let mut out = String::new();
    out.push_str(&format!("itemSync={}\n", store.item_sync));
    out.push_str(&format!("breakCooldown={}\n", store.break_cooldown));
    out.push_str(&format!("logTrade={}\n", store.log_trade));
    out.push_str(&format!("preBreak={}\n", store.pre_breaking));
    out.push_str(&format!("preserveTool={}\n", store.preserve_tool));
    out.push_str(&format!("autoRemove={}\n", store.auto_remove));
    out.push_str(&format!(
        "autoTrade={}\n",
        format!("{:?}", store.auto_trade).to_uppercase()
    ));
    out.push_str("goals=");
    for goal in &store.goals {
        out.push_str(&goal.enchant);
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
    out
}

/// `addToGoal`: the goal goes in with the level and price windows; with no
/// enchantment named, one goal per tradeable enchantment goes in instead.
/// The uuid ticks either way, and nothing is said to chat.
fn add_to_goal(
    store: &mut Store,
    enchant: Option<&str>,
    registry: &dyn Registry,
    lvl_min: i32,
    lvl_max: i32,
    price_min: i32,
    price_max: i32,
) {
    match enchant {
        Some(id) => store.goals.push(Goal {
            enchant: id.to_string(),
            lvl_min,
            lvl_max,
            price_min,
            price_max,
        }),
        None => {
            for id in registry.tradeable() {
                store.goals.push(Goal {
                    enchant: id,
                    lvl_min,
                    lvl_max,
                    price_min,
                    price_max,
                });
            }
        }
    }
    store.uuid += 1;
}

/// `listGoals`: the "Goals:" line, then one row per goal whose enchantment
/// resolves, each with a [REMOVE] click-run. The index counts only the rows
/// shown, upstream's quirk, so it can drift off the list's real indices.
fn list_goals(store: &Store, registry: &dyn Registry) -> Vec<Msg> {
    let mut out = vec![msg(vec![part(Color::White, "Goals:", None)])];
    let mut i = 0;
    for goal in &store.goals {
        let Some(display) = registry.display(&goal.enchant) else {
            continue;
        };
        let run = format!("/autolec remove {i} {}", store.uuid);
        out.push(msg(vec![
            part(Color::Yellow, format!("[{i}] "), None),
            part(
                Color::White,
                format!(
                    "{display}{}{}",
                    enchant_lvl_info(goal.lvl_min, goal.lvl_max),
                    price_info(goal.price_min, goal.price_max)
                ),
                None,
            ),
            part(Color::Red, " [REMOVE]", Some(run)),
        ]));
        i += 1;
    }
    out
}

/// `removeGoal`: the Command Expired check on the uuid, then the swap with
/// the last goal and the re-list; out of bounds says so. Every path resets
/// the chat scroll (the game side reads this and scrolls).
fn remove_goal(store: &mut Store, registry: &dyn Registry, index: i32, uuid: i32) -> Vec<Msg> {
    if uuid != -1 && uuid != store.uuid {
        return vec![msg(vec![part(
            Color::Red,
            "Command Expired.\nType \"/autolec list\" then choose again.",
            None,
        )])];
    }
    let max = store.goals.len() as i32 - 1;
    if max >= index {
        let idx = index as usize;
        let last = store.goals.len() - 1;
        store.goals.swap(idx, last);
        store.goals.remove(last);
        store.uuid += 1;
        return list_goals(store, registry);
    }
    vec![msg(vec![part(Color::Red, "Index out of bounds.", None)])]
}

/// `/autolec`: upstream's AutoLec tree, one command line at a time. The
/// head word is already stripped. Numbers are upstream's 0..=MAX integer
/// arguments, so a negative fails the same way brigadier does.
pub fn run_command(store: &mut Store, registry: &dyn Registry, line: &str) -> Vec<Msg> {
    let args: Vec<&str> = line.split_whitespace().collect();
    let usage = || vec![msg(vec![part(Color::Red, USAGE, None)])];
    let Some(head) = args.first().copied() else {
        return usage();
    };
    match head {
        "start" => {
            if store.cur_state != State::Stopped {
                return vec![msg(vec![part(
                    Color::Red,
                    "Please stop before starting again.",
                    None,
                )])];
            }
            store.cur_state = State::Starting;
            vec![]
        }
        "stop" => {
            if store.cur_state == State::Stopped {
                return vec![msg(vec![part(Color::Red, "Already stopped.", None)])];
            }
            store.cur_state = State::Stopping;
            vec![]
        }
        "sync" => {
            if args.get(1).copied() != Some("item") {
                return usage();
            }
            store.item_sync = !store.item_sync;
            vec![msg(vec![part(
                Color::White,
                format!("Item Sync is now {}", on_off(store.item_sync)),
                None,
            )])]
        }
        "breakCooldown" => {
            store.break_cooldown = !store.break_cooldown;
            vec![msg(vec![part(
                Color::White,
                format!("Break cooldown is now {}", on_off(store.break_cooldown)),
                None,
            )])]
        }
        "autoTrade" => {
            let Some(mode) = args.get(1).copied() else {
                return usage();
            };
            store.auto_trade = match mode {
                "ENCHANT" => AutoTrade::Enchant,
                "CHEAPEST" => AutoTrade::Cheapest,
                "OFF" => AutoTrade::Off,
                _ => return usage(),
            };
            let name = match store.auto_trade {
                AutoTrade::Enchant => "ENCHANT",
                AutoTrade::Cheapest => "CHEAPEST",
                AutoTrade::Off => "OFF",
            };
            vec![msg(vec![part(
                Color::White,
                format!("Auto Trading Mode is now set to {name}."),
                None,
            )])]
        }
        "log" => {
            store.log_trade = !store.log_trade;
            vec![msg(vec![part(
                Color::White,
                format!("Logging is now {}", on_off(store.log_trade)),
                None,
            )])]
        }
        "preBreak" => {
            store.pre_breaking = !store.pre_breaking;
            vec![msg(vec![part(
                Color::White,
                format!("Pre breaking is now {}", on_off(store.pre_breaking)),
                None,
            )])]
        }
        "preserveTool" => {
            store.preserve_tool = !store.preserve_tool;
            vec![msg(vec![part(
                Color::White,
                format!("Tool preserving is now {}", on_off(store.preserve_tool)),
                None,
            )])]
        }
        "add" => add_command(store, registry, &args[1..]),
        "clear" => {
            store.goals.clear();
            store.uuid += 1;
            vec![msg(vec![part(Color::Green, "All goals removed.", None)])]
        }
        "remove" => {
            let Some(index) = args
                .get(1)
                .and_then(|s| s.parse::<i32>().ok())
                .filter(|n| *n >= 0)
            else {
                return usage();
            };
            let uuid = match args.get(2) {
                None => -1,
                Some(s) => match s.parse::<i32>() {
                    Ok(n) if n >= 0 => n,
                    _ => return usage(),
                },
            };
            remove_goal(store, registry, index, uuid)
        }
        "autoRemove" => {
            store.auto_remove = !store.auto_remove;
            vec![msg(vec![part(
                Color::White,
                format!("Auto remove is now {}", on_off(store.auto_remove)),
                None,
            )])]
        }
        "list" => list_goals(store, registry),
        _ => usage(),
    }
}

/// The `add` subtree: `[<enchantment>] (min|max|any|<minLevel> <maxLevel>)
/// (min|max|any|<minPrice> <maxPrice>)`, the two sentinel encodings kept
/// (-2 marks the "read both level numbers" shape).
fn add_command(store: &mut Store, registry: &dyn Registry, args: &[&str]) -> Vec<Msg> {
    let usage = || vec![msg(vec![part(Color::Red, USAGE, None)])];
    let (enchant, rest) = match args.first().copied() {
        // `add min ...`: the first token is a level word, so no enchantment.
        Some(w) if w == "min" || w == "max" || w == "any" => (None, &args[..]),
        Some(_) => (Some(args[0]), &args[1..]),
        None => return usage(),
    };
    match rest {
        ["min", rest @ ..] => match price_shape(rest) {
            Some((price_min, price_max)) => {
                return push_add(store, registry, enchant, -1, 0, price_min, price_max);
            }
            None => return usage(),
        },
        ["max", rest @ ..] => match price_shape(rest) {
            Some((price_min, price_max)) => {
                return push_add(store, registry, enchant, 0, -1, price_min, price_max);
            }
            None => return usage(),
        },
        ["any", rest @ ..] => match price_shape(rest) {
            Some((price_min, price_max)) => {
                return push_add(store, registry, enchant, -1, -1, price_min, price_max);
            }
            None => return usage(),
        },
        [min_level, max_level, rest @ ..] => {
            let (Ok(min_level), Ok(max_level)) =
                (min_level.parse::<i32>(), max_level.parse::<i32>())
            else {
                return usage();
            };
            if min_level < 0 || max_level < 0 {
                return usage();
            }
            match price_shape(rest) {
                Some((price_min, price_max)) => {
                    return push_add(
                        store, registry, enchant, min_level, max_level, price_min, price_max,
                    );
                }
                None => return usage(),
            }
        }
        _ => return usage(),
    }
}

/// One `add` execution: the sentinels resolved, the goal (or the tradeable
/// expansion) in, the uuid ticked, and nothing said to chat.
fn push_add(
    store: &mut Store,
    registry: &dyn Registry,
    enchant: Option<&str>,
    lvl_min: i32,
    lvl_max: i32,
    price_min: i32,
    price_max: i32,
) -> Vec<Msg> {
    add_to_goal(
        store, enchant, registry, lvl_min, lvl_max, price_min, price_max,
    );
    vec![]
}

/// The price half of `add`: min|max|any or two numbers.
fn price_shape(rest: &[&str]) -> Option<(i32, i32)> {
    match rest {
        ["min"] => Some((-1, 0)),
        ["max"] => Some((0, -1)),
        ["any"] => Some((-1, -1)),
        [min_price, max_price] => {
            let (Ok(min_price), Ok(max_price)) =
                (min_price.parse::<i32>(), max_price.parse::<i32>())
            else {
                return None;
            };
            (min_price >= 0 && max_price >= 0).then_some((min_price, max_price))
        }
        _ => None,
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "ON" } else { "OFF" }
}

/// The one-line how-the-command-reads, shown when a line is not understood.
pub const USAGE: &str = "Usage: /autolec [start | stop | sync item | breakCooldown | autoTrade ENCHANT|CHEAPEST|OFF | log | preBreak | preserveTool | autoRemove | add [enchant] min|max|any|min max min|max|any|priceMin priceMax | clear | remove index [uuid] | list]";

// --- The game-touchpoint half -------------------------------------------
// Compiled only in the bundle build, where `crate::game` is the host game
// crate; the standalone test build compiles it out. Every host touch goes
// through a thin seam so a staging mismatch is fixed in that one place;
// each seam's `stage:` comment names the upstream call it stands in for.

/// The offer carrier the host's merchant hook fills in.
/// The registry seam against the host: enchantment lookup, the tradeable
/// tag and the display names. The stage pass wires the real calls; until
/// then lookups resolve nothing.
#[cfg(not(feature = "standalone"))]
struct GameRegistry;

#[cfg(not(feature = "standalone"))]
impl Registry for GameRegistry {
    fn resolve(&self, id: &str) -> Option<EnchantSpec> {
        // stage: the game's enchantment registry (its max level and the
        // double-trade-price tag).
        let _ = id;
        None
    }
    fn tradeable(&self) -> Vec<String> {
        // stage: the tradeable enchantment tag's members.
        Vec::new()
    }
    fn display(&self, id: &str) -> Option<String> {
        // stage: the enchantment's chat description.
        let _ = id;
        None
    }
}

/// What the inventory seam reports about one stack (upstream reads the
/// TOOL component and the mining speed against a lectern).
#[cfg(not(feature = "standalone"))]
#[derive(Clone, Copy, Debug)]
pub struct ToolFacts {
    pub is_tool: bool,
    pub damageable: bool,
    pub damage: i32,
    pub max_damage: i32,
    pub mining_speed_lectern: f32,
}

/// Which hand `equipLectern` put the lectern block into.
#[cfg(not(feature = "standalone"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hand {
    Main,
    Off,
}

#[cfg(not(feature = "standalone"))]
struct Runner {
    store: Store,
    /// The merchant menu the farm intercepted.
    trading: bool,
    /// The offer the offer hook picked (the met goal acts on it).
    met: Option<Offer>,
    /// The captured merchant container id (upstream's merchantSyncId).
    merchant_sync: i32,
    /// Log lines the offer hook queued for the next frame.
    pending_msgs: Vec<Msg>,
    /// Trade lock-in packets the offer hook queued for the next frame.
    pending_actions: Vec<TradeAction>,
}

#[cfg(not(feature = "standalone"))]
static RUNNER: Mutex<Runner> = Mutex::new(Runner {
    store: Store {
        auto_trade: AutoTrade::Off,
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
        prev_selected_slot: 0,
        lectern_pos: None,
        lectern_side: None,
        cur_state: State::Stopped,
        tick_cooldown: 0,
        updated_villager: None,
        goals: Vec::new(),
        lec_pitch: 0.0,
        lec_yaw: 0.0,
        fake_pitch: 0.0,
        fake_yaw: 0.0,
        forced_pos: None,
    },
    trading: false,
    met: None,
    merchant_sync: -1,
    pending_msgs: Vec::new(),
    pending_actions: Vec::new(),
});

#[cfg(not(feature = "standalone"))]
fn r_store_lock() -> std::sync::MutexGuard<'static, Runner> {
    RUNNER.lock().unwrap_or_else(|e| e.into_inner())
}

/// One game tick of the farm, called from the frame hook.
#[cfg(not(feature = "standalone"))]
pub fn frame(game: &mut crate::game::Game<'_>, _elapsed: std::time::Duration) {
    let mut r = r_store_lock();
    if r.store.cur_state == State::Stopped {
        return;
    }
    // The head guard: no player, or a menu other than the player's own
    // inventory open, stops the farm.
    if !seam_in_main_inventory(game) {
        r.store.cur_state = State::Stopping;
    }
    // The offer hook's work waits for this frame: upstream sends the
    // trade lock-in packets and the log lines straight off the
    // merchant-offers packet.
    // stage: send each pending TradeAction to the server
    // (ServerboundSelectTradePacket, then the two result-slot clicks).
    r.pending_actions.clear();
    let pending = std::mem::take(&mut r.pending_msgs);
    for m in &pending {
        say_msg(game, m);
    }
    tick_head(game, &mut r);
}

/// The state machine, upstream's MinecraftTickHead: a `while (true)` over
/// the state switch where `continue 'tick` re-enters the switch inside the
/// same tick and `break` ends the tick (upstream's `return`).
#[cfg(not(feature = "standalone"))]
fn tick_head(game: &mut crate::game::Game<'_>, r: &mut Runner) {
    'tick: loop {
        match r.store.cur_state {
            State::Stopped => break,
            State::Stopping => {
                if seam_in_main_inventory(game) {
                    if let Some(pos) = r.store.lectern_pos {
                        seam_abort_destroy(game, pos);
                    }
                }
                r.store.forced_pos = None;
                r.store.prev_selected_slot = -1;
                r.store.signals = 0;
                r.store.lectern_pos = None;
                r.store.lectern_side = None;
                r.store.updated_villager = None;
                say_msg(game, &msg(vec![part(Color::Red, "Stopped.", None)]));
                r.store.cur_state = State::Stopped;
                break;
            }
            State::Starting => {
                r.store.attempts = 0;
                r.store.prev_selected_slot = -1;
                r.store.signals = 0;
                r.store.updated_villager = None;
                if !seam_in_world(game) {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                }
                let Some((pos, side)) = lectern_under_crosshair(game) else {
                    say_msg(
                        game,
                        &msg(vec![part(
                            Color::Red,
                            "Please look at a lectern before running this command.",
                            None,
                        )]),
                    );
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                };
                // DummyInput is the movement lock below; the walk inputs
                // stay zeroed while the farm runs.
                r.store.lec_pitch = seam_pitch(game);
                r.store.lec_yaw = seam_yaw(game);
                r.store.fake_pitch = r.store.lec_pitch;
                r.store.fake_yaw = r.store.lec_yaw;
                r.store.forced_pos = Some(seam_position(game));
                r.store.lectern_pos = Some(pos);
                r.store.lectern_side = Some(side);
                r.store.cur_state = State::Breaking;
                continue 'tick;
            }
            State::Breaking => {
                if !seam_in_world(game) || !seam_game_mode(game) {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                }
                // Upstream reads lecternPos unguarded here; a missing
                // position stops the farm instead of panicking.
                let Some(pos) = r.store.lectern_pos else {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                };
                if seam_block_replaceable(game, pos) {
                    r.store.cur_state = if r.store.item_sync {
                        State::WaitingItem
                    } else {
                        State::Placing
                    };
                    continue 'tick;
                }
                r.store.fake_pitch = r.store.lec_pitch;
                r.store.fake_yaw = r.store.lec_yaw;
                seam_move_self(game, r.store.forced_pos);
                if r.store.prev_selected_slot != -1 {
                    seam_select_hotbar(game, r.store.prev_selected_slot);
                    r.store.prev_selected_slot = -1;
                }
                if r.store.preserve_tool {
                    if check_preserve_tool(game, r) {
                        continue 'tick;
                    }
                } else if seam_main_hand_empty(game) {
                    equip_working_tool(game);
                }
                break_dig(game, pos);
            }
            State::WaitingItem => {
                seam_move_self(game, r.store.forced_pos);
                if (r.store.signals & ITEM) != 0 {
                    r.store.cur_state = State::Placing;
                    continue 'tick;
                }
                break;
            }
            State::Placing => {
                if !seam_in_world(game) {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                }
                r.store.fake_pitch = r.store.lec_pitch;
                r.store.fake_yaw = r.store.lec_yaw;
                let Some(pos) = r.store.lectern_pos else {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                };
                if seam_block_is_lectern(game, pos) {
                    r.store.updated_villager = None;
                    r.store.tick_cooldown = 40;
                    r.store.signals = 0;
                    r.store.cur_state = State::WaitingProf;
                }
                if !seam_game_mode(game) {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                }
                let Some(hit) = get_looking_at(game, r) else {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                };
                if let Some(hand) = equip_lectern(game, r) {
                    seam_use_item_on(game, &hit, hand);
                }
                if !seam_block_is_lectern(game, pos) {
                    // Not placed yet: try again next tick (upstream returns).
                    break;
                }
                r.store.updated_villager = None;
                r.store.tick_cooldown = 40;
                r.store.signals = 0;
                r.store.cur_state = State::WaitingProf;
                // Falls out: the switch loops again this tick.
            }
            State::WaitingProf => {
                if (r.store.signals & PROF) == 0 {
                    seam_move_self(game, r.store.forced_pos);
                    if !seam_in_world(game) {
                        r.store.cur_state = State::Stopping;
                        continue 'tick;
                    }
                    let Some(pos) = r.store.lectern_pos else {
                        r.store.cur_state = State::Stopping;
                        continue 'tick;
                    };
                    if seam_block_air(game, pos) {
                        r.store.cur_state = State::Placing;
                        continue 'tick;
                    }
                    if !seam_block_is_lectern(game, pos) {
                        r.store.cur_state = State::Breaking;
                        continue 'tick;
                    }
                    if r.store.tick_cooldown > 0 {
                        if r.store.pre_breaking {
                            pre_break(game, r);
                        }
                        r.store.tick_cooldown -= 1;
                        break;
                    }
                    r.store.cur_state = State::Breaking;
                    continue 'tick;
                }
                r.store.cur_state = State::InteractVil;
                // Falls out: INTERACT_VIL runs this same tick.
            }
            State::InteractVil => {
                let Some(villager) = r.store.updated_villager else {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                };
                if !seam_game_mode(game) {
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                }
                r.store.tick_cooldown = 5;
                r.store.signals = 0;
                r.store.cur_state = State::WaitingTrade;
                // Aim from the eye at the villager (upstream's fakeYaw and
                // fakePitch), then right-click it without opening its menu.
                let eye = seam_eye_position(game);
                if let Some(vp) = seam_mob_position(game, villager) {
                    let d = [vp[0] - eye[0], vp[1] - eye[1], vp[2] - eye[2]];
                    r.store.fake_yaw = ((d[2].atan2(d[0]).to_degrees() - 90.0) % 360.0) as f32;
                    r.store.fake_pitch =
                        (-(d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt())).to_degrees()) as f32;
                    seam_interact_villager(game, villager);
                }
                // Falls out: WAITING_TRADE runs this same tick.
            }
            State::WaitingTrade => {
                if (r.store.signals & TRADE) != 0 {
                    if (r.store.signals & TRADE_OK) == 0 {
                        r.store.cur_state = State::Breaking;
                        continue 'tick;
                    }
                    seam_goal_met_cue(game);
                    let gi = r.store.last_goal_met as usize;
                    let mut parts = vec![part(Color::White, "Goal met: ", None)];
                    let name = seam_enchant_display(game, &r.store.goals[gi].enchant)
                        .unwrap_or_else(|| r.store.goals[gi].enchant.clone());
                    parts.push(part(Color::Gray, name, None));
                    if !r.store.auto_remove {
                        let run = format!("/autolec remove {} {}", gi, r.store.uuid);
                        parts.push(part(Color::Red, " [REMOVE]", Some(run)));
                    } else {
                        let last = r.store.goals.len() - 1;
                        r.store.goals.swap(gi, last);
                        r.store.goals.remove(last);
                        r.store.uuid += 1;
                        parts.push(part(Color::Red, " [AUTO REMOVED]", None));
                    }
                    say_msg(game, &msg(parts));
                    say_msg(game, &msg(vec![part(Color::Green, "Completed.", None)]));
                    r.store.cur_state = State::Stopping;
                    continue 'tick;
                }
                seam_move_self(game, r.store.forced_pos);
                if r.store.tick_cooldown > 0 {
                    if r.store.pre_breaking {
                        pre_break(game, r);
                    }
                    r.store.tick_cooldown -= 1;
                    break;
                }
                r.store.cur_state = State::InteractVil;
                // Falls out: INTERACT_VIL runs this same tick.
            }
        }
    }
}

/// `toolNearBreak`: a tool within 2 damage of its max is left alone.
#[cfg(not(feature = "standalone"))]
fn tool_near_break(tool: &ToolFacts) -> bool {
    tool.damageable && tool.damage + 2 >= tool.max_damage
}

/// `equipWorkingTool`: the hotbar's fastest tool on a lectern that is not
/// near breaking, selected into hand.
#[cfg(not(feature = "standalone"))]
fn equip_working_tool(game: &mut crate::game::Game<'_>) -> bool {
    let mut best: Option<(i32, f32)> = None;
    for slot in 0..9 {
        let Some(tool) = seam_hotbar_tool(game, slot) else {
            continue;
        };
        if tool_near_break(&tool) {
            continue;
        }
        if best.is_some_and(|(_, speed)| speed >= tool.mining_speed_lectern) {
            continue;
        }
        best = Some((slot, tool.mining_speed_lectern));
    }
    match best {
        Some((slot, _)) => {
            seam_select_hotbar(game, slot);
            true
        }
        None => false,
    }
}

/// `checkPreserveTool`: keep a healthy tool in hand; none left means the
/// farm stops (returns true, the caller re-enters the switch).
#[cfg(not(feature = "standalone"))]
fn check_preserve_tool(game: &mut crate::game::Game<'_>, r: &mut Runner) -> bool {
    match seam_main_hand_tool(game) {
        Some(tool) if tool.is_tool && !tool_near_break(&tool) => false,
        _ => {
            if equip_working_tool(game) {
                false
            } else {
                r.store.cur_state = State::Stopping;
                true
            }
        }
    }
}

/// `getLookingAt`: pick along the lectern's saved angles, blocks only.
#[cfg(not(feature = "standalone"))]
fn get_looking_at(game: &mut crate::game::Game<'_>, r: &Runner) -> Option<[i32; 3]> {
    seam_pick_from(game, r.store.lec_pitch, r.store.lec_yaw)
}

/// `equipLectern`: a lectern in either hand wins; otherwise the first
/// lectern in the hotbar is selected (remembering what was held).
#[cfg(not(feature = "standalone"))]
fn equip_lectern(game: &mut crate::game::Game<'_>, r: &mut Runner) -> Option<Hand> {
    if seam_offhand_is_lectern(game) {
        return Some(Hand::Off);
    }
    if seam_mainhand_is_lectern(game) {
        return Some(Hand::Main);
    }
    let slot = seam_first_hotbar_lectern(game)?;
    r.store.prev_selected_slot = seam_selected_hotbar(game);
    seam_select_hotbar(game, slot);
    Some(Hand::Main)
}

/// `preBreak`: the break effects and dig continue at the lectern while the
/// farm waits, with the same tool care as BREAKING.
#[cfg(not(feature = "standalone"))]
fn pre_break(game: &mut crate::game::Game<'_>, r: &mut Runner) {
    if r.store.prev_selected_slot != -1 {
        seam_select_hotbar(game, r.store.prev_selected_slot);
        r.store.prev_selected_slot = -1;
    }
    if r.store.preserve_tool {
        check_preserve_tool(game, r);
    } else if seam_main_hand_empty(game) {
        equip_working_tool(game);
    }
    if let Some(pos) = r.store.lectern_pos {
        break_dig(game, pos);
    }
}

/// The BREAKING dig: the block's break effects, continue-or-start the
/// destroy, and a swing.
#[cfg(not(feature = "standalone"))]
fn break_dig(game: &mut crate::game::Game<'_>, pos: [i32; 3]) {
    seam_add_breaking_effects(game, pos);
    if seam_is_destroying(game) {
        seam_continue_destroy(game, pos);
    } else {
        seam_start_destroy(game, pos);
    }
    seam_swing_main(game);
}

/// The movement hook reads this to zero the walk inputs (DummyInput).
#[cfg(not(feature = "standalone"))]
pub static LOCK: AtomicBool = AtomicBool::new(false);

/// Called by the host's movement dispatch with the walk inputs.
#[cfg(not(feature = "standalone"))]
pub fn movement(forward: &mut f32, strafe: &mut f32, jump: &mut bool, sneak: &mut bool) {
    if LOCK.load(Ordering::Relaxed) {
        *forward = 0.0;
        *strafe = 0.0;
        *jump = false;
        *sneak = false;
    }
}

/// The offers hook: the merchant menu's offers, upstream's
/// determineTrades. Waiting for a trade is the whole gate; upstream lets
/// the offer check run whatever auto-trade is set to, and the setting
/// decides the lock-in sends only.
#[cfg(not(feature = "standalone"))]
pub fn on_offers(offers: &[Offer]) {
    let mut r = r_store_lock();
    if r.store.cur_state != State::WaitingTrade {
        return;
    }
    // stage: the main inventory's emerald / book / paper counts
    // (`plr.getInventory().getNonEquipmentItems()`).
    let counts = Counts::default();
    // stage: `plr.getInventory().getFreeSlot()`, -1 when the inventory is
    // full.
    let free_slot = -1;
    let (tar_idx, actions, msgs) =
        determine_trades(&mut r.store, &GameRegistry, offers, &counts, free_slot);
    if tar_idx != -1 {
        r.met = offers.get(tar_idx as usize).cloned();
    }
    r.pending_msgs.extend(msgs);
    r.pending_actions.extend(actions);
}

/// The profession hook: a villager took the librarian spot; WAITING_PROF
/// reads the signal.
#[cfg(not(feature = "standalone"))]
pub fn on_profession(villager: u32) {
    let mut r = r_store_lock();
    r.store.updated_villager = Some(villager);
    r.store.signal(PROF);
}

/// The item hook: the dropped lectern item came back to the inventory.
#[cfg(not(feature = "standalone"))]
pub fn on_item_returned() {
    let mut r = r_store_lock();
    r.store.signal(ITEM);
}

/// One chat message from the mod, its colors mapped to the host's text
/// formatting (the stage pass finishes the color wiring).
#[cfg(not(feature = "standalone"))]
fn say_msg(game: &mut crate::game::Game<'_>, m: &Msg) {
    let line: String = m.parts.iter().map(|p| p.text.as_str()).collect();
    game.system_message(horizon_client::text::Text::literal(line));
}

// --- The host seams ------------------------------------------------------
// Thin calls kept in one place so a staging mismatch is fixed here only.

/// stage: the crosshair target being a lectern block, plus the face it was
/// hit on (`mc.hitResult` as BlockHitResult, `getDirection`).
#[cfg(not(feature = "standalone"))]
fn lectern_under_crosshair(game: &crate::game::Game<'_>) -> Option<([i32; 3], u8)> {
    let hit = game.pick()?;
    let pos = hit.pos;
    (game.block_name(pos).as_ref() == "minecraft:lectern").then_some((pos, 0))
}

/// stage: the player's main inventory is the open menu
/// (`plr.containerMenu instanceof InventoryMenu`).
#[cfg(not(feature = "standalone"))]
fn seam_in_main_inventory(game: &crate::game::Game<'_>) -> bool {
    game.in_main_inventory()
}

/// stage: `mc.level != null`.
#[cfg(not(feature = "standalone"))]
fn seam_in_world(game: &crate::game::Game<'_>) -> bool {
    game.in_world()
}

/// stage: `mc.gameMode != null`.
#[cfg(not(feature = "standalone"))]
fn seam_game_mode(game: &crate::game::Game<'_>) -> bool {
    game.game_mode_ready()
}

/// stage: `world.getBlockState(pos).canBeReplaced()`.
#[cfg(not(feature = "standalone"))]
fn seam_block_replaceable(game: &crate::game::Game<'_>, pos: [i32; 3]) -> bool {
    game.block_name(pos).is_none()
}

/// stage: `world.getBlockState(pos).is(Blocks.LECTERN)`.
#[cfg(not(feature = "standalone"))]
fn seam_block_is_lectern(game: &crate::game::Game<'_>, pos: [i32; 3]) -> bool {
    game.block_name(pos).as_ref() == "minecraft:lectern"
}

/// stage: `world.getBlockState(pos).isAir()`.
#[cfg(not(feature = "standalone"))]
fn seam_block_air(game: &crate::game::Game<'_>, pos: [i32; 3]) -> bool {
    game.block_name(pos).is_none()
}

/// stage: `plr.getXRot()`.
#[cfg(not(feature = "standalone"))]
fn seam_pitch(game: &crate::game::Game<'_>) -> f32 {
    game.pitch()
}

/// stage: `plr.getYRot()`.
#[cfg(not(feature = "standalone"))]
fn seam_yaw(game: &crate::game::Game<'_>) -> f32 {
    game.yaw()
}

/// stage: `plr.position()`.
#[cfg(not(feature = "standalone"))]
fn seam_position(game: &crate::game::Game<'_>) -> [f64; 3] {
    game.position()
}

/// stage: `plr.getEyePosition()`.
#[cfg(not(feature = "standalone"))]
fn seam_eye_position(game: &crate::game::Game<'_>) -> [f64; 3] {
    game.position()
}

/// stage: the villager's position (`updatedVillager.position()`).
#[cfg(not(feature = "standalone"))]
fn seam_mob_position(game: &crate::game::Game<'_>, id: u32) -> Option<[f64; 3]> {
    game.mob_position(id as i32)
}

/// stage: `plr.move(SELF, forcedPos - pos - (0, 0.00001, 0))`, the farm
/// drifting back to where it started a hair downward.
#[cfg(not(feature = "standalone"))]
fn seam_move_self(game: &mut crate::game::Game<'_>, forced: Option<[f64; 3]>) {
    if let Some(f) = forced {
        game.hold_at(f);
    }
    LOCK.store(true, Ordering::Relaxed);
}

/// stage: `plr.getInventory().setSelectedSlot(slot)`.
#[cfg(not(feature = "standalone"))]
fn seam_select_hotbar(game: &mut crate::game::Game<'_>, slot: i32) {
    game.select_hotbar(slot)
}

/// stage: `plr.getInventory().getSelectedSlot()`.
#[cfg(not(feature = "standalone"))]
fn seam_selected_hotbar(game: &crate::game::Game<'_>) -> i32 {
    game.selected_hotbar()
}

/// stage: `plr.getItemInHand(MAIN_HAND)` as TOOL facts.
#[cfg(not(feature = "standalone"))]
fn seam_main_hand_tool(game: &crate::game::Game<'_>) -> Option<ToolFacts> {
    game.main_hand_tool()
}

/// stage: `inventory.getItem(slot)` as TOOL facts, hotbar slots 0..9.
#[cfg(not(feature = "standalone"))]
fn seam_hotbar_tool(game: &crate::game::Game<'_>, slot: i32) -> Option<ToolFacts> {
    game.hotbar_tool(slot)
}

/// stage: `plr.getMainHandItem().isEmpty()`.
#[cfg(not(feature = "standalone"))]
fn seam_main_hand_empty(game: &crate::game::Game<'_>) -> bool {
    game.main_hand_empty()
}

/// stage: `plr.getOffhandItem().is(Items.LECTERN)`.
#[cfg(not(feature = "standalone"))]
fn seam_offhand_is_lectern(game: &crate::game::Game<'_>) -> bool {
    false
}

/// stage: `plr.getMainHandItem().is(Items.LECTERN)`.
#[cfg(not(feature = "standalone"))]
fn seam_mainhand_is_lectern(game: &crate::game::Game<'_>) -> bool {
    false
}

/// stage: the first lectern block among the hotbar's non-equipment items.
#[cfg(not(feature = "standalone"))]
fn seam_first_hotbar_lectern(game: &crate::game::Game<'_>) -> Option<i32> {
    None
}

/// stage: `world.addBreakingBlockEffects(pos, side, false)`.
#[cfg(not(feature = "standalone"))]
fn seam_add_breaking_effects(game: &mut crate::game::Game<'_>, pos: [i32; 3]) {
    let _ = (game, pos);
}

/// stage: `interactionManager.isDestroying()`.
#[cfg(not(feature = "standalone"))]
fn seam_is_destroying(game: &crate::game::Game<'_>) -> bool {
    game.is_destroying()
}

/// stage: `interactionManager.continueDestroyBlock(pos, side)`.
#[cfg(not(feature = "standalone"))]
fn seam_continue_destroy(game: &mut crate::game::Game<'_>, pos: [i32; 3]) {
    game.attack_block(pos)
}

/// stage: `interactionManager.startDestroyBlock(pos, side)`.
#[cfg(not(feature = "standalone"))]
fn seam_start_destroy(game: &mut crate::game::Game<'_>, pos: [i32; 3]) {
    game.attack_block(pos)
}

/// stage: `plr.swing(MAIN_HAND, ...)` (`getAttackAnimation` / the place
/// animation from the use result).
#[cfg(not(feature = "standalone"))]
fn seam_swing_main(game: &mut crate::game::Game<'_>) {
    game.swing_hand()
}

/// stage: `interactionManager.useItemOn(plr, hand, hitResult)`, swinging on
/// a predicted success.
#[cfg(not(feature = "standalone"))]
fn seam_use_item_on(game: &mut crate::game::Game<'_>, _hit: &[i32; 3], _hand: Hand) {
    game.use_with_hand();
}

/// stage: `plr.pick(4.5, 1.0, false)` along the given angles, block hits
/// only.
#[cfg(not(feature = "standalone"))]
fn seam_pick_from(game: &mut crate::game::Game<'_>, _pitch: f32, _yaw: f32) -> Option<[i32; 3]> {
    let hit = game.pick()?;
    Some(hit.pos)
}

/// stage: the ABORT_DESTROY_BLOCK packet plus
/// `world.destroyBlockProgress(plr, pos, -1)`, sent at the lectern face.
#[cfg(not(feature = "standalone"))]
fn seam_abort_destroy(game: &mut crate::game::Game<'_>, pos: [i32; 3]) {
    game.abort_block(pos)
}

/// stage: `interactionManager.interact(plr, villager, hit-or-fallback,
/// MAIN_HAND)` with a swing on a predicted success.
#[cfg(not(feature = "standalone"))]
fn seam_interact_villager(game: &mut crate::game::Game<'_>, villager: u32) {
    let _ = game.interact_mob(Some(crate::game::InteractTarget {
        id: villager as i32,
        location: [0.0, 0.0, 0.0],
    }));
}

/// stage: the goal-met cue, `SimpleSoundInstance.forUI(EXPERIENCE_ORB_PICKUP,
/// 1)` plus `SDL_FlashWindow(handle, SDL_FLASH_UNTIL_FOCUSED)`.
#[cfg(not(feature = "standalone"))]
fn seam_goal_met_cue(game: &mut crate::game::Game<'_>) {
    let _ = game;
}

/// stage: the enchantment's chat description for the Goal met line.
#[cfg(not(feature = "standalone"))]
fn seam_enchant_display(game: &crate::game::Game<'_>, id: &str) -> Option<String> {
    let _ = (game, id);
    None
}

/// The `/autolec` command entry, called from the command glue.
#[cfg(not(feature = "standalone"))]
pub fn command_entry(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    let Some(stripped) = command.strip_prefix(HEADS[0]) else {
        return;
    };
    if !stripped.is_empty() && !stripped.starts_with(char::is_whitespace) {
        return;
    }
    *ran = true;
    let mut r = r_store_lock();
    for m in run_command(&mut r.store, &GameRegistry, stripped.trim()) {
        say_msg(game, &m);
    }
}

use std::sync::Mutex;
#[cfg(not(feature = "standalone"))]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRegistry;

    impl Registry for FakeRegistry {
        fn resolve(&self, id: &str) -> Option<EnchantSpec> {
            match id {
                "minecraft:mending" => Some(EnchantSpec {
                    max_level: 1,
                    double_price: false,
                }),
                "minecraft:sharpness" => Some(EnchantSpec {
                    max_level: 5,
                    double_price: false,
                }),
                "minecraft:protection" => Some(EnchantSpec {
                    max_level: 4,
                    double_price: true,
                }),
                _ => None,
            }
        }
        fn tradeable(&self) -> Vec<String> {
            vec!["minecraft:mending".into(), "minecraft:sharpness".into()]
        }
        fn display(&self, id: &str) -> Option<String> {
            self.resolve(id).map(|_| format!("{id} display"))
        }
    }

    fn payload(m: &Msg) -> &str {
        &m.parts[1].text
    }

    #[test]
    fn level_quirks() {
        // -1 min with a set max accepts level 1 or below, whatever the max.
        assert!(is_goal_level_met(5, -1, 0, 1));
        assert!(!is_goal_level_met(5, -1, 0, 2));
        // Both -1: any level.
        assert!(is_goal_level_met(5, -1, -1, 9));
        // -1 max with a set min: at least the enchant's own max level.
        assert!(is_goal_level_met(5, 4, -1, 5));
        assert!(!is_goal_level_met(5, 4, -1, 4));
        // A plain range.
        assert!(is_goal_level_met(5, 2, 3, 2));
        assert!(!is_goal_level_met(5, 2, 3, 4));
    }

    #[test]
    fn price_helpers() {
        let plain = EnchantSpec {
            max_level: 5,
            double_price: false,
        };
        let double = EnchantSpec {
            max_level: 5,
            double_price: true,
        };
        assert_eq!(cheapest_enchant(&plain), 17);
        // 71 caps at 64, doubled or not.
        assert_eq!(most_expensive_enchant(&plain), 64);
        assert_eq!(cheapest_enchant(&double), 34);
        assert_eq!(most_expensive_enchant(&double), 64);
        let high = EnchantSpec {
            max_level: 11,
            double_price: true,
        };
        assert_eq!(cheapest_enchant(&high), 64);
    }

    #[test]
    fn price_met() {
        let spec = EnchantSpec {
            max_level: 5,
            double_price: false,
        };
        // -1 min, set max: at or under the cheapest.
        assert!(is_goal_price_met(&spec, -1, 0, 17));
        assert!(!is_goal_price_met(&spec, -1, 0, 18));
        // Set min, -1 max: at or over the most expensive.
        assert!(is_goal_price_met(&spec, 0, -1, 64));
        assert!(!is_goal_price_met(&spec, 0, -1, 63));
        // Both -1: any price. A plain range.
        assert!(is_goal_price_met(&spec, -1, -1, 0));
        assert!(is_goal_price_met(&spec, 10, 20, 15));
        assert!(!is_goal_price_met(&spec, 10, 20, 25));
    }

    #[test]
    fn goal_index_skips_unresolved() {
        let goals = vec![
            Goal {
                enchant: "minecraft:mending".into(),
                lvl_min: -1,
                lvl_max: -1,
                price_min: -1,
                price_max: -1,
            },
            Goal {
                enchant: "minecraft:missing".into(),
                lvl_min: -1,
                lvl_max: -1,
                price_min: -1,
                price_max: -1,
            },
            Goal {
                enchant: "minecraft:sharpness".into(),
                lvl_min: 1,
                lvl_max: 5,
                price_min: -1,
                price_max: -1,
            },
        ];
        let reg = FakeRegistry;
        // The sharpness goal is index 1, not 2: the unresolved one is
        // skipped without counting.
        assert_eq!(
            goal_met(&goals, &reg, "minecraft:sharpness", 3, 10),
            Some(1)
        );
        assert_eq!(goal_met(&goals, &reg, "minecraft:mending", 1, 10), Some(0));
        assert_eq!(goal_met(&goals, &reg, "minecraft:sharpness", 6, 10), None);
    }

    #[test]
    fn config_roundtrip() {
        let mut store = Store::default();
        store.item_sync = true;
        store.auto_trade = AutoTrade::Cheapest;
        store.goals.push(Goal {
            enchant: "minecraft:mending".into(),
            lvl_min: -1,
            lvl_max: 0,
            price_min: -1,
            price_max: 64,
        });
        let text = save_config(&store);
        assert_eq!(
            text,
            "itemSync=true\nbreakCooldown=false\nlogTrade=false\npreBreak=true\npreserveTool=true\nautoRemove=false\nautoTrade=CHEAPEST\ngoals=minecraft:mending,-1,0,-1,64;"
        );
        let mut loaded = Store::default();
        load_config(&mut loaded, &text);
        assert_eq!(loaded.goals, store.goals);
        assert_eq!(loaded.auto_trade, AutoTrade::Cheapest);
        assert!(loaded.item_sync);
        // Bad lines are skipped; a bad goal record drops its whole line.
        load_config(
            &mut loaded,
            "nonsense\nbreakCooldown=true\ngoals=minecraft:x,1,2,3;minecraft:y,1,2,3,4\n",
        );
        assert!(loaded.break_cooldown);
        // The bad line throws on its first record, so neither lands; the
        // roundtrip goal is all that is left.
        assert_eq!(loaded.goals.len(), 1);
        assert_eq!(loaded.goals[0].enchant, "minecraft:mending");
    }

    #[test]
    fn start_stop_guards() {
        let mut store = Store::default();
        let reg = FakeRegistry;
        assert!(run_command(&mut store, &reg, "start").is_empty());
        assert_eq!(store.cur_state, State::Starting);
        let again = run_command(&mut store, &reg, "start");
        assert_eq!(payload(&again[0]), "Please stop before starting again.");
        assert!(run_command(&mut store, &reg, "stop").is_empty());
        assert_eq!(store.cur_state, State::Stopping);
        store.cur_state = State::Stopped;
        let stopped = run_command(&mut store, &reg, "stop");
        assert_eq!(payload(&stopped[0]), "Already stopped.");
    }

    #[test]
    fn toggles() {
        let mut store = Store::default();
        let reg = FakeRegistry;
        assert_eq!(
            payload(&run_command(&mut store, &reg, "sync item")[0]),
            "Item Sync is now ON"
        );
        assert!(store.item_sync);
        assert_eq!(
            payload(&run_command(&mut store, &reg, "sync item")[0]),
            "Item Sync is now OFF"
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "breakCooldown")[0]),
            "Break cooldown is now ON"
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "log")[0]),
            "Logging is now ON"
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "preBreak")[0]),
            "Pre breaking is now OFF"
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "preserveTool")[0]),
            "Tool preserving is now OFF"
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "autoRemove")[0]),
            "Auto remove is now ON"
        );
    }

    #[test]
    fn auto_trade_modes() {
        let mut store = Store::default();
        let reg = FakeRegistry;
        assert_eq!(
            payload(&run_command(&mut store, &reg, "autoTrade ENCHANT")[0]),
            "Auto Trading Mode is now set to ENCHANT."
        );
        assert_eq!(store.auto_trade, AutoTrade::Enchant);
        assert_eq!(
            payload(&run_command(&mut store, &reg, "autoTrade CHEAPEST")[0]),
            "Auto Trading Mode is now set to CHEAPEST."
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "autoTrade OFF")[0]),
            "Auto Trading Mode is now set to OFF."
        );
        assert_eq!(store.auto_trade, AutoTrade::Off);
        // Lowercase fails: brigadier's literals are exact.
        assert_eq!(
            payload(&run_command(&mut store, &reg, "autoTrade enchant")[0]),
            USAGE
        );
        assert_eq!(
            payload(&run_command(&mut store, &reg, "autoTrade")[0]),
            USAGE
        );
    }

    #[test]
    fn add_shapes() {
        let mut store = Store::default();
        let reg = FakeRegistry;
        // With an enchantment: min level, min price.
        assert!(run_command(&mut store, &reg, "add minecraft:sharpness min min").is_empty());
        assert_eq!(store.goals[0].lvl_min, -1);
        assert_eq!(store.goals[0].lvl_max, 0);
        assert_eq!(store.goals[0].price_min, -1);
        assert_eq!(store.goals[0].price_max, 0);
        // Max level, any price.
        run_command(&mut store, &reg, "add minecraft:sharpness max any");
        assert_eq!(store.goals[1].lvl_min, 0);
        assert_eq!(store.goals[1].lvl_max, -1);
        assert_eq!(store.goals[1].price_min, -1);
        assert_eq!(store.goals[1].price_max, -1);
        // Two numbers on both halves.
        run_command(&mut store, &reg, "add minecraft:mending 2 4 10 30");
        assert_eq!(store.goals[2].lvl_min, 2);
        assert_eq!(store.goals[2].lvl_max, 4);
        assert_eq!(store.goals[2].price_min, 10);
        assert_eq!(store.goals[2].price_max, 30);
        // Without an enchantment: one goal per tradeable enchant.
        run_command(&mut store, &reg, "add any min");
        assert_eq!(store.goals.len(), 5);
        assert_eq!(store.goals[3].enchant, "minecraft:mending");
        assert_eq!(store.goals[4].enchant, "minecraft:sharpness");
        // The uuid ticked once per add, and nothing was said.
        assert_eq!(store.uuid, 4);
        // Bare add and a bad shape fail with the usage line.
        assert_eq!(payload(&run_command(&mut store, &reg, "add")[0]), USAGE);
        assert_eq!(
            payload(&run_command(&mut store, &reg, "add minecraft:sharpness min nope")[0]),
            USAGE
        );
    }

    #[test]
    fn clear_and_remove() {
        let mut store = Store::default();
        let reg = FakeRegistry;
        run_command(&mut store, &reg, "add minecraft:sharpness any any");
        run_command(&mut store, &reg, "add minecraft:mending any any");
        run_command(&mut store, &reg, "add minecraft:protection any any");
        assert_eq!(store.uuid, 3);
        // An out-of-date uuid refuses.
        let expired = run_command(&mut store, &reg, "remove 0 2");
        assert_eq!(
            payload(&expired[0]),
            "Command Expired.\nType \"/autolec list\" then choose again."
        );
        // No uuid at all is accepted; the last goal swaps into the slot.
        let listed = run_command(&mut store, &reg, "remove 0");
        assert_eq!(store.goals.len(), 2);
        assert_eq!(store.goals[0].enchant, "minecraft:protection");
        assert_eq!(store.uuid, 4);
        // The re-list carries the fresh uuid in the click-run.
        assert_eq!(
            listed[1].parts[3].run.as_deref(),
            Some("/autolec remove 0 4")
        );
        // Out of bounds says so.
        let oob = run_command(&mut store, &reg, "remove 9");
        assert_eq!(payload(&oob[0]), "Index out of bounds.");
        // A negative index fails the argument bounds.
        assert_eq!(
            payload(&run_command(&mut store, &reg, "remove -1")[0]),
            USAGE
        );
        // Clear greens and ticks the uuid.
        let cleared = run_command(&mut store, &reg, "clear");
        assert_eq!(payload(&cleared[0]), "All goals removed.");
        assert_eq!(cleared[0].parts[1].color, Color::Green);
        assert_eq!(store.uuid, 5);
    }

    #[test]
    fn list_rows() {
        let mut store = Store::default();
        store.uuid = 7;
        store.goals.push(Goal {
            enchant: "minecraft:mending".into(),
            lvl_min: -1,
            lvl_max: -1,
            price_min: -1,
            price_max: -1,
        });
        store.goals.push(Goal {
            enchant: "minecraft:missing".into(),
            lvl_min: 1,
            lvl_max: 2,
            price_min: 3,
            price_max: 4,
        });
        store.goals.push(Goal {
            enchant: "minecraft:sharpness".into(),
            lvl_min: 2,
            lvl_max: 4,
            price_min: 5,
            price_max: 10,
        });
        let rows = run_command(&mut store, &FakeRegistry, "list");
        assert_eq!(payload(&rows[0]), "Goals:");
        // Two rows: the unresolved goal is skipped and does not advance the
        // index, so sharpness lands on [1].
        assert_eq!(
            rows[1].parts[2].text,
            "minecraft:mending display any level, any price."
        );
        assert_eq!(rows[1].parts[3].run.as_deref(), Some("/autolec remove 0 7"));
        assert_eq!(
            rows[2].parts[2].text,
            "minecraft:sharpness display level from 2 to 4, from 5 to 10 emeralds."
        );
        assert_eq!(rows[2].parts[3].run.as_deref(), Some("/autolec remove 1 7"));
    }

    #[test]
    fn info_strings() {
        assert_eq!(enchant_lvl_info(-1, -1), " any level,");
        assert_eq!(enchant_lvl_info(-1, 0), " lowest level,");
        assert_eq!(enchant_lvl_info(2, -1), " max level,");
        assert_eq!(enchant_lvl_info(2, 4), " level from 2 to 4,");
        assert_eq!(price_info(-1, -1), " any price.");
        assert_eq!(price_info(-1, 0), " cheapest.");
        assert_eq!(price_info(0, -1), " most expensive.");
        assert_eq!(price_info(5, 10), " from 5 to 10 emeralds.");
    }

    #[test]
    fn roman_numerals() {
        assert_eq!(roman_numeral(1), "I");
        assert_eq!(roman_numeral(5), "V");
        assert_eq!(roman_numeral(10), "X");
        assert_eq!(roman_numeral(11), "11");
        assert_eq!(roman_numeral(-1), "-1");
    }

    #[test]
    fn enchant_fullnames() {
        let reg = FakeRegistry;
        // Level 1 on a max-1 enchant carries no numeral.
        assert_eq!(
            enchant_fullname(&reg, "minecraft:mending", 1),
            "minecraft:mending display"
        );
        // A level above 1 or a max above 1 adds the numeral.
        assert_eq!(
            enchant_fullname(&reg, "minecraft:sharpness", 3),
            "minecraft:sharpness display III"
        );
        assert_eq!(
            enchant_fullname(&reg, "minecraft:mending", 2),
            "minecraft:mending display II"
        );
    }

    fn offer(cost_a: CostItem, cost_a_count: i32) -> Offer {
        Offer {
            enchant: "minecraft:mending".into(),
            level: 1,
            base_cost: cost_a_count,
            cost: cost_a_count,
            cost_a: (cost_a, cost_a_count),
            cost_b: None,
            result_book: true,
            can_store: true,
            result_enchants: vec![("minecraft:mending".into(), 1)],
        }
    }

    #[test]
    fn cheapest_trade_weights_and_gaps() {
        let counts = Counts {
            emerald: 10,
            book: 10,
            paper: 32,
        };
        // Emerald 10 costs 10; paper 32 costs 0.5 and wins.
        let offers = vec![offer(CostItem::Emerald, 10), offer(CostItem::Paper, 32)];
        assert_eq!(find_cheapest_trade(&offers, &counts), 1);
        // A first cost the inventory cannot cover is skipped, so the
        // affordable offer wins even when it is heavier.
        let thin_paper = Counts {
            emerald: 10,
            book: 10,
            paper: 10,
        };
        let offers = vec![offer(CostItem::Paper, 32), offer(CostItem::Emerald, 10)];
        assert_eq!(find_cheapest_trade(&offers, &thin_paper), 1);
        let poor = Counts::default();
        assert_eq!(find_cheapest_trade(&offers, &poor), -1);
        // A first cost that is neither emerald, paper nor book is skipped.
        let offers = vec![offer(CostItem::Other, 1), offer(CostItem::Book, 5)];
        assert_eq!(find_cheapest_trade(&offers, &counts), 1);
        // Ties keep the earlier index (upstream's strict `<`).
        let offers = vec![offer(CostItem::Emerald, 4), offer(CostItem::Book, 5)];
        // 4 emeralds cost 4.0; 5 books cost 0.73, so books win.
        assert_eq!(find_cheapest_trade(&offers, &counts), 1);
        let offers = vec![offer(CostItem::Paper, 64), offer(CostItem::Paper, 64)];
        let paper_rich = Counts {
            emerald: 10,
            book: 10,
            paper: 64,
        };
        assert_eq!(find_cheapest_trade(&offers, &paper_rich), 0);
    }

    #[test]
    fn trade_log_line() {
        let reg = FakeRegistry;
        let mut store = Store::default();
        store.log_trade = true;
        store.attempts = 3;
        let offers = vec![offer(CostItem::Emerald, 7)];
        let (idx, msgs) = check_trades(&mut store, &reg, &offers);
        assert_eq!(idx, -1);
        assert_eq!(msgs.len(), 1);
        let m = &msgs[0];
        assert_eq!(m.parts.len(), 5);
        assert_eq!(m.parts[0].color, Color::Yellow);
        assert_eq!(m.parts[0].text, "[Auto Lectern] ");
        assert_eq!(m.parts[1].color, Color::Green);
        assert_eq!(m.parts[1].text, "7");
        assert_eq!(m.parts[2].text, " emeralds for ");
        assert_eq!(m.parts[3].text, "minecraft:mending display");
        assert_eq!(m.parts[4].text, " [3]");
        // (msg() contributes the prefix, the log parts are the other four.)
        assert_eq!(m.parts[2].color, Color::White);
        // Log off: no lines.
        store.log_trade = false;
        let (_, msgs) = check_trades(&mut store, &reg, &offers);
        assert!(msgs.is_empty());
    }

    #[test]
    fn trade_checks_goals() {
        let reg = FakeRegistry;
        let mut store = Store::default();
        store.goals.push(Goal {
            enchant: "minecraft:mending".into(),
            lvl_min: 1,
            lvl_max: -1,
            price_min: 0,
            price_max: 100,
        });
        let offers = vec![offer(CostItem::Emerald, 7)];
        let (idx, _) = check_trades(&mut store, &reg, &offers);
        assert_eq!(idx, 0);
        assert_eq!(store.last_goal_met, 0);
        // A second offer matching a later goal keeps its own index.
        store.goals.clear();
        store.last_goal_met = 0;
        assert_eq!(check_trades(&mut store, &reg, &offers).0, -1);
    }

    fn met_trade_store(auto_trade: AutoTrade) -> Store {
        let mut store = Store::default();
        store.auto_trade = auto_trade;
        store.goals.push(Goal {
            enchant: "minecraft:mending".into(),
            lvl_min: 1,
            lvl_max: -1,
            price_min: 0,
            price_max: 100,
        });
        store
    }

    #[test]
    fn determine_trades_signals() {
        let reg = FakeRegistry;
        let counts = Counts {
            emerald: 10,
            book: 10,
            paper: 10,
        };
        let offers = vec![offer(CostItem::Emerald, 7)];

        let mut store = met_trade_store(AutoTrade::Off);
        let (tar, actions, _) = determine_trades(&mut store, &reg, &offers, &counts, 0);
        assert_eq!(tar, 0);
        assert!(actions.is_empty());
        assert_eq!(store.signals, TRADE | TRADE_OK);
        assert_eq!(store.attempts, 1);

        // No goal met: TRADE alone, nothing to send.
        let mut store = Store::default();
        let (tar, actions, _) = determine_trades(&mut store, &reg, &offers, &counts, 0);
        assert_eq!(tar, -1);
        assert!(actions.is_empty());
        assert_eq!(store.signals, TRADE);
        assert_eq!(store.attempts, 1);
    }

    #[test]
    fn determine_trades_lock_in() {
        let reg = FakeRegistry;
        let counts = Counts {
            emerald: 10,
            book: 10,
            paper: 10,
        };
        let mut with_book = offer(CostItem::Emerald, 7);
        with_book.cost_b = Some((CostItem::Book, 1));
        let offers = vec![with_book];

        // Enchant mode with the costs covered: select, pick the result
        // off slot 2, drop it into the free slot.
        let mut store = met_trade_store(AutoTrade::Enchant);
        let (tar, actions, _) = determine_trades(&mut store, &reg, &offers, &counts, 12);
        assert_eq!(tar, 0);
        assert_eq!(
            actions,
            vec![
                TradeAction::SelectTrade(0),
                TradeAction::Click {
                    slot: 2,
                    carried_empty: false
                },
                TradeAction::Click {
                    slot: 12,
                    carried_empty: true
                },
            ]
        );

        // The ENCHANT branch needs emerald + book on the offer itself.
        let mut no_book = offer(CostItem::Emerald, 7);
        no_book.cost_b = None;
        let offers_nb = vec![no_book];
        let mut store = met_trade_store(AutoTrade::Enchant);
        let (tar, actions, _) = determine_trades(&mut store, &reg, &offers_nb, &counts, 12);
        assert_eq!(tar, 0);
        assert!(actions.is_empty());

        // A full inventory sends nothing.
        let mut store = met_trade_store(AutoTrade::Cheapest);
        let (tar, actions, _) = determine_trades(&mut store, &reg, &offers, &counts, -1);
        assert_eq!(tar, 0);
        assert!(actions.is_empty());

        // Cheapest mode buys the cheapest affordable offer, not the met one.
        let mut store = met_trade_store(AutoTrade::Cheapest);
        let two = vec![offer(CostItem::Emerald, 7), offer(CostItem::Book, 5)];
        let (tar, actions, _) = determine_trades(&mut store, &reg, &two, &counts, 3);
        assert_eq!(tar, 0);
        assert_eq!(
            actions,
            vec![
                TradeAction::SelectTrade(1),
                TradeAction::Click {
                    slot: 2,
                    carried_empty: false
                },
                TradeAction::Click {
                    slot: 3,
                    carried_empty: true
                },
            ]
        );
    }

    #[test]
    fn dig_gates() {
        // Zeroing the delay: breaking, or pre-breaking toward a villager
        // state, with the cooldown flag off.
        assert!(zero_destroy_delay(false, true, State::Breaking));
        assert!(zero_destroy_delay(false, true, State::WaitingProf));
        assert!(zero_destroy_delay(false, true, State::WaitingTrade));
        assert!(!zero_destroy_delay(true, true, State::Breaking));
        assert!(!zero_destroy_delay(false, false, State::WaitingProf));
        assert!(!zero_destroy_delay(false, true, State::Stopped));

        // Holding the dig: only when it would finish this tick and the
        // state is pre-breaking toward a villager.
        assert_eq!(anti_block_breaking(0.2, 0.5, true, State::WaitingTrade), 0.2);
        assert_eq!(anti_block_breaking(0.2, 0.9, true, State::WaitingTrade), 0.0);
        assert_eq!(anti_block_breaking(0.2, 0.9, true, State::Breaking), 0.2);
        assert_eq!(anti_block_breaking(0.2, 0.9, true, State::Stopped), 0.2);
        assert_eq!(anti_block_breaking(0.2, 0.9, false, State::WaitingTrade), 0.2);
    }

    #[test]
    fn prof_pick_gates() {
        // In WAITING_PROF, an unpicked librarian next to the lectern wins.
        assert!(picks_prof_villager(
            State::WaitingProf,
            None,
            [10, 64, 10],
            [10.5, 64.5, 10.5],
            true,
            false
        ));
        // Distance gate: squared distance over 8 misses, 4 in.
        assert!(!picks_prof_villager(
            State::WaitingProf,
            None,
            [10, 64, 10],
            [13.5, 64.5, 10.5],
            true,
            false
        ));
        assert!(picks_prof_villager(
            State::WaitingProf,
            None,
            [10, 64, 10],
            [12.5, 64.5, 10.0],
            true,
            false
        ));
        // Wrong state, already picked, was already a librarian, not a
        // librarian now: all miss.
        assert!(!picks_prof_villager(
            State::WaitingItem,
            None,
            [10, 64, 10],
            [10.5, 64.5, 10.5],
            true,
            false
        ));
        assert!(!picks_prof_villager(
            State::WaitingProf,
            Some(9),
            [10, 64, 10],
            [10.5, 64.5, 10.5],
            true,
            false
        ));
        assert!(!picks_prof_villager(
            State::WaitingProf,
            None,
            [10, 64, 10],
            [10.5, 64.5, 10.5],
            true,
            true
        ));
        assert!(!picks_prof_villager(
            State::WaitingProf,
            None,
            [10, 64, 10],
            [10.5, 64.5, 10.5],
            false,
            false
        ));
    }

    #[test]
    fn mixin_gates() {
        // The keepalive and the stop-destroy skip run while the farm runs.
        assert!(keeps_client_active(State::Breaking));
        assert!(!keeps_client_active(State::Stopped));
        assert!(saves_on_stop(true));
        assert!(!saves_on_stop(false));
        assert!(skips_stop_destroy(State::Breaking));
        assert!(!skips_stop_destroy(State::Stopped));
        // The rotation override hands out the fake angles while running.
        assert_eq!(
            rotation_override(State::Breaking, 12.0, -3.5),
            Some((12.0, -3.5))
        );
        assert_eq!(rotation_override(State::Stopped, 12.0, -3.5), None);
        // The merchant menu is captured in WAITING_TRADE only, and its
        // close is swallowed while the capture holds.
        assert!(captures_merchant(State::WaitingTrade, true));
        assert!(!captures_merchant(State::WaitingTrade, false));
        assert!(!captures_merchant(State::Breaking, true));
        assert!(swallows_merchant_close(true, 7, 7));
        assert!(!swallows_merchant_close(true, 7, 8));
        assert!(!swallows_merchant_close(false, 7, 7));
    }

    #[test]
    fn unknown_and_empty_fail() {
        let mut store = Store::default();
        let reg = FakeRegistry;
        assert_eq!(
            payload(&run_command(&mut store, &reg, "nonsense")[0]),
            USAGE
        );
        assert_eq!(payload(&run_command(&mut store, &reg, "")[0]), USAGE);
        assert_eq!(payload(&run_command(&mut store, &reg, "sync")[0]), USAGE);
    }
}
