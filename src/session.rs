//! The title-menu page. Lua draws whatever rows this returns.

use crate::mode::PlayMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Title,
    Online,
    PickMode,
    JoinList,
    InLobby,
    InRoom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Friend {
    pub steam_id: u64,
    pub name: String,
    pub lobby: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Create(PlayMode),
    Join(u64),
    Leave,
    Refresh,
}

pub struct Menu {
    pub phase: Phase,
    pub cursor: usize,
    pub russian: bool,
    pub notice: String,
    pub friends: Vec<Friend>,
    pub mode: PlayMode,
    pub lobby_id: u64,
}

impl Menu {
    pub fn new() -> Self {
        Self {
            phase: Phase::Title,
            cursor: 0,
            russian: false,
            notice: String::new(),
            friends: Vec::new(),
            mode: PlayMode::Coop,
            lobby_id: 0,
        }
    }

    pub fn set_locale(&mut self, locale: &str) {
        self.russian = locale.eq_ignore_ascii_case("ru");
    }

    pub fn rows(&self) -> Vec<Row> {
        let text = |key| label(self.russian, key).to_string();
        match self.phase {
            Phase::Title => Vec::new(),
            Phase::Online => vec![
                Row { id: "create".into(), label: text("create") },
                Row { id: "join".into(), label: text("join") },
                Row { id: "back".into(), label: text("back") },
            ],
            Phase::PickMode => vec![
                Row { id: "coop".into(), label: text("coop") },
                Row { id: "pvp".into(), label: text("pvp") },
                Row { id: "back".into(), label: text("back") },
            ],
            Phase::JoinList => {
                let mut rows: Vec<Row> = self
                    .friends
                    .iter()
                    .map(|friend| Row {
                        id: format!("friend:{}", friend.steam_id),
                        label: friend.name.clone(),
                    })
                    .collect();
                if rows.is_empty() {
                    rows.push(Row { id: "empty".into(), label: text("empty") });
                }
                rows.push(Row { id: "back".into(), label: text("back") });
                rows
            }
            Phase::InLobby | Phase::InRoom => {
                vec![Row { id: "back".into(), label: text("back") }]
            }
        }
    }

    pub fn command(&mut self, name: &str) -> Effect {
        match name {
            "open" => {
                if self.phase == Phase::InLobby || self.phase == Phase::InRoom {
                    Effect::None
                } else {
                    self.enter(Phase::Online);
                    Effect::None
                }
            }
            "up" => {
                self.move_cursor(-1);
                Effect::None
            }
            "down" => {
                self.move_cursor(1);
                Effect::None
            }
            "back" => self.go_back(),
            "confirm" => self.confirm(),
            _ => Effect::None,
        }
    }

    pub fn fail(&mut self, notice: impl Into<String>) {
        self.notice = notice.into();
    }

    pub fn entered_lobby(&mut self, id: u64, mode: PlayMode) {
        self.lobby_id = id;
        self.mode = mode;
        self.notice.clear();
        self.enter(Phase::InLobby);
    }

    pub fn entered_room(&mut self) {
        self.enter(Phase::InRoom);
    }

    pub fn round_over(&mut self) {
        self.notice = label(self.russian, "round").to_string();
        self.enter(Phase::InLobby);
    }

    fn confirm(&mut self) -> Effect {
        let rows = self.rows();
        let Some(row) = rows.get(self.cursor) else {
            return Effect::None;
        };
        match row.id.as_str() {
            "create" => {
                self.enter(Phase::PickMode);
                Effect::None
            }
            "join" => {
                self.enter(Phase::JoinList);
                Effect::Refresh
            }
            "back" => self.go_back(),
            "coop" => {
                self.mode = PlayMode::Coop;
                Effect::Create(PlayMode::Coop)
            }
            "pvp" => {
                self.mode = PlayMode::Pvp;
                Effect::Create(PlayMode::Pvp)
            }
            "empty" => Effect::None,
            other if other.starts_with("friend:") => {
                let id: u64 = other.trim_start_matches("friend:").parse().unwrap_or(0);
                let lobby = self.friends.iter().find(|friend| friend.steam_id == id).map(|friend| friend.lobby).unwrap_or(0);
                if lobby == 0 {
                    self.notice = label(self.russian, "no_lobby").to_string();
                    Effect::None
                } else {
                    Effect::Join(lobby)
                }
            }
            _ => Effect::None,
        }
    }

    fn go_back(&mut self) -> Effect {
        let effect = if self.phase == Phase::InLobby || self.phase == Phase::InRoom {
            Effect::Leave
        } else {
            Effect::None
        };
        self.notice.clear();
        self.enter(match self.phase {
            Phase::Online | Phase::InLobby | Phase::InRoom => Phase::Title,
            Phase::PickMode | Phase::JoinList => Phase::Online,
            Phase::Title => Phase::Title,
        });
        effect
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.cursor = 0;
    }

    fn move_cursor(&mut self, delta: i32) {
        let len = self.rows().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        let next = self.cursor as i32 + delta;
        self.cursor = next.rem_euclid(len as i32) as usize;
    }
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

fn label(russian: bool, key: &str) -> &'static str {
    match (russian, key) {
        (true, "create") => "Создать лобби",
        (false, "create") => "Create lobby",
        (true, "join") => "Присоединиться",
        (false, "join") => "Join lobby",
        (true, "back") => "Назад",
        (false, "back") => "Back",
        (true, "coop") => "Совместное прохождение",
        (false, "coop") => "Co-op",
        (true, "pvp") => "PVP",
        (false, "pvp") => "PVP",
        (true, "empty") => "Никого нет в игре",
        (false, "empty") => "Nobody is in a session",
        (true, "no_lobby") => "У этой сессии нет лобби",
        (false, "no_lobby") => "That session has no lobby",
        (true, "round") => "Раунд окончен",
        (false, "round") => "Round over",
        (true, "mismatch") => "Другая версия, режим или набор модов",
        (false, "mismatch") => "Different version, mode, or mods",
        (true, "steam") => "Steam ещё не готов",
        (false, "steam") => "Steam is not ready",
        (true, "full") => "Лобби уже полное",
        (false, "full") => "That lobby is full",
        (true, "proto") => "Другая версия протокола",
        (false, "proto") => "Different protocol",
        (true, "mods") => "Другой набор модов",
        (false, "mods") => "Different mods",
        (true, "data") => "Другая версия игры",
        (false, "data") => "Different game data",
        _ => "",
    }
}

pub fn text(russian: bool, key: &str) -> &'static str {
    label(russian, key)
}

pub fn write_view(menu: &Menu) -> String {
    let mut out = format!("phase {}\ncursor {}\nnotice {}\nmode {}\nlobby {}\n", phase_name(menu.phase), menu.cursor, one_line(&menu.notice), menu.mode.as_str(), menu.lobby_id);
    for row in menu.rows() {
        out.push_str(&format!("row {} {}\n", row.id, row.label));
    }
    out
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Title => "title",
        Phase::Online => "online",
        Phase::PickMode => "mode",
        Phase::JoinList => "join",
        Phase::InLobby => "lobby",
        Phase::InRoom => "room",
    }
}

fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_page_creates_or_joins() {
        let mut menu = Menu::new();
        menu.set_locale("ru");
        menu.command("open");
        let rows = menu.rows();
        assert_eq!(rows[0].label, "Создать лобби");
        assert_eq!(rows[1].label, "Присоединиться");
        menu.command("confirm");
        assert_eq!(menu.phase, Phase::PickMode);
        assert_eq!(menu.command("confirm"), Effect::Create(PlayMode::Coop));
    }

    #[test]
    fn join_without_a_lobby_keeps_the_row_and_explains() {
        let mut menu = Menu::new();
        menu.command("open");
        menu.command("down");
        assert_eq!(menu.command("confirm"), Effect::Refresh);
        menu.friends.push(Friend { steam_id: 5, name: "Ada".into(), lobby: 0 });
        menu.cursor = 0;
        assert_eq!(menu.command("confirm"), Effect::None);
        assert_eq!(menu.notice, "That session has no lobby");
        menu.friends[0].lobby = 42;
        assert_eq!(menu.command("confirm"), Effect::Join(42));
    }
}
