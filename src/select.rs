use std::collections::HashSet;

use crate::schema::{Pack, Preset};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Min,
    Std,
    Max,
}

pub struct Selection {
    ids: Vec<String>,
    index: HashSet<String>,
}

impl Selection {
    pub fn new() -> Selection {
        return Selection {
            ids: Vec::new(),
            index: HashSet::new(),
        };
    }

    #[allow(dead_code)]
    pub fn toggle(&mut self, id: &str) {
        if !self.index.remove(id) {
            self.ids.push(id.to_string());
            self.index.insert(id.to_string());
            return;
        }
        self.ids.retain(|existing| existing != id);
    }

    pub fn contains(&self, id: &str) -> bool {
        return self.index.contains(id);
    }

    pub fn is_empty(&self) -> bool {
        return self.ids.is_empty();
    }

    pub fn ids(&self) -> Vec<String> {
        return self.ids.clone();
    }

    pub fn select_tweak(&mut self, id: &str, on: bool) {
        if on {
            if !self.index.contains(id) {
                self.ids.push(id.to_string());
                self.index.insert(id.to_string());
            }
            return;
        }
        if self.index.remove(id) {
            self.ids.retain(|existing| existing != id);
        }
    }

    pub fn select_category(&mut self, packs: &[Pack], category_id: &str, on: bool) {
        for pack in packs {
            if pack.meta.id != category_id {
                continue;
            }
            for tweak in &pack.tweak {
                self.select_tweak(&tweak.id, on);
            }
        }
    }

    pub fn select_all(&mut self, packs: &[Pack], on: bool) {
        for pack in packs {
            for tweak in &pack.tweak {
                self.select_tweak(&tweak.id, on);
            }
        }
    }

    pub fn apply_preset(&mut self, packs: &[Pack], tier: Tier, replace: bool) {
        for pack in packs {
            if pack.meta.oneway() {
                continue;
            }
            for tweak in &pack.tweak {
                let wanted = match tier {
                    Tier::Min => tweak.preset == Some(Preset::Min),
                    Tier::Std => matches!(tweak.preset, Some(Preset::Min) | Some(Preset::Std)),
                    Tier::Max => matches!(
                        tweak.preset,
                        Some(Preset::Min) | Some(Preset::Std) | Some(Preset::Max)
                    ),
                };
                if replace || wanted {
                    self.select_tweak(&tweak.id, wanted);
                }
            }
        }
    }
}
