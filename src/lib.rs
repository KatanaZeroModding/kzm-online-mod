//! Katana ZERO online session. Lua only forwards menu keys and instance fields.

mod link;
mod lobby;
mod lockstep;
mod mode;
mod session;
mod steam;
mod wire;

use lockstep::{Checksum, Input, Lockstep};
use mode::PlayMode;
use session::{Effect, Menu, Phase};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use wire::{Body, Message, PROTO};

const FREEZE: u32 = 1;
const IN_ROOM: u32 = 2;
const COOP: u32 = 4;
const PVP: u32 = 8;
const ROUND: u32 = 16;
const NEED_SNAP: u32 = 32;
const APPLY_SNAP: u32 = 64;
const BLOCK_STORY: u32 = 128;

struct Online {
    menu: Menu,
    lock: Lockstep,
    steam: Option<steam::Steam>,
    link: Option<link::Link>,
    is_host: bool,
    info: Option<lobby::LobbyInfo>,
    mods: [u8; 32],
    data: [u8; 32],
    hello_sent: bool,
    invited: bool,
    room: String,
    peer_room: String,
    snapshot: Vec<Body>,
    need_snapshot: bool,
    apply_snapshot: bool,
    round_over: bool,
    saw_peer: bool,
    last_sent_room: String,
    seed: u32,
    remote: Input,
    sim_tick: u32,
    have_tick: bool,
    synced: bool,
}

static ONLINE: Mutex<Option<Online>> = Mutex::new(None);

#[no_mangle]
pub extern "C" fn KZM_PluginInit(_mod_id: *const i8) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn KZM_BindLua(_state: *mut std::ffi::c_void) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn online_step() -> i32 {
    let Ok(mut slot) = ONLINE.lock() else {
        return 0;
    };
    if slot.is_none() {
        *slot = Some(Online::boot());
    }
    if let Some(online) = slot.as_mut() {
        online.step();
    }
    0
}

impl Online {
    fn boot() -> Self {
        let game = game_dir();
        let mods = hash_file(&game.join("kzm").join("__kzm_mods.lua"));
        let data = hash_file(&game.join("data.win"));
        let mut online = Self {
            menu: Menu::new(),
            lock: Lockstep::new(),
            steam: steam::attach(),
            link: None,
            is_host: false,
            info: None,
            mods,
            data,
            hello_sent: false,
            invited: false,
            room: String::new(),
            peer_room: String::new(),
            snapshot: Vec::new(),
            need_snapshot: false,
            apply_snapshot: false,
            round_over: false,
            saw_peer: false,
            last_sent_room: String::new(),
            seed: 0,
            remote: Input::default(),
            sim_tick: 0,
            have_tick: false,
            synced: false,
        };
        if let Some(steam) = online.steam.as_mut() {
            steam.note_probe(&game);
        }
        online
    }

    fn step(&mut self) {
        let game = game_dir();
        let notes = fs::read_to_string(game.join("kzm").join("online-in.txt")).unwrap_or_default();
        let mut command = None;
        let mut input = Input::default();
        let mut checksum = None;
        let mut bodies = Vec::new();
        let mut died = false;
        for line in notes.lines() {
            let mut parts = line.split_whitespace();
            match parts.next() {
                Some("locale") => self.menu.set_locale(parts.next().unwrap_or("en")),
                Some("cmd") => command = parts.next().map(str::to_string),
                Some("input") => {
                    input.buttons = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    input.aim_x = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    input.aim_y = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                Some("room") => self.room = parts.next().unwrap_or("").to_string(),
                Some("checksum") => checksum = parse_checksum(parts),
                Some("body") => {
                    if let Some(body) = parse_body(parts) {
                        bodies.push(body);
                    }
                }
                Some("died") => died = true,
                _ => {}
            }
        }
        if let Some(command) = command {
            self.on_command(&command);
        }
        self.on_events();
        if !self.invited {
            self.invited = true;
            if let Some(lobby) = steam::invite_from_args() {
                self.menu.command("open");
                self.join(lobby);
            }
        }
        if self.menu.phase == Phase::JoinList {
            if let Some(steam) = self.steam.as_ref() {
                self.menu.friends = steam.friends_in_game();
            }
        }
        self.on_net(input, checksum, bodies, died);
        let _ = fs::create_dir_all(game.join("kzm"));
        let _ = fs::write(game.join("kzm").join("online-out.txt"), self.view());
        let _ = fs::write(game.join("kzm").join("online-frame.bin"), self.frame(input));
        if self.apply_snapshot {
            self.snapshot.clear();
            self.apply_snapshot = false;
        }
    }

    fn on_command(&mut self, command: &str) {
        match self.menu.command(command) {
            Effect::Create(mode) => self.create(mode),
            Effect::Join(id) => self.join(id),
            Effect::Leave => self.leave(),
            Effect::Refresh | Effect::None => {}
        }
    }

    fn create(&mut self, mode: PlayMode) {
        let Some(steam) = self.steam.as_ref() else {
            self.menu.fail(session::text(self.menu.russian, "steam"));
            return;
        };
        self.is_host = true;
        self.info = Some(lobby::LobbyInfo::new(mode, self.mods, self.data));
        self.menu.mode = mode;
        steam.create_lobby();
    }

    fn join(&mut self, id: u64) {
        let Some(steam) = self.steam.as_ref() else {
            self.menu.fail(session::text(self.menu.russian, "steam"));
            return;
        };
        self.is_host = false;
        steam.join_lobby(id);
    }

    fn leave(&mut self) {
        if let Some(steam) = self.steam.as_ref() {
            steam.leave_lobby(self.menu.lobby_id);
        }
        self.link = None;
        self.hello_sent = false;
        self.saw_peer = false;
        self.have_tick = false;
        self.synced = false;
        self.peer_room.clear();
        self.last_sent_room.clear();
        self.lock.reset();
        self.menu.lobby_id = 0;
    }

    fn on_events(&mut self) {
        for event in steam::drain() {
            match event {
                steam::SteamEvent::LobbyCreated { id, ok } => {
                    if ok && self.is_host {
                        if let Some(info) = self.info.clone() {
                            if let Some(steam) = self.steam.as_ref() {
                                steam.publish(id, &info);
                            }
                        }
                        self.menu.entered_lobby(id, self.menu.mode);
                        self.open_link(0);
                    } else if !ok {
                        self.menu.fail(session::text(self.menu.russian, "steam"));
                    }
                }
                steam::SteamEvent::LobbyEntered { id, response } => {
                    if self.is_host {
                        continue;
                    }
                    if response == 4 {
                        self.menu.fail(session::text(self.menu.russian, "full"));
                        continue;
                    }
                    if response != 1 {
                        self.menu.fail(session::text(self.menu.russian, "no_lobby"));
                        continue;
                    }
                    let info = self.steam.as_ref().and_then(|steam| steam.read_info(id));
                    let Some(info) = info else {
                        self.menu.fail(session::text(self.menu.russian, "no_lobby"));
                        if let Some(steam) = self.steam.as_ref() {
                            steam.leave_lobby(id);
                        }
                        continue;
                    };
                    let expected = lobby::LobbyInfo::new(self.menu.mode, self.mods, self.data);
                    let reject = if info.proto != PROTO {
                        Some("proto")
                    } else if info.mods != expected.mods {
                        Some("mods")
                    } else if info.data != expected.data {
                        Some("data")
                    } else {
                        None
                    };
                    if let Some(key) = reject {
                        self.menu.fail(session::text(self.menu.russian, key));
                        if let Some(steam) = self.steam.as_ref() {
                            steam.leave_lobby(id);
                        }
                        continue;
                    }
                    self.menu.mode = info.mode;
                    let owner = self.steam.as_ref().map(|steam| steam.owner(id)).unwrap_or(0);
                    self.menu.entered_lobby(id, self.menu.mode);
                    self.open_link(owner);
                }
                steam::SteamEvent::Invite { lobby } => self.join(lobby),
                steam::SteamEvent::Connection { handle, state } => {
                    if state == 4 || state == 5 {
                        continue;
                    }
                    let mode = self.menu.mode;
                    let delay = self.lock.delay();
                    let mods = self.mods;
                    let data = self.data;
                    let hello = self.hello_sent;
                    if let Some(link) = self.link.as_ref() {
                        link.accept(handle);
                        if !hello {
                            link.send(&Message::Hello { mode, proto: PROTO, mods, data });
                            for tick in 0..delay {
                                link.send(&Message::Input { tick, buttons: 0, aim_x: 0, aim_y: 0 });
                            }
                        }
                    }
                    self.hello_sent = true;
                }
            }
        }
    }

    fn open_link(&mut self, remote: u64) {
        let Some(sockets) = self.steam.as_ref().and_then(|steam| steam.sockets()) else {
            self.menu.fail(session::text(self.menu.russian, "steam"));
            return;
        };
        if self.link.is_none() {
            self.link = Some(link::Link::spawn(sockets, self.is_host, remote));
        }
    }

    fn on_net(&mut self, input: Input, checksum: Option<Checksum>, bodies: Vec<Body>, died: bool) {
        if died && self.menu.mode.end_round_on_death() {
            self.round_over = true;
            self.menu.round_over();
            self.lock.reset();
            self.saw_peer = false;
            self.have_tick = false;
            self.synced = false;
            self.peer_room.clear();
            self.last_sent_room.clear();
            return;
        }
        if let Some(link) = self.link.as_ref() {
            let rtt = link.rtt_ms();
            if !self.hello_sent && rtt > 0 {
                self.lock.set_delay_from_rtt(rtt);
            }
            if gameplay_room(&self.room) && self.room != self.last_sent_room {
                link.send(&Message::Room { tick: self.lock.next_tick(), seed: self.menu.lobby_id as u32, name: self.room.clone() });
                self.last_sent_room = self.room.clone();
            }
            if self.need_snapshot && !bodies.is_empty() {
                link.send(&Message::Snapshot { tick: self.lock.next_tick(), bodies });
                self.need_snapshot = false;
            }
        }
        let incoming = self.link.as_ref().map(|link| link.recv()).unwrap_or_default();
        for message in incoming {
            self.on_message(message, checksum);
        }
        self.synced = self.saw_peer && gameplay_room(&self.room) && self.room == self.peer_room;
        if self.synced && self.menu.phase != Phase::InRoom {
            self.menu.entered_room();
        }
        self.apply_snapshot = !self.snapshot.is_empty();
        self.have_tick = false;
        if self.synced {
            if let Some(link) = self.link.as_ref() {
                let stamped = self.lock.push_local(input);
                link.send(&Message::Input { tick: stamped, buttons: input.buttons, aim_x: input.aim_x, aim_y: input.aim_y });
                if let Some(sum) = checksum {
                    link.send(&Message::Checksum { tick: sum.tick, count: sum.count, sum_x: sum.sum_x, sum_y: sum.sum_y, sum_hp: sum.sum_hp });
                }
            }
            if let Some(sim) = self.lock.ready() {
                self.remote = sim.remote;
                self.sim_tick = sim.tick;
                self.have_tick = true;
                self.lock.advance();
                self.round_over = false;
            }
        }
    }

    fn on_message(&mut self, message: Message, checksum: Option<Checksum>) {
        match message {
            Message::Hello { mode, proto, mods, data } => {
                if proto != PROTO || mods != self.mods || data != self.data || mode != self.menu.mode {
                    self.menu.fail(session::text(self.menu.russian, "mismatch"));
                    self.leave();
                    return;
                }
                self.saw_peer = true;
                self.menu.entered_room();
            }
            Message::Input { tick, buttons, aim_x, aim_y } => {
                self.lock.push_remote(tick, Input { buttons, aim_x, aim_y });
            }
            Message::Room { name, seed, .. } => {
                self.peer_room = name;
                self.seed = seed;
            }
            Message::Checksum { tick, count, sum_x, sum_y, sum_hp } => {
                if let Some(local) = checksum {
                    let remote = Checksum { tick, count, sum_x, sum_y, sum_hp };
                    if self.is_host && lockstep::diverged(local, remote) {
                        self.need_snapshot = true;
                    }
                }
            }
            Message::Snapshot { bodies, .. } => self.snapshot = bodies,
        }
    }

    fn view(&self) -> String {
        let mut text = session::write_view(&self.menu);
        text.push_str(&format!("host {}\n", u32::from(self.is_host)));
        text.push_str(&format!("tick {}\n", if self.have_tick { self.sim_tick } else { self.lock.next_tick() }));
        text.push_str(&format!("seed {}\n", self.seed));
        text.push_str(&format!("room_target {}\n", self.peer_room));
        text.push_str(&format!("freeze {}\n", u32::from(self.frozen())));
        text.push_str(&format!("need_snap {}\n", u32::from(self.need_snapshot)));
        text.push_str(&format!("damage {}\n", u32::from(self.menu.mode.players_damage_each_other())));
        text.push_str(&format!("story {}\n", u32::from(self.menu.mode.wait_for_story() && !self.block_story())));
        if self.apply_snapshot {
            for body in &self.snapshot {
                text.push_str(&format!("snap {} {} {}\n", body.x, body.y, body.hp));
            }
        }
        text
    }

    fn frame(&self, input: Input) -> Vec<u8> {
        let remote = if self.have_tick { self.remote } else { Input::default() };
        let mut flags = 0;
        if self.frozen() { flags |= FREEZE; }
        if self.menu.phase == Phase::InRoom { flags |= IN_ROOM; }
        if self.menu.mode == PlayMode::Coop { flags |= COOP; }
        if self.menu.mode == PlayMode::Pvp { flags |= PVP; }
        if self.round_over { flags |= ROUND; }
        if self.need_snapshot { flags |= NEED_SNAP; }
        if self.apply_snapshot { flags |= APPLY_SNAP; }
        if self.block_story() { flags |= BLOCK_STORY; }
        let mut bytes = Vec::with_capacity(36);
        bytes.extend_from_slice(&0x4B5A4D31u32.to_le_bytes());
        let tick = if self.have_tick { self.sim_tick } else { self.lock.next_tick() };
        bytes.extend_from_slice(&tick.to_le_bytes());
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&input.buttons.to_le_bytes());
        bytes.extend_from_slice(&remote.buttons.to_le_bytes());
        bytes.extend_from_slice(&input.aim_x.to_le_bytes());
        bytes.extend_from_slice(&input.aim_y.to_le_bytes());
        bytes.extend_from_slice(&remote.aim_x.to_le_bytes());
        bytes.extend_from_slice(&remote.aim_y.to_le_bytes());
        bytes
    }

    fn frozen(&self) -> bool {
        self.synced && !self.have_tick
    }

    fn block_story(&self) -> bool {
        if self.menu.mode.end_round_on_death() {
            return true;
        }
        if !self.is_host {
            return self.saw_peer;
        }
        gameplay_room(&self.peer_room) && self.peer_room != self.room
    }
}

fn gameplay_room(name: &str) -> bool {
    !name.is_empty() && name != "room_title" && name != "room_logo" && name != "room_init" && !name.starts_with("room_splash")
}

fn game_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|path| path.parent().map(Path::to_path_buf)).unwrap_or_else(|| PathBuf::from("."))
}

fn hash_file(path: &Path) -> [u8; 32] {
    let mut hasher = Sha256::new();
    if let Ok(bytes) = fs::read(path) {
        hasher.update(bytes);
    }
    hasher.finalize().into()
}

fn parse_checksum<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<Checksum> {
    Some(Checksum {
        tick: parts.next()?.parse().ok()?,
        count: parts.next()?.parse().ok()?,
        sum_x: parts.next()?.parse().ok()?,
        sum_y: parts.next()?.parse().ok()?,
        sum_hp: parts.next()?.parse().ok()?,
    })
}

fn parse_body<'a>(mut parts: impl Iterator<Item = &'a str>) -> Option<Body> {
    Some(Body {
        x: parts.next()?.parse().ok()?,
        y: parts.next()?.parse().ok()?,
        hp: parts.next()?.parse().ok()?,
    })
}
