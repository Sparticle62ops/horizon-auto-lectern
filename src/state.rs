//! Port of `ALState.java` and `ALAutoTrade.java` (enums).

/// Upstream `ALState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ALState {
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

/// Upstream `ALAutoTrade`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ALAutoTrade {
    Off,
    Enchant,
    Cheapest,
}
