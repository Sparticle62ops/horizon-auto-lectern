//! 1:1 port of the `commands` package: `AutoLec`, `ClientCommandManager`,
//! `FakeCommandSource` (upstream capthehacker99/AutoLectern @301008d,
//! CC-BY-NC-SA-4.0; the license and credit carry with this port), plus the
//! Brigadier surface it drives (`CommandDispatcher`,
//! `LiteralArgumentBuilder`/`RequiredArgumentBuilder`, `IntegerArgumentType`,
//! `RegistryEntryReferenceArgumentType`, `StringReader`,
//! `CommandSyntaxException`) as a same-shape seam: the builder tree, parse
//! walk, argument coercion and exception shape mirror upstream so the command
//! code stays exact.
//!
//! Seam notes: the command closures take the `AutoLectern` instance as a
//! parameter where upstream reads its `AutoLectern.getInstance()` static, and
//! `FakeCommandSource` holds seam clones of the client surfaces whose
//! messages sync back at the `ClientCommandManager.executeCommand` boundary
//! (upstream holds live references).

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::auto_lectern::AutoLectern;
use crate::goal::{Identifier, ALGoal};
use crate::seams::{ChatFormatting, ClickEvent, Component, LevelSeam, MinecraftSeam, PlayerSeam};
use crate::state::{ALAutoTrade, ALState};

// ----- Brigadier seam -----

/// Upstream `com.mojang.brigadier.exceptions.CommandSyntaxException`.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSyntaxException {
    pub message: String,
    /// Upstream `getInput()` (absent when unset).
    pub input: Option<String>,
    /// Upstream `getCursor()` (unset is cursor -1 upstream).
    pub cursor: Option<usize>,
}

/// Upstream `com.mojang.brigadier.StringReader`.
#[derive(Clone, Debug)]
pub struct StringReader {
    pub string: String,
    pub cursor: usize,
}

impl StringReader {
    pub fn new(string: impl Into<String>) -> StringReader {
        StringReader { string: string.into(), cursor: 0 }
    }

    fn is_end(&self) -> bool {
        self.cursor >= self.string.len()
    }

    /// Brigadier `StringReader.canRead()`.
    pub fn can_read(&self) -> bool {
        !self.is_end()
    }

    /// Brigadier `StringReader.readUnquotedString()`.
    pub fn read_unquoted_string(&mut self) -> String {
        self.read_word()
    }

    /// Brigadier `StringReader.setCursor(int)`.
    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }

    fn peek(&self) -> Option<char> {
        self.string[self.cursor..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.cursor += 1;
            } else {
                break;
            }
        }
    }

    /// Brigadier's plain-word read: up to the next whitespace.
    fn read_word(&mut self) -> String {
        self.skip_whitespace();
        let start = self.cursor;
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                break;
            }
            self.cursor += 1;
        }
        self.string[start..self.cursor].to_string()
    }
}

/// Upstream `IntegerArgumentType.integer(min, max)` and
/// `RegistryEntryReferenceArgumentType.registryEntry(registryAccess,
/// RegistryKeys.ENCHANTMENT)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgumentType {
    Integer { min: i32, max: i32 },
    Enchantment,
}

impl ArgumentType {
    pub fn integer(min: i32, max: i32) -> ArgumentType {
        ArgumentType::Integer { min, max }
    }
    pub fn registry_entry_reference() -> ArgumentType {
        ArgumentType::Enchantment
    }
}

/// Upstream `RegistryEntry.Reference<Enchantment>`: the key's value is the
/// enchantment identifier; `registryKey().isOf(RegistryKeys.ENCHANTMENT)`
/// always holds for keys this argument produces (seam).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnchantHolder {
    pub key: Identifier,
}

/// Upstream `context.getArgument(name, ...)` values.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgumentValue {
    Integer(i32),
    Enchant(EnchantHolder),
}

type Command = Box<dyn Fn(&mut CommandContext, &mut AutoLectern) -> i32>;

/// A built command node (`CommandNode` upstream). `argument_type` is `None`
/// for a literal, `Some` for a typed argument.
pub struct CommandNode {
    pub name: String,
    pub argument_type: Option<ArgumentType>,
    pub children: Vec<CommandNode>,
    pub command: Option<Command>,
}

/// Upstream `LiteralArgumentBuilder` / `RequiredArgumentBuilder`, unified.
pub struct ArgumentBuilder {
    pub name: String,
    pub argument_type: Option<ArgumentType>,
    pub children: Vec<ArgumentBuilder>,
    pub command: Option<Command>,
}

impl ArgumentBuilder {
    /// Upstream `LiteralArgumentBuilder.literal(name)`.
    pub fn literal(name: impl Into<String>) -> ArgumentBuilder {
        ArgumentBuilder {
            name: name.into(),
            argument_type: None,
            children: Vec::new(),
            command: None,
        }
    }

    /// Upstream `RequiredArgumentBuilder.argument(name, type)`.
    pub fn argument(name: impl Into<String>, argument_type: ArgumentType) -> ArgumentBuilder {
        ArgumentBuilder {
            name: name.into(),
            argument_type: Some(argument_type),
            children: Vec::new(),
            command: None,
        }
    }

    /// Upstream `builder.then(child)`.
    pub fn then(mut self, child: ArgumentBuilder) -> ArgumentBuilder {
        self.children.push(child);
        self
    }

    /// Upstream `builder.executes(command)`.
    pub fn executes<F>(mut self, command: F) -> ArgumentBuilder
    where
        F: Fn(&mut CommandContext, &mut AutoLectern) -> i32 + 'static,
    {
        self.command = Some(Box::new(command));
        self
    }

    fn build(self) -> CommandNode {
        CommandNode {
            name: self.name,
            argument_type: self.argument_type,
            children: self.children.into_iter().map(|c| c.build()).collect(),
            command: self.command,
        }
    }
}

/// Upstream `com.mojang.brigadier.context.CommandContext`.
pub struct CommandContext<'a> {
    source: &'a mut FakeCommandSource,
    args: BTreeMap<String, ArgumentValue>,
}

impl<'a> CommandContext<'a> {
    /// Upstream `context.getSource()`.
    pub fn get_source(&mut self) -> &mut FakeCommandSource {
        self.source
    }

    /// Upstream `IntegerArgumentType.getInteger(context, name)`.
    pub fn get_integer(&self, name: &str) -> i32 {
        match self.args.get(name) {
            Some(ArgumentValue::Integer(v)) => *v,
            other => panic!("getInteger({name}): not an integer argument: {other:?}"),
        }
    }

    /// Upstream `context.getArgument(name, RegistryEntry.Reference.class)`.
    pub fn get_enchantment(&self, name: &str) -> Option<EnchantHolder> {
        match self.args.get(name) {
            Some(ArgumentValue::Enchant(h)) => Some(h.clone()),
            other => panic!("getArgument({name}): not an enchantment argument: {other:?}"),
        }
    }
}

/// Upstream `com.mojang.brigadier.CommandDispatcher<ClientCommandSource>`.
pub struct CommandDispatcher {
    root: Option<CommandNode>,
}

impl CommandDispatcher {
    pub fn new() -> CommandDispatcher {
        CommandDispatcher { root: None }
    }

    /// Upstream `dispatcher.register(builder)`: the builder hangs under the
    /// dispatcher's own empty root.
    pub fn register(&mut self, builder: ArgumentBuilder) {
        let built = builder.build();
        match self.root.take() {
            Some(mut existing) => {
                existing.children.push(built);
                self.root = Some(existing);
            }
            None => {
                self.root = Some(CommandNode {
                    name: String::new(),
                    argument_type: None,
                    children: vec![built],
                    command: None,
                });
            }
        }
    }

    /// Upstream `dispatcher.execute(reader, source)`. The `AutoLectern`
    /// instance stands in for upstream's `AutoLectern.getInstance()` static.
    pub fn execute(
        &mut self,
        mut reader: StringReader,
        source: &mut FakeCommandSource,
        al: &mut AutoLectern,
    ) -> Result<i32, CommandSyntaxException> {
        let mut node: &CommandNode = self.root.as_ref().expect("no commands registered");
        let mut args: BTreeMap<String, ArgumentValue> = BTreeMap::new();
        let full_input = reader.string.clone();
        let mut at_root = true;
        loop {
            reader.skip_whitespace();
            if reader.is_end() {
                return match &node.command {
                    Some(command) => {
                        let mut ctx = CommandContext { source, args };
                        Ok((command)(&mut ctx, al))
                    }
                    None => Err(CommandSyntaxException {
                        message: if at_root {
                            "Unknown command".to_string()
                        } else {
                            format!("Incorrect argument for command: {}", children_hint(node))
                        },
                        input: Some(full_input.clone()),
                        cursor: Some(reader.cursor),
                    }),
                };
            }
            let arg_start = reader.cursor;
            let word = reader.read_word();

            // Children in registration order; a failed argument parse falls
            // through to the next sibling (upstream Brigadier behaviour, and
            // how `add` falls from the enchantment argument to the sibling
            // literals when the element is not in the registry).
            let mut next: Option<&CommandNode> = None;
            let mut parse_error: Option<CommandSyntaxException> = None;
            for child in &node.children {
                match child.argument_type {
                    None => {
                        if child.name == word {
                            next = Some(child);
                            break;
                        }
                    }
                    Some(ArgumentType::Integer { min, max }) => match word.parse::<i32>().ok() {
                        Some(v) if v >= min && v <= max => {
                            args.insert(child.name.clone(), ArgumentValue::Integer(v));
                            next = Some(child);
                            break;
                        }
                        Some(v) => {
                            parse_error = Some(CommandSyntaxException {
                                message: format!(
                                    "Integer must be between {} and {}, found {}",
                                    min, max, v
                                ),
                                input: Some(full_input.clone()),
                                cursor: Some(arg_start),
                            });
                        }
                        None => {
                            parse_error = Some(CommandSyntaxException {
                                message: "Expected integer".to_string(),
                                input: Some(full_input.clone()),
                                cursor: Some(arg_start),
                            });
                        }
                    },
                    Some(ArgumentType::Enchantment) => {
                        // `Identifier.read(reader)` then the registry lookup
                        // through the source's world registry.
                        match Identifier::parse(&word) {
                            Ok(id) => {
                                let present = source
                                    .mc
                                    .level
                                    .as_ref()
                                    .map(|w| w.enchantments.entries.contains_key(&id.0))
                                    .unwrap_or(false);
                                if present {
                                    args.insert(
                                        child.name.clone(),
                                        ArgumentValue::Enchant(EnchantHolder { key: id }),
                                    );
                                    next = Some(child);
                                    break;
                                } else {
                                    parse_error = Some(CommandSyntaxException {
                                        message: format!(
                                            "There is no element with value '{}'",
                                            id.0
                                        ),
                                        input: Some(full_input.clone()),
                                        cursor: Some(arg_start),
                                    });
                                }
                            }
                            Err(_) => {
                                parse_error = Some(CommandSyntaxException {
                                    message: "Incorrect argument for command".to_string(),
                                    input: Some(full_input.clone()),
                                    cursor: Some(arg_start),
                                });
                            }
                        }
                    }
                }
            }

            match next {
                Some(child) => {
                    at_root = false;
                    node = child;
                }
                None => {
                    if let Some(_command) = &node.command {
                        // A runnable node with trailing input: Brigadier
                        // errors rather than running the command.
                        return Err(CommandSyntaxException {
                            message: format!(
                                "Incorrect argument for command: {}",
                                children_hint(node)
                            ),
                            input: Some(full_input.clone()),
                            cursor: Some(arg_start),
                        });
                    }
                    return Err(parse_error.unwrap_or(CommandSyntaxException {
                        message: if at_root {
                            "Unknown command".to_string()
                        } else {
                            format!(
                                "Incorrect argument for command: {}",
                                children_hint(node)
                            )
                        },
                        input: Some(full_input.clone()),
                        cursor: Some(arg_start),
                    }));
                }
            }
        }
    }
}

fn children_hint(node: &CommandNode) -> String {
    node.children
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

// ----- ClientCommandManager -----

/// Upstream `private static final Set<String> clientSideCommands`.
static CLIENT_SIDE_COMMANDS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Upstream `ClientCommandManager.clearClientSideCommands()`.
pub fn clear_client_side_commands() {
    CLIENT_SIDE_COMMANDS.lock().unwrap().clear();
}

/// Upstream `ClientCommandManager.addClientSideCommand(name)`.
pub fn add_client_side_command(name: &str) {
    CLIENT_SIDE_COMMANDS.lock().unwrap().push(name.to_string());
}

/// Upstream `ClientCommandManager.isClientSideCommand(name)`.
pub fn is_client_side_command(name: &str) -> bool {
    CLIENT_SIDE_COMMANDS.lock().unwrap().iter().any(|n| n == name)
}

/// Upstream `FakeCommandSource extends ClientCommandSource`
/// (`super(mc.getNetworkHandler(), mc, perm -> true)`).
#[derive(Clone, Debug)]
pub struct FakeCommandSource {
    pub mc: MinecraftSeam,
    pub player: PlayerSeam,
}

impl FakeCommandSource {
    pub fn new(mc: MinecraftSeam, player: PlayerSeam) -> FakeCommandSource {
        FakeCommandSource { mc, player }
    }

    /// Upstream `sendMessage(message)`: `player.displayClientMessage(message, false)`.
    pub fn send_message(&mut self, message: Component) {
        self.player.display_client_message(message, false);
    }

    /// Upstream `getCompletions(context)`: `Suggestions.empty()`.
    pub fn get_completions(&self) {}

    /// Upstream `getPlayerNames()`: network-handler player-list names, empty
    /// when the handler is absent.
    pub fn get_player_names(&self) -> Vec<String> {
        self.mc.player_list_names.clone()
    }

    /// Seam: pushes this source's chat and player messages back onto the live
    /// surfaces at the `executeCommand` boundary.
    pub fn sync_back(&mut self, mc: &mut MinecraftSeam) {
        mc.chat.messages.append(&mut self.mc.chat.messages);
        mc.chat.scroll_resets += self.mc.chat.scroll_resets;
        self.mc.chat.scroll_resets = 0;
        if let Some(player) = &mut mc.player {
            player.client_messages.append(&mut self.player.client_messages);
        }
    }
}

/// Upstream `ClientCommandManager.sendFeedback(message)`: the chat HUD.
pub fn send_feedback(source: &mut FakeCommandSource, message: Component) {
    source.mc.chat.add_client_system_message(message);
}

/// Upstream `ClientCommandManager.sendError(error)`:
/// `sendFeedback(error.copy().formatted(Formatting.RED))`.
pub fn send_error(source: &mut FakeCommandSource, error: Component) {
    send_feedback(source, error.with_style(ChatFormatting::Blue));
}

/// Upstream `AutoLectern.LOGGER.error("An error occurred: {}", msg, e)`.
fn log_error(message: &str) {
    #[cfg(not(test))]
    eprintln!("An error occurred: {}", message);
    #[cfg(test)]
    let _ = message;
}

/// Upstream `ClientCommandManager.executeCommand(mc, reader, command)`.
/// `al` stands in for upstream `AutoLectern.getInstance()`.
pub fn execute_command(
    mc: &mut MinecraftSeam,
    dispatcher: &mut CommandDispatcher,
    reader: StringReader,
    command: &str,
    al: &mut AutoLectern,
) {
    if mc.player.is_none() {
        return;
    }
    // upstream: `new FakeCommandSource(mc, player)` against the live objects;
    // the seam clones and syncs back below.
    let mut source = FakeCommandSource::new(mc.clone(), mc.player.clone().unwrap());
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatcher.execute(reader, &mut source, al)
    }));
    match outcome {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => {
            // upstream `catch (final CommandSyntaxException e)` branch
            send_error(&mut source, Component::literal(e.message.clone()));
            if let (Some(input), Some(raw_cursor)) = (&e.input, e.cursor) {
                let cursor = raw_cursor.min(input.len());
                let mut text = Component::literal("")
                    .with_style(ChatFormatting::Gray)
                    .with_click_event(ClickEvent::SuggestCommand(command.to_string()));
                if cursor > 10 {
                    text = text.append(Component::literal("..."));
                }
                text = text
                    .append(Component::literal(&input[cursor.saturating_sub(10)..cursor]));
                if cursor < input.len() {
                    text = text.append(
                        Component::literal(&input[cursor..])
                            .with_style(ChatFormatting::Blue)
                            .with_underline(),
                    );
                }
                text = text.append(
                    Component::translatable("command.context.here")
                        .with_style(ChatFormatting::Blue)
                        .with_italic(),
                );
                send_feedback(&mut source, text);
            }
        }
        Err(payload) => {
            // upstream `catch (final Exception e)` branch
            let message = payload
                .downcast_ref::<&str>()
                .map(|m| m.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "Box<dyn Any>".to_string());
            let error = Component::literal(message.clone());
            send_error(
                &mut source,
                Component::translatable("command.failed").with_hover_event(error),
            );
            log_error(&message);
        }
    }
    source.sync_back(mc);
}

// ----- AutoLec -----

/// Upstream `net.minecraft.command.CommandRegistryAccess`.
pub struct CommandBuildContext;

pub struct AutoLec;

impl AutoLec {
    /// Upstream `getEnchantment(context)`.
    fn get_enchantment(context: &CommandContext) -> Option<EnchantHolder> {
        // upstream: `registryKey().isOf(RegistryKeys.ENCHANTMENT)`; the only
        // keys this argument produces are enchantment keys (seam).
        context.get_enchantment("enchantment")
    }

    fn literal(literal: &str) -> ArgumentBuilder {
        ArgumentBuilder::literal(literal)
    }

    fn argument(name: &str, argument_type: ArgumentType) -> ArgumentBuilder {
        ArgumentBuilder::argument(name, argument_type)
    }

    /// Upstream `AutoLec.register(dispatcher, registryAccess)`.
    pub fn register(dispatcher: &mut CommandDispatcher, _registry_access: &CommandBuildContext) {
        add_client_side_command("autolec");

        dispatcher.register(
            Self::literal("autolec")
                .then(Self::literal("start").executes(|ctx, al| {
                    if al.get_state() != ALState::Stopped {
                        // upstream: `mc.inGameHud.getChatHud().addMessage(...)`
                        ctx.get_source().mc.chat.add_client_system_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal("Please stop before starting again.")
                                        .with_style(ChatFormatting::Blue),
                                ),
                        );
                        return 0;
                    }
                    al.set_state(ALState::Starting);
                    0
                }))
                .then(Self::literal("stop").executes(|ctx, al| {
                    if al.get_state() == ALState::Stopped {
                        ctx.get_source().send_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal("Already stopped.")
                                        .with_style(ChatFormatting::Blue),
                                ),
                        );
                        return 0;
                    }
                    al.set_state(ALState::Stopping);
                    0
                }))
                .then(Self::literal("sync").then(Self::literal("item").executes(
                    |ctx, al| {
                        al.item_sync = !al.item_sync;
                        ctx.get_source().send_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal(format!(
                                        "Item Sync is now {}",
                                        if al.item_sync { "ON" } else { "OFF" }
                                    ))
                                    .with_style(ChatFormatting::White),
                                ),
                        );
                        0
                    },
                )))
                .then(Self::literal("breakCooldown").executes(|ctx, al| {
                    al.break_cooldown = !al.break_cooldown;
                    ctx.get_source().send_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal(format!(
                                    "Break cooldown is now {}",
                                    if al.break_cooldown { "ON" } else { "OFF" }
                                ))
                                .with_style(ChatFormatting::White),
                            ),
                    );
                    0
                }))
                .then(Self::literal("autoTrade")
                    .then(Self::literal("ENCHANT").executes(|ctx, al| {
                        al.auto_trade = ALAutoTrade::Enchant;
                        ctx.get_source().send_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal("Auto Trading Mode is now set to ENCHANT.")
                                        .with_style(ChatFormatting::White),
                                ),
                        );
                        0
                    }))
                    .then(Self::literal("CHEAPEST").executes(|ctx, al| {
                        al.auto_trade = ALAutoTrade::Cheapest;
                        ctx.get_source().send_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal("Auto Trading Mode is now set to CHEAPEST.")
                                        .with_style(ChatFormatting::White),
                                ),
                        );
                        0
                    }))
                    .then(Self::literal("OFF").executes(|ctx, al| {
                        al.auto_trade = ALAutoTrade::Off;
                        ctx.get_source().send_message(
                            Component::literal("[Auto Lectern] ")
                                .with_style(ChatFormatting::Yellow)
                                .append(
                                    Component::literal("Auto Trading Mode is now set to OFF.")
                                        .with_style(ChatFormatting::White),
                                ),
                        );
                        0
                    })))
                .then(Self::literal("log").executes(|ctx, al| {
                    al.log_trade = !al.log_trade;
                    ctx.get_source().send_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal(format!(
                                    "Logging is now {}",
                                    if al.log_trade { "ON" } else { "OFF" }
                                ))
                                .with_style(ChatFormatting::White),
                            ),
                    );
                    0
                }))
                .then(Self::literal("preBreak").executes(|ctx, al| {
                    al.pre_breaking = !al.pre_breaking;
                    ctx.get_source().send_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal(format!(
                                    "Pre breaking is now {}",
                                    if al.pre_breaking { "ON" } else { "OFF" }
                                ))
                                .with_style(ChatFormatting::White),
                            ),
                    );
                    0
                }))
                .then(Self::literal("preserveTool").executes(|ctx, al| {
                    al.preserve_tool = !al.preserve_tool;
                    ctx.get_source().send_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal(format!(
                                    "Tool preserving is now {}",
                                    if al.preserve_tool { "ON" } else { "OFF" }
                                ))
                                .with_style(ChatFormatting::White),
                            ),
                    );
                    0
                }))
                .then(Self::create_add_goal_subcommand(_registry_access))
                .then(Self::literal("clear").executes(|ctx, al| {
                    al.goals.clear();
                    al.increment_uuid();
                    ctx.get_source().send_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal("All goals removed.")
                                    .with_style(ChatFormatting::Green),
                            ),
                    );
                    0
                }))
                .then(Self::literal("remove")
                    .then(Self::argument("index", ArgumentType::integer(0, i32::MAX))
                        .executes(|ctx, al| {
                            let index = ctx.get_integer("index");
                            Self::remove_goal(ctx, al, index, -1)
                        })
                        .then(Self::argument("uuid", ArgumentType::integer(0, i32::MAX))
                            .executes(|ctx, al| {
                                let index = ctx.get_integer("index");
                                let uuid = ctx.get_integer("uuid");
                                Self::remove_goal(ctx, al, index, uuid)
                            }))))
                .then(Self::literal("autoRemove").executes(|ctx, al| {
                    al.auto_remove = !al.auto_remove;
                    ctx.get_source().send_message(
                        Component::literal("[Auto Lectern] ")
                            .with_style(ChatFormatting::Yellow)
                            .append(
                                Component::literal(format!(
                                    "Auto remove is now {}",
                                    if al.auto_remove { "ON" } else { "OFF" }
                                ))
                                .with_style(ChatFormatting::White),
                            ),
                    );
                    0
                }))
                .then(Self::literal("list").executes(|ctx, al| Self::list_goals(ctx, al))),
        );
    }

    /// Upstream `createLvlSubCommand(arg, has_enchantment)`.
    fn create_lvl_sub_command(arg: ArgumentBuilder, has_enchantment: bool) -> ArgumentBuilder {
        arg.then(Self::create_price_sub_command(
            Self::literal("min"),
            has_enchantment,
            -1,
            0,
        ))
        .then(Self::create_price_sub_command(
            Self::literal("max"),
            has_enchantment,
            0,
            -1,
        ))
        .then(Self::create_price_sub_command(
            Self::literal("any"),
            has_enchantment,
            -1,
            -1,
        ))
        .then(Self::argument("minLevel", ArgumentType::integer(0, i32::MAX))
            .then(Self::create_price_sub_command(
                Self::argument("maxLevel", ArgumentType::integer(0, i32::MAX)),
                has_enchantment,
                -2,
                0,
            )))
    }

    /// Upstream `addToGoal(AL, ctx, enchantment, minLvl, maxLvl, minPrice, maxPrice)`.
    fn add_to_goal(
        al: &mut AutoLectern,
        ctx: &mut CommandContext,
        enchantment: Option<EnchantHolder>,
        min_lvl: i32,
        max_lvl: i32,
        min_price: i32,
        max_price: i32,
    ) {
        let (new_min_lvl, new_max_lvl) = if min_lvl == -2 {
            (ctx.get_integer("minLevel"), ctx.get_integer("maxLevel"))
        } else {
            (min_lvl, max_lvl)
        };
        let world: Option<LevelSeam> = ctx.get_source().mc.level.clone();
        assert!(world.is_some(), "world null"); // upstream assert
        let world = world.unwrap();
        match enchantment {
            Some(enchantment) => {
                // upstream assert `enchantment.getKey().isPresent()`
                al.goals.push(ALGoal {
                    enchant: Some(enchantment.key),
                    lvl_min: new_min_lvl,
                    lvl_max: new_max_lvl,
                    price_min: min_price,
                    price_max: max_price,
                });
            }
            None => {
                // upstream: `world.getRegistryManager().getOrThrow(RegistryKeys.ENCHANTMENT)
                //   .iterateEntries(EnchantmentTags.TRADEABLE).forEach(...)`
                for id in &world.enchantments.tradeable {
                    al.goals.push(ALGoal {
                        enchant: Some(Identifier(id.clone())),
                        lvl_min: new_min_lvl,
                        lvl_max: new_max_lvl,
                        price_min: min_price,
                        price_max: max_price,
                    });
                }
            }
        }
        al.increment_uuid();
    }

    /// Upstream `createPriceSubCommand(arg, has_enchantment, minLvl, maxLvl)`.
    fn create_price_sub_command(
        arg: ArgumentBuilder,
        has_enchantment: bool,
        min_lvl: i32,
        max_lvl: i32,
    ) -> ArgumentBuilder {
        arg.then(Self::literal("min").executes(move |ctx, al| {
            let enchantment = if has_enchantment { Self::get_enchantment(ctx) } else { None };
            Self::add_to_goal(al, ctx, enchantment, min_lvl, max_lvl, -1, 0);
            0
        }))
        .then(Self::literal("max").executes(move |ctx, al| {
            let enchantment = if has_enchantment { Self::get_enchantment(ctx) } else { None };
            Self::add_to_goal(al, ctx, enchantment, min_lvl, max_lvl, 0, -1);
            0
        }))
        .then(Self::literal("any").executes(move |ctx, al| {
            let enchantment = if has_enchantment { Self::get_enchantment(ctx) } else { None };
            Self::add_to_goal(al, ctx, enchantment, min_lvl, max_lvl, -1, -1);
            0
        }))
        .then(Self::argument("minPrice", ArgumentType::integer(0, i32::MAX))
            .then(Self::argument("maxPrice", ArgumentType::integer(0, i32::MAX))
                .executes(move |ctx, al| {
                    let enchantment = if has_enchantment { Self::get_enchantment(ctx) } else { None };
                    let min_price = ctx.get_integer("minPrice");
                    let max_price = ctx.get_integer("maxPrice");
                    Self::add_to_goal(al, ctx, enchantment, min_lvl, max_lvl, min_price, max_price);
                    0
                })))
    }

    /// Upstream `createAddGoalSubcommand(registryAccess)`.
    fn create_add_goal_subcommand(_registry_access: &CommandBuildContext) -> ArgumentBuilder {
        let sub_cmd = Self::literal("add");
        let sub_cmd = sub_cmd.then(Self::create_lvl_sub_command(
            Self::argument("enchantment", ArgumentType::registry_entry_reference()),
            true,
        ));
        Self::create_lvl_sub_command(sub_cmd, false)
    }

    /// Upstream `removeGoal(src, index, uuid)`.
    fn remove_goal(ctx: &mut CommandContext, al: &mut AutoLectern, index: i32, uuid: i32) -> i32 {
        if uuid != -1 && uuid != al.get_uuid() {
            ctx.get_source().send_message(
                Component::literal("[Auto Lectern] ")
                    .with_style(ChatFormatting::Yellow)
                    .append(
                        Component::literal(
                            "Command Expired.\nType \"/autolec list\" then choose again.",
                        )
                        .with_style(ChatFormatting::Blue),
                    ),
            );
            ctx.get_source().mc.chat.reset_scroll();
            return 0;
        }
        let max = al.goals.len() as i32 - 1;
        if max >= index {
            al.goals[index as usize] = al.goals[max as usize].clone();
            al.goals.remove(max as usize);
            al.increment_uuid();
            Self::list_goals(ctx, al);
            ctx.get_source().mc.chat.reset_scroll();
            return 0;
        }
        ctx.get_source().send_message(
            Component::literal("[Auto Lectern] ")
                .with_style(ChatFormatting::Yellow)
                .append(
                    Component::literal("Index out of bounds.")
                        .with_style(ChatFormatting::Blue),
                ),
        );
        ctx.get_source().mc.chat.reset_scroll();
        0
    }

    /// Upstream `enchantLvlInfo(min, max)`.
    fn enchant_lvl_info(min: i32, max: i32) -> String {
        if min == -1 {
            if max == -1 {
                return " any level,".to_string();
            }
            return " lowest level,".to_string();
        } else if max == -1 {
            return " max level,".to_string();
        }
        format!(" level from {} to {},", min, max)
    }

    /// Upstream `priceInfo(min, max)`.
    fn price_info(min: i32, max: i32) -> String {
        if min == -1 {
            if max == -1 {
                return " any price.".to_string();
            }
            return " cheapest.".to_string();
        } else if max == -1 {
            return " most expensive.".to_string();
        }
        format!(" from {} to {} emeralds.", min, max)
    }

    /// Upstream `listGoals(source)`.
    fn list_goals(ctx: &mut CommandContext, al: &mut AutoLectern) -> i32 {
        ctx.get_source().send_message(
            Component::literal("[Auto Lectern] ")
                .with_style(ChatFormatting::Yellow)
                .append(Component::literal("Goals:").with_style(ChatFormatting::White)),
        );
        let mut i: i32 = 0;
        let world = ctx.get_source().mc.level.clone();
        assert!(world.is_some(), "world null"); // upstream assert
        let world = world.unwrap();
        for goal in &al.goals {
            // upstream: `AutoLectern.enchantFromIdentifier(world, goal.enchant())`
            let enchant = match &goal.enchant {
                Some(id) => AutoLectern::enchant_from_identifier(&world, id),
                None => None,
            };
            let enchant = match enchant {
                Some(e) => e,
                None => continue, // upstream `if(enchant == null) continue;`
            };
            let uuid = al.get_uuid();
            let index = i;
            ctx.get_source().send_message(
                Component::literal(format!("[{}] ", i))
                    .with_style(ChatFormatting::Yellow)
                    .append(
                        Component::literal(enchant.description.clone())
                            .append(Component::literal(format!(
                                "{}{}",
                                Self::enchant_lvl_info(goal.lvl_min, goal.lvl_max),
                                Self::price_info(goal.price_min, goal.price_max)
                            )))
                            .with_style(ChatFormatting::White)
                            .append(
                                Component::literal(" [REMOVE]")
                                    .with_click_event(ClickEvent::RunCommand(format!(
                                        "/autolec remove {} {}",
                                        index, uuid
                                    )))
                                    .with_style(ChatFormatting::Blue),
                            ),
                    ),
            );
            i += 1;
        }
        0
    }
}
