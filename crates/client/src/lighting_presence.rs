//! VG-681 Research L1–L6 lighting presence stills.
//!
//! `--shot-lighting-presence <dir>` stamps controlled look-pads on the same
//! small-world seed demo pad (bounce/shadows style) and writes:
//! l1-plains-noon, l2-forest-canopy, l3-cave-mouth-exterior, l3-cave-under-roof,
//! l4-night-open, l4-night-under-tree, l5-trace-off, l5-trace-on,
//! l6-hills, l6-marsh (L6 plains/forest covered by L1/L2 grass meta).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightingPresenceScene {
    PlainsNoon,
    ForestCanopy,
    CaveMouthExterior,
    CaveUnderRoof,
    NightOpen,
    NightUnderTree,
    TraceOff,
    TraceOn,
    HillsBiome,
    MarshBiome,
}

impl LightingPresenceScene {
    pub const ALL: [Self; 10] = [
        Self::PlainsNoon,
        Self::ForestCanopy,
        Self::CaveMouthExterior,
        Self::CaveUnderRoof,
        Self::NightOpen,
        Self::NightUnderTree,
        Self::TraceOff,
        Self::TraceOn,
        Self::HillsBiome,
        Self::MarshBiome,
    ];

    pub fn filename(self) -> &'static str {
        match self {
            Self::PlainsNoon => "l1-plains-noon.png",
            Self::ForestCanopy => "l2-forest-canopy.png",
            Self::CaveMouthExterior => "l3-cave-mouth-exterior.png",
            Self::CaveUnderRoof => "l3-cave-under-roof.png",
            Self::NightOpen => "l4-night-open.png",
            Self::NightUnderTree => "l4-night-under-tree.png",
            Self::TraceOff => "l5-trace-off.png",
            Self::TraceOn => "l5-trace-on.png",
            Self::HillsBiome => "l6-hills.png",
            Self::MarshBiome => "l6-marsh.png",
        }
    }

    pub fn chat(self) -> &'static str {
        match self {
            Self::PlainsNoon => "L1 plains noon",
            Self::ForestCanopy => "L2 forest canopy",
            Self::CaveMouthExterior => "L3 cave mouth exterior",
            Self::CaveUnderRoof => "L3 cave under roof",
            Self::NightOpen => "L4 night open",
            Self::NightUnderTree => "L4 night under tree",
            Self::TraceOff => "L5 trace off",
            Self::TraceOn => "L5 trace on",
            Self::HillsBiome => "L6 hills",
            Self::MarshBiome => "L6 marsh",
        }
    }

    /// Noon ≈ 0.34 (bounce day), midnight = 0.0.
    pub fn time_of_day(self) -> f32 {
        match self {
            Self::NightOpen | Self::NightUnderTree => 0.0,
            _ => 0.34,
        }
    }

    pub fn trace_on(self) -> bool {
        !matches!(self, Self::TraceOff)
    }
}
