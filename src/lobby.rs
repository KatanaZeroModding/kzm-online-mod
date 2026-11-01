//! Lobby metadata both clients must share before the first room.

use crate::mode::PlayMode;
use crate::wire::PROTO;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LobbyInfo {
    pub mode: PlayMode,
    pub proto: u32,
    pub mods: String,
    pub data: String,
}

impl LobbyInfo {
    pub fn new(mode: PlayMode, mods: [u8; 32], data: [u8; 32]) -> Self {
        Self {
            mode,
            proto: PROTO,
            mods: hex_encode(&mods),
            data: hex_encode(&data),
        }
    }

    #[cfg(test)]
    pub fn matches(&self, other: &LobbyInfo) -> bool {
        self.mode == other.mode && self.proto == other.proto && self.mods == other.mods && self.data == other.data
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

#[cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code))]
pub fn parse_info(mode: &str, proto: &str, mods: &str, data: &str) -> Option<LobbyInfo> {
    Some(LobbyInfo {
        mode: PlayMode::parse(mode)?,
        proto: proto.parse().ok()?,
        mods: mods.to_string(),
        data: data.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_rejects_a_different_mod_set() {
        let host = LobbyInfo::new(PlayMode::Coop, [1; 32], [2; 32]);
        let mut guest = host.clone();
        assert!(host.matches(&guest));
        guest.mods = "00".into();
        assert!(!host.matches(&guest));
    }
}
