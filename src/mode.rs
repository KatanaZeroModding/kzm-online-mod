//! Co-op and PVP are the same simulation with different rules.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode {
    Coop,
    Pvp,
}

impl PlayMode {
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(PlayMode::Coop),
            2 => Some(PlayMode::Pvp),
            _ => None,
        }
    }

    pub fn as_byte(self) -> u8 {
        match self {
            PlayMode::Coop => 1,
            PlayMode::Pvp => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PlayMode::Coop => "coop",
            PlayMode::Pvp => "pvp",
        }
    }

    #[cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code))]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "coop" => Some(PlayMode::Coop),
            "pvp" => Some(PlayMode::Pvp),
            _ => None,
        }
    }

    /// Co-op hits between the two players are ignored. PVP counts them.
    pub fn players_damage_each_other(self) -> bool {
        matches!(self, PlayMode::Pvp)
    }

    /// Co-op waits until both clients are in the next room. PVP does not advance the story.
    pub fn wait_for_story(self) -> bool {
        matches!(self, PlayMode::Coop)
    }

    pub fn end_round_on_death(self) -> bool {
        matches!(self, PlayMode::Pvp)
    }
}
