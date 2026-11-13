//! VG-693 free-walk L2 / L3 / L6 lighting stills.
//!
//! `--shot-lighting-freewalk <dir>` seeds the small-world ring and teleports to
//! real terrain (camp forest, mine mouth, biome cells) — no demo-pad stamps —
//! then writes:
//! vg693-l2-forest-canopy, vg693-l3-cave-exterior, vg693-l3-cave-under-roof,
//! vg693-l6-plains, vg693-l6-forest, vg693-l6-hills, vg693-l6-marsh.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightingFreewalkScene {
    ForestCanopy,
    CaveMouthExterior,
    CaveUnderRoof,
    PlainsBiome,
    ForestBiome,
    HillsBiome,
    MarshBiome,
}

impl LightingFreewalkScene {
    pub const ALL: [Self; 7] = [
        Self::ForestCanopy,
        Self::CaveMouthExterior,
        Self::CaveUnderRoof,
        Self::PlainsBiome,
        Self::ForestBiome,
        Self::HillsBiome,
        Self::MarshBiome,
    ];

    pub fn filename(self) -> &'static str {
        match self {
            Self::ForestCanopy => "vg693-l2-forest-canopy.png",
            Self::CaveMouthExterior => "vg693-l3-cave-exterior.png",
            Self::CaveUnderRoof => "vg693-l3-cave-under-roof.png",
            Self::PlainsBiome => "vg693-l6-plains.png",
            Self::ForestBiome => "vg693-l6-forest.png",
            Self::HillsBiome => "vg693-l6-hills.png",
            Self::MarshBiome => "vg693-l6-marsh.png",
        }
    }

    pub fn chat(self) -> &'static str {
        match self {
            Self::ForestCanopy => "L2 freewalk forest canopy",
            Self::CaveMouthExterior => "L3 freewalk cave exterior",
            Self::CaveUnderRoof => "L3 freewalk cave under roof",
            Self::PlainsBiome => "L6 freewalk plains",
            Self::ForestBiome => "L6 freewalk forest",
            Self::HillsBiome => "L6 freewalk hills",
            Self::MarshBiome => "L6 freewalk marsh",
        }
    }

    /// Noon so Soft wrap + biome fog read clearly (avoid End-adjacent stands).
    pub fn time_of_day(self) -> f32 {
        0.50
    }
}
