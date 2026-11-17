//! Port of `interfaces/ExtraVillagerData.java` (the villager mixin interface).

use crate::seams::VillagerProfessionHolder;

/// Upstream `interface ExtraVillagerData`, injected on `Villager` by the mixin:
/// `autolec$getPrevProfession` / `autolec$setPrevProfession`.
pub trait ExtraVillagerData {
    fn autolec_get_prev_profession(&self) -> Option<VillagerProfessionHolder>;
    fn autolec_set_prev_profession(&mut self, prev_profession: VillagerProfessionHolder);
}
