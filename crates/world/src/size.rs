//! Finite horizontal sizes.
//!
//! Small is the synthetic demo plus biome terrain in the rest of the square.
//! Medium, large, and mega are wider squares of the same kind. The player
//! stops at the edge of whichever square is loaded.

/// How wide a world is, measured in chunks around its center chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldSize {
    /// 31×31 chunks, 496 blocks on a side.
    Small,
    /// 63×63 chunks, 1008 blocks on a side.
    Medium,
    /// 127×127 chunks, 2032 blocks on a side.
    Large,
    /// 255×255 chunks, 4080 blocks on a side.
    Mega,
}

impl WorldSize {
    /// Chunks from the center chunk to the edge, inclusive of the center.
    pub fn chunk_radius(self) -> i32 {
        match self {
            Self::Small => 15,
            Self::Medium => 31,
            Self::Large => 63,
            Self::Mega => 127,
        }
    }

    /// Chunks along one side of the square.
    pub fn side(self) -> i32 {
        self.chunk_radius() * 2 + 1
    }

    /// Blocks along one side. Each chunk is 16 blocks.
    pub fn blocks(self) -> i32 {
        self.side() * 16
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Mega => "mega",
        }
    }

    /// The next size in the settings list. Mega wraps back to small.
    pub fn cycle(self) -> Self {
        match self {
            Self::Small => Self::Medium,
            Self::Medium => Self::Large,
            Self::Large => Self::Mega,
            Self::Mega => Self::Small,
        }
    }

    /// Byte on the world-size frame. `0` is small.
    pub fn code(self) -> u8 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
            Self::Mega => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Small),
            1 => Some(Self::Medium),
            2 => Some(Self::Large),
            3 => Some(Self::Mega),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::WorldSize;

    #[test]
    fn four_sizes_grow_and_small_is_thirty_one_chunks_on_a_side() {
        let sizes = [
            WorldSize::Small,
            WorldSize::Medium,
            WorldSize::Large,
            WorldSize::Mega,
        ];
        assert_eq!(
            sizes.map(WorldSize::label),
            ["small", "medium", "large", "mega"]
        );
        assert_eq!(WorldSize::Small.chunk_radius(), 15);
        assert_eq!(WorldSize::Small.side(), 31);
        assert_eq!(WorldSize::Small.blocks(), 496);
        assert_eq!(WorldSize::Medium.chunk_radius(), 31);
        assert_eq!(WorldSize::Medium.side(), 63);
        assert_eq!(WorldSize::Medium.blocks(), 1008);
        assert_eq!(WorldSize::Large.chunk_radius(), 63);
        assert_eq!(WorldSize::Large.side(), 127);
        assert_eq!(WorldSize::Large.blocks(), 2032);
        assert_eq!(WorldSize::Mega.chunk_radius(), 127);
        assert_eq!(WorldSize::Mega.side(), 255);
        assert_eq!(WorldSize::Mega.blocks(), 4080);
        let mut previous = 0;
        for size in sizes {
            assert!(size.side() > previous, "{}", size.label());
            assert_eq!(size.side(), size.chunk_radius() * 2 + 1);
            assert_eq!(size.blocks(), size.side() * 16);
            previous = size.side();
        }
    }
}
