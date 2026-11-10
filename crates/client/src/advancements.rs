//! Baked milestone toasts — first craft, kill, sleep, elytra glide, map open.
//!
//! Unlocks persist on the world save (`"advancements": [...]`). A short egui
//! card slides in from the top-right, holds, then fades.

use std::collections::HashSet;

/// Total toast lifetime in seconds (slide + hold + fade).
pub const TOAST_LIFE: f32 = 4.0;
/// Seconds spent sliding in from the right.
pub const TOAST_SLIDE: f32 = 0.28;
/// Seconds spent fading out at the end.
pub const TOAST_FADE: f32 = 0.85;

/// One baked milestone the player can unlock once.
#[derive(Clone, Copy, Debug)]
pub struct Milestone {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
}

/// First-time milestones shown as toasts.
pub const MILESTONES: &[Milestone] = &[
    Milestone {
        id: "craft",
        title: "Crafting Time",
        subtitle: "Take your first recipe",
    },
    Milestone {
        id: "kill",
        title: "Monster Hunter",
        subtitle: "Defeat your first foe",
    },
    Milestone {
        id: "sleep",
        title: "Sweet Dreams",
        subtitle: "Sleep through the night",
    },
    Milestone {
        id: "elytra",
        title: "Sky's the Limit",
        subtitle: "Take your first glide",
    },
    Milestone {
        id: "map",
        title: "Cartographer",
        subtitle: "Open a held map",
    },
];

/// One row on the advancements list screen.
#[derive(Clone, Copy, Debug)]
pub struct AdvancementRow {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub unlocked: bool,
}

/// Paint state for the active toast (alpha + horizontal slide).
#[derive(Clone, Debug)]
pub struct ToastView {
    pub title: String,
    pub subtitle: String,
    /// 0..1 opacity.
    pub alpha: f32,
    /// Pixels to the right of the resting spot (slides toward 0).
    pub slide: f32,
}

#[derive(Clone, Debug)]
struct Toast {
    title: String,
    subtitle: String,
    age: f32,
}

/// Unlocked set + one active toast.
#[derive(Debug, Default)]
pub struct Advancements {
    unlocked: HashSet<String>,
    toast: Option<Toast>,
    dirty: bool,
}

impl Advancements {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the unlocked set from a save (does not toast).
    pub fn install(&mut self, ids: &[String]) {
        self.unlocked.clear();
        for id in ids {
            if milestone(id).is_some() {
                self.unlocked.insert(id.clone());
            }
        }
        self.dirty = false;
    }

    /// Sorted ids for the world save.
    pub fn list(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.unlocked.iter().cloned().collect();
        ids.sort();
        ids
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
    }

    pub fn is_unlocked(&self, id: &str) -> bool {
        self.unlocked.contains(id)
    }

    /// Unlock without popping a toast (list-screen shots / install helpers).
    pub fn unlock_silent(&mut self, id: &str) -> bool {
        let Some(mile) = milestone(id) else {
            return false;
        };
        if !self.unlocked.insert(mile.id.to_string()) {
            return false;
        }
        self.dirty = true;
        true
    }

    /// Rows for the advancements list screen (baked order).
    pub fn screen_rows(&self) -> Vec<AdvancementRow> {
        MILESTONES
            .iter()
            .map(|mile| AdvancementRow {
                id: mile.id,
                title: mile.title,
                subtitle: mile.subtitle,
                unlocked: self.unlocked.contains(mile.id),
            })
            .collect()
    }

    /// Unlock a baked milestone. Returns true when a new toast starts.
    pub fn unlock(&mut self, id: &str) -> bool {
        let Some(mile) = milestone(id) else {
            return false;
        };
        if !self.unlocked.insert(mile.id.to_string()) {
            return false;
        }
        self.dirty = true;
        self.toast = Some(Toast {
            title: mile.title.to_string(),
            subtitle: mile.subtitle.to_string(),
            age: 0.0,
        });
        true
    }

    /// Force a toast for shots (marks unlocked + dirty when new).
    pub fn show_for_shot(&mut self, id: &str, age: f32) {
        let Some(mile) = milestone(id) else {
            return;
        };
        if self.unlocked.insert(mile.id.to_string()) {
            self.dirty = true;
        }
        self.toast = Some(Toast {
            title: mile.title.to_string(),
            subtitle: mile.subtitle.to_string(),
            age: age.clamp(0.0, TOAST_LIFE - 0.05),
        });
    }

    pub fn tick(&mut self, dt: f32) {
        let Some(toast) = self.toast.as_mut() else {
            return;
        };
        toast.age += dt.max(0.0);
        if toast.age >= TOAST_LIFE {
            self.toast = None;
        }
    }

    pub fn view(&self) -> Option<ToastView> {
        let toast = self.toast.as_ref()?;
        let age = toast.age;
        let alpha = if age < TOAST_SLIDE {
            (age / TOAST_SLIDE).clamp(0.0, 1.0)
        } else if age > TOAST_LIFE - TOAST_FADE {
            ((TOAST_LIFE - age) / TOAST_FADE).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let slide = if age < TOAST_SLIDE {
            let t = 1.0 - (age / TOAST_SLIDE).clamp(0.0, 1.0);
            // Ease-out cubic.
            let e = t * t * t;
            140.0 * e
        } else {
            0.0
        };
        Some(ToastView {
            title: toast.title.clone(),
            subtitle: toast.subtitle.clone(),
            alpha,
            slide,
        })
    }
}

fn milestone(id: &str) -> Option<&'static Milestone> {
    MILESTONES.iter().find(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_milestone_toasts_once_then_stays_unlocked() {
        let mut adv = Advancements::new();
        assert!(adv.unlock("craft"));
        assert!(adv.view().is_some());
        assert!(adv.is_unlocked("craft"));
        assert!(!adv.unlock("craft"));
        assert!(adv.take_dirty());
        assert!(!adv.take_dirty());
        let list = adv.list();
        assert_eq!(list, vec!["craft".to_string()]);
    }

    #[test]
    fn toast_slides_then_fades() {
        let mut adv = Advancements::new();
        adv.unlock("kill");
        let early = adv.view().unwrap();
        assert!(early.slide > 50.0);
        assert!(early.alpha < 1.0);
        adv.tick(TOAST_SLIDE + 0.5);
        let mid = adv.view().unwrap();
        assert_eq!(mid.slide, 0.0);
        assert!((mid.alpha - 1.0).abs() < 0.01);
        adv.tick(TOAST_LIFE);
        assert!(adv.view().is_none());
    }

    #[test]
    fn install_skips_unknown_ids() {
        let mut adv = Advancements::new();
        adv.install(&[
            "craft".into(),
            "bogus".into(),
            "elytra".into(),
        ]);
        assert!(adv.is_unlocked("craft"));
        assert!(adv.is_unlocked("elytra"));
        assert!(!adv.is_unlocked("bogus"));
        assert!(!adv.take_dirty());
    }
}
