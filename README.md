# auto-lectern (Horizon port)

An exact 1:1 Rust port of [Auto Lectern](https://github.com/capthehacker99/AutoLectern)
(capthehacker99, CC-BY-NC-SA-4.0) for [Horizon](https://github.com/horizon-rust/horizon),
which cycles librarian trades by breaking and replacing lecterns until an
offered book matches your goal. The upstream license and credit carry with
this port; see LICENSE.

## Scope (1:1, upstream sys.exe.al)

- src/state.rs, src/goal.rs, src/auto_lectern.rs: ALState, ALGoal,
  ALAutoTrade, DummyInput and the AutoLectern main class 1:1 (state cycle,
  goal matching with upstream's -1 sentinel quirks, the vanilla price
  helpers, autolec.txt config load/save).
- src/commands_seam.rs: the commands package 1:1 (AutoLec's register tree,
  ClientCommandManager with the CommandSyntaxException context-line
  rendering, FakeCommandSource) plus the Brigadier surface it drives
  (StringReader, literal/required argument builders, IntegerArgumentType,
  RegistryEntryReferenceArgumentType with registry fall-through,
  CommandDispatcher) as a same-shape seam.
- src/mixins.rs: the mixin package 1:1 (ClientCommonNetworkHandler send
  redirect with the rotation spoof, GameRenderer render head,
  MinecraftClient tick/stop, ClientPlayerInteractionManager breaking hooks,
  ClientPlayNetworkHandler: merchant screen open/close, command tree
  update, entity-tracker librarian switch, item pickup animation, and the
  full trade pipeline: findCheapestTrade, checkTrades, attemptLockTrade,
  determineTrades, plus sendChatCommand/runClickEventCommand intercept).
- Game-facing calls land behind the same seams upstream uses, injected so
  the logic stays exact; src/seams.rs and src/dummy_input.rs carry the
  seam surface and its test fakes.

39 tests green.

The hnmod/ directory holds the earlier packaged bundle (phase 1) kept for
the .hnmod packaging flow; the src/ crate is the source of truth.

## License

Upstream Auto Lectern: CC-BY-NC-SA-4.0 (c) capthehacker99. This port carries
the same license; see LICENSE.
