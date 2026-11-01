//! One enum is the whole packet. Encode and decode live next to it.

use crate::mode::PlayMode;

pub const PROTO: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub x: i32,
    pub y: i32,
    pub hp: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Hello {
        mode: PlayMode,
        proto: u32,
        mods: [u8; 32],
        data: [u8; 32],
    },
    Input {
        tick: u32,
        buttons: u32,
        aim_x: i32,
        aim_y: i32,
    },
    Room {
        tick: u32,
        seed: u32,
        name: String,
    },
    Checksum {
        tick: u32,
        count: u32,
        sum_x: i64,
        sum_y: i64,
        sum_hp: i64,
    },
    Snapshot {
        tick: u32,
        bodies: Vec<Body>,
    },
}

const HELLO: u8 = 1;
const INPUT: u8 = 2;
const ROOM: u8 = 3;
const CHECKSUM: u8 = 4;
const SNAPSHOT: u8 = 5;

pub fn encode(message: &Message) -> Vec<u8> {
    let mut out = Vec::new();
    match message {
        Message::Hello { mode, proto, mods, data } => {
            out.push(HELLO);
            out.extend_from_slice(&0u32.to_le_bytes());
            out.push(mode.as_byte());
            out.extend_from_slice(&proto.to_le_bytes());
            out.extend_from_slice(mods);
            out.extend_from_slice(data);
        }
        Message::Input { tick, buttons, aim_x, aim_y } => {
            out.push(INPUT);
            out.extend_from_slice(&tick.to_le_bytes());
            out.extend_from_slice(&buttons.to_le_bytes());
            out.extend_from_slice(&aim_x.to_le_bytes());
            out.extend_from_slice(&aim_y.to_le_bytes());
        }
        Message::Room { tick, seed, name } => {
            out.push(ROOM);
            out.extend_from_slice(&tick.to_le_bytes());
            out.extend_from_slice(&seed.to_le_bytes());
            push_str(&mut out, name);
        }
        Message::Checksum { tick, count, sum_x, sum_y, sum_hp } => {
            out.push(CHECKSUM);
            out.extend_from_slice(&tick.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&sum_x.to_le_bytes());
            out.extend_from_slice(&sum_y.to_le_bytes());
            out.extend_from_slice(&sum_hp.to_le_bytes());
        }
        Message::Snapshot { tick, bodies } => {
            out.push(SNAPSHOT);
            out.extend_from_slice(&tick.to_le_bytes());
            let count = bodies.len().min(u16::MAX as usize) as u16;
            out.extend_from_slice(&count.to_le_bytes());
            for body in bodies.iter().take(count as usize) {
                out.extend_from_slice(&body.x.to_le_bytes());
                out.extend_from_slice(&body.y.to_le_bytes());
                out.extend_from_slice(&body.hp.to_le_bytes());
            }
        }
    }
    out
}

pub fn decode(bytes: &[u8]) -> Option<Message> {
    if bytes.is_empty() {
        return None;
    }
    let mut cursor = Cursor { bytes, at: 1 };
    let tick = cursor.u32()?;
    match bytes[0] {
        HELLO => {
            let mode = PlayMode::from_byte(cursor.u8()?)?;
            let proto = cursor.u32()?;
            let mods = cursor.array()?;
            let data = cursor.array()?;
            let _ = tick;
            Some(Message::Hello { mode, proto, mods, data })
        }
        INPUT => Some(Message::Input {
            tick,
            buttons: cursor.u32()?,
            aim_x: cursor.i32()?,
            aim_y: cursor.i32()?,
        }),
        ROOM => Some(Message::Room {
            tick,
            seed: cursor.u32()?,
            name: cursor.str()?,
        }),
        CHECKSUM => Some(Message::Checksum {
            tick,
            count: cursor.u32()?,
            sum_x: cursor.i64()?,
            sum_y: cursor.i64()?,
            sum_hp: cursor.i64()?,
        }),
        SNAPSHOT => {
            let count = cursor.u16()? as usize;
            let mut bodies = Vec::with_capacity(count);
            for _ in 0..count {
                bodies.push(Body {
                    x: cursor.i32()?,
                    y: cursor.i32()?,
                    hp: cursor.i32()?,
                });
            }
            Some(Message::Snapshot { tick, bodies })
        }
        _ => None,
    }
}

pub fn reliable(message: &Message) -> bool {
    !matches!(message, Message::Input { .. })
}

fn push_str(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let len = bytes.len().min(u16::MAX as usize) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&bytes[..len as usize]);
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(len)?;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(self.u32()? as i32)
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn array(&mut self) -> Option<[u8; 32]> {
        self.take(32)?.try_into().ok()
    }
    fn str(&mut self) -> Option<String> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_roundtrip() {
        let samples = [
            Message::Hello {
                mode: PlayMode::Pvp,
                proto: PROTO,
                mods: [7; 32],
                data: [9; 32],
            },
            Message::Input {
                tick: 4,
                buttons: 15,
                aim_x: -3,
                aim_y: 8,
            },
            Message::Room {
                tick: 1,
                seed: 99,
                name: "rm_bar".into(),
            },
            Message::Checksum {
                tick: 30,
                count: 2,
                sum_x: 10,
                sum_y: -4,
                sum_hp: 200,
            },
            Message::Snapshot {
                tick: 30,
                bodies: vec![Body { x: 1, y: 2, hp: 3 }],
            },
        ];
        for message in samples {
            assert_eq!(decode(&encode(&message)).as_ref(), Some(&message));
        }
    }
}
