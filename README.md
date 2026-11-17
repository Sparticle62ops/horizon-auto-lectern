# auto-lectern (Horizon port)

A Rust port of [Auto Lectern](https://github.com/capthehacker99/AutoLectern)
(capthehacker99, CC-BY-NC-SA-4.0), which cycles librarian trades by breaking
and replacing lecterns until an offered book matches your goal.

## Scope (1:1)

The port lives in mixins/auto_lectern/runtime.rs, upstream's `sys.exe.al`
package translated verbatim, with the game-touchpoint half compiled out under
`--features standalone` so the logic tests run anywhere:

- Goals are upstream's ALGoal records: an enchantment id plus level and price
  windows with -1 as the extreme-bound sentinel. The matcher keeps upstream's
  quirks, including a -1 level minimum with a set maximum accepting only
  level 1 or below, and a level maximum of -1 meaning "at the enchantment's
  own max level".
- The villager price helpers are upstream's: cheapest is maxLevel*3+2 and
  dearest is maxLevel*3+2+(4+maxLevel*10), both doubled for the
  double-trade-price tag and then capped at 64.
- The config file is autolec.txt in upstream's exact format (load/save).
- `/autolec` is upstream's AutoLec command tree verbatim: start, stop, sync
  item, breakCooldown, autoTrade ENCHANT|CHEAPEST|OFF, log, preBreak,
  preserveTool, autoRemove, add with enchantment+level+price subcommands,
  clear, remove index or uuid with the Command Expired check, list, with
  upstream's chat lines, the [REMOVE] click-run on list rows and the level
  and price info renderers.
- The tick loop is upstream's MinecraftTickHead state cycle 1:1: stopped,
  stopping, starting, breaking, waiting-item, placing, waiting-prof,
  interact-vil, waiting-trade, with upstream's 40-tick break cooldown and
  5-tick interact cooldown, tool preservation and equipping, the goal-met
  fanfare, and the autoRemove lectern removal.
- The packet/trade layer is upstream's mixin behavior 1:1: findCheapestTrade
  weights (paper cost x0.015625, book x0.146875), checkTrades log lines and
  goal scan, attemptLockTrade's select-trade / click-slot / close packet
  plan, determineTrades, the dig-delay zeroing and anti-block-breaking hold,
  the rotation override the client sends instead of its real angles, the
  keepalive while waiting for the villager, merchant screen capture and the
  swallowed close, and the profession pick from the villager's tracked data.

23 standalone tests green.

## License

Upstream Auto Lectern: CC-BY-NC-SA-4.0 (c) capthehacker99. This port carries
the same license; see LICENSE.
