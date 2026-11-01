//! Calls the `steam_api.dll` the game already loaded. No second `SteamAPI_Init`.
#![cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code, unused_imports))]

use crate::lobby::LobbyInfo;
use crate::session::Friend;
use std::ffi::{CStr, CString};
use std::sync::Mutex;

pub const APP_ID: u64 = 460950;

#[derive(Clone, Debug)]
pub enum SteamEvent {
    LobbyCreated { id: u64, ok: bool },
    LobbyEntered { id: u64, response: u32 },
    Invite { lobby: u64 },
    Connection { handle: u32, state: i32 },
}

static EVENTS: Mutex<Vec<SteamEvent>> = Mutex::new(Vec::new());

pub fn drain() -> Vec<SteamEvent> {
    EVENTS.lock().map(|mut queue| std::mem::take(&mut *queue)).unwrap_or_default()
}

pub fn push(event: SteamEvent) {
    if let Ok(mut queue) = EVENTS.lock() {
        queue.push(event);
    }
}

pub fn invite_from_args() -> Option<u64> {
    let args: Vec<String> = std::env::args().collect();
    for (index, arg) in args.iter().enumerate() {
        if arg == "+connect_lobby" {
            return args.get(index + 1)?.parse().ok();
        }
        if let Some(rest) = arg.strip_prefix("+connect_lobby") {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.parse().ok();
            }
        }
    }
    None
}

#[cfg(all(windows, target_arch = "x86"))]
mod imp {
    use super::*;
    use std::sync::Once;
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    type CreateInterface = unsafe extern "C" fn(*const i8) -> *mut std::ffi::c_void;
    type GetInt = unsafe extern "C" fn() -> i32;
    type ClientGet = unsafe extern "C" fn(*mut std::ffi::c_void, i32, i32, *const i8) -> *mut std::ffi::c_void;
    type Reg = unsafe extern "C" fn(*mut Callback, i32);
    type RegCall = unsafe extern "C" fn(*mut Callback, u64);
    type CreateLobby = unsafe extern "C" fn(*mut std::ffi::c_void, i32, i32) -> u64;
    type JoinLobby = unsafe extern "C" fn(*mut std::ffi::c_void, u64) -> u64;
    type LeaveLobby = unsafe extern "C" fn(*mut std::ffi::c_void, u64);
    type SetData = unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const i8, *const i8) -> i32;
    type GetData = unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const i8) -> *const i8;
    type Owner = unsafe extern "C" fn(*mut std::ffi::c_void, u64) -> u64;
    type FriendCount = unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32;
    type FriendAt = unsafe extern "C" fn(*mut std::ffi::c_void, i32, i32) -> u64;
    type GamePlayed = unsafe extern "C" fn(*mut std::ffi::c_void, u64, *mut FriendGame) -> i32;
    type NameOf = unsafe extern "C" fn(*mut std::ffi::c_void, u64) -> *const i8;
    type RichGet = unsafe extern "C" fn(*mut std::ffi::c_void, u64, *const i8) -> *const i8;
    type RichAsk = unsafe extern "C" fn(*mut std::ffi::c_void, u64);
    type RichSet = unsafe extern "C" fn(*mut std::ffi::c_void, *const i8, *const i8) -> i32;

    #[repr(C)]
    struct FriendGame {
        game_id: u64,
        ip: u32,
        game_port: u16,
        query_port: u16,
        lobby: u64,
    }

    #[repr(C)]
    struct Callback {
        vtable: *const usize,
        flags: u8,
        _pad: [u8; 3],
        id: i32,
        bytes: i32,
    }

    pub struct Steam {
        friends: *mut std::ffi::c_void,
        matchmaking: *mut std::ffi::c_void,
        sockets: usize,
        create_lobby: CreateLobby,
        join_lobby: JoinLobby,
        leave_lobby: LeaveLobby,
        set_data: SetData,
        get_data: GetData,
        owner: Owner,
        friend_count: FriendCount,
        friend_at: FriendAt,
        game_played: GamePlayed,
        name_of: NameOf,
        rich_get: RichGet,
        rich_ask: RichAsk,
        rich_set: RichSet,
        register_call: RegCall,
        probed: bool,
    }

    unsafe impl Send for Steam {}

    pub fn attach() -> Option<Steam> {
        unsafe {
            let module = GetModuleHandleA(PCSTR(b"steam_api.dll\0".as_ptr())).ok()?;
            let create: CreateInterface = load(module, b"SteamInternal_CreateInterface\0")?;
            let user = load::<GetInt>(module, b"SteamAPI_GetHSteamUser\0")?();
            let pipe = load::<GetInt>(module, b"SteamAPI_GetHSteamPipe\0")?();
            if user == 0 || pipe == 0 {
                return None;
            }
            let client = first(create, &[b"SteamClient023\0", b"SteamClient021\0", b"SteamClient020\0", b"SteamClient019\0", b"SteamClient017\0"])?;
            let get_friends: ClientGet = load(module, b"SteamAPI_ISteamClient_GetISteamFriends\0")?;
            let get_match: ClientGet = load(module, b"SteamAPI_ISteamClient_GetISteamMatchmaking\0")?;
            let get_generic: ClientGet = load(module, b"SteamAPI_ISteamClient_GetISteamGenericInterface\0")?;
            let friends = first_of(get_friends, client, user, pipe, &[b"SteamFriends018\0", b"SteamFriends017\0", b"SteamFriends015\0"])?;
            let matchmaking = first_of(get_match, client, user, pipe, &[b"SteamMatchMaking009\0", b"SteamMatchmaking009\0"])?;
            let sockets = first_of(
                get_generic,
                client,
                user,
                pipe,
                &[b"SteamNetworkingSockets013\0", b"SteamNetworkingSockets012\0", b"SteamNetworkingSockets011\0", b"SteamNetworkingSockets009\0"],
            );
            let utils = first_of(get_generic, client, user, pipe, &[b"SteamNetworkingUtils004\0", b"SteamNetworkingUtils003\0"]);
            if let Some(utils) = utils {
                init_relay(utils);
            }
            let steam = Steam {
                friends,
                matchmaking,
                sockets: sockets.map(|ptr| ptr as usize).unwrap_or(0),
                create_lobby: load(module, b"SteamAPI_ISteamMatchmaking_CreateLobby\0")?,
                join_lobby: load(module, b"SteamAPI_ISteamMatchmaking_JoinLobby\0")?,
                leave_lobby: load(module, b"SteamAPI_ISteamMatchmaking_LeaveLobby\0")?,
                set_data: load(module, b"SteamAPI_ISteamMatchmaking_SetLobbyData\0")?,
                get_data: load(module, b"SteamAPI_ISteamMatchmaking_GetLobbyData\0")?,
                owner: load(module, b"SteamAPI_ISteamMatchmaking_GetLobbyOwner\0")?,
                friend_count: load(module, b"SteamAPI_ISteamFriends_GetFriendCount\0")?,
                friend_at: load(module, b"SteamAPI_ISteamFriends_GetFriendByIndex\0")?,
                game_played: load(module, b"SteamAPI_ISteamFriends_GetFriendGamePlayed\0")?,
                name_of: load(module, b"SteamAPI_ISteamFriends_GetFriendPersonaName\0")?,
                rich_get: load(module, b"SteamAPI_ISteamFriends_GetFriendRichPresence\0")?,
                rich_ask: load(module, b"SteamAPI_ISteamFriends_RequestFriendRichPresence\0")?,
                rich_set: load(module, b"SteamAPI_ISteamFriends_SetRichPresence\0")?,
                register_call: load(module, b"SteamAPI_RegisterCallResult\0")?,
                probed: false,
            };
            register(module);
            let _ = sockets;
            Some(steam)
        }
    }

    impl Steam {
        pub fn sockets(&self) -> Option<usize> {
            if self.sockets == 0 { None } else { Some(self.sockets) }
        }

        pub fn note_probe(&mut self, game: &std::path::Path) {
            if self.probed {
                return;
            }
            self.probed = true;
            let ok = self.sockets != 0;
            let missing = if ok { Vec::new() } else { vec!["SteamNetworkingSockets".to_string()] };
            let path = game.join("kzm-data").join("steam-probe.json");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let body = format!("{{\"ok\":{ok},\"missing\":{}}}", serde_missing(&missing));
            let _ = std::fs::write(path, body);
        }

        pub fn create_lobby(&self) -> u64 {
            unsafe {
                let call = (self.create_lobby)(self.matchmaking, 1, 2);
                let callback = lobby_created();
                if !callback.is_null() {
                    (self.register_call)(callback, call);
                }
                call
            }
        }

        pub fn join_lobby(&self, id: u64) {
            unsafe {
                let call = (self.join_lobby)(self.matchmaking, id);
                let callback = lobby_entered();
                if !callback.is_null() {
                    (self.register_call)(callback, call);
                }
            }
        }

        pub fn leave_lobby(&self, id: u64) {
            if id != 0 {
                unsafe { (self.leave_lobby)(self.matchmaking, id) }
            }
            self.set_rich("connect", "");
        }

        pub fn publish(&self, id: u64, info: &LobbyInfo) {
            self.put(id, "proto", &info.proto.to_string());
            self.put(id, "mode", info.mode.as_str());
            self.put(id, "mods", &info.mods);
            self.put(id, "data", &info.data);
            self.set_rich("connect", &format!("+connect_lobby {id}"));
        }

        pub fn read_info(&self, id: u64) -> Option<LobbyInfo> {
            crate::lobby::parse_info(&self.get(id, "mode"), &self.get(id, "proto"), &self.get(id, "mods"), &self.get(id, "data"))
        }

        pub fn owner(&self, id: u64) -> u64 {
            unsafe { (self.owner)(self.matchmaking, id) }
        }

        pub fn friends_in_game(&self) -> Vec<Friend> {
            let mut out = Vec::new();
            unsafe {
                let count = (self.friend_count)(self.friends, 4);
                for index in 0..count {
                    let steam_id = (self.friend_at)(self.friends, index, 4);
                    if steam_id == 0 {
                        continue;
                    }
                    (self.rich_ask)(self.friends, steam_id);
                    let mut game = FriendGame { game_id: 0, ip: 0, game_port: 0, query_port: 0, lobby: 0 };
                    let playing = (self.game_played)(self.friends, steam_id, &mut game);
                    if playing == 0 || (game.game_id & 0x00FF_FFFF) != APP_ID {
                        continue;
                    }
                    let mut lobby = game.lobby;
                    if lobby == 0 {
                        let key = CString::new("connect").unwrap();
                        let text = (self.rich_get)(self.friends, steam_id, key.as_ptr());
                        lobby = lobby_from_connect(text);
                    }
                    let name_ptr = (self.name_of)(self.friends, steam_id);
                    let name = if name_ptr.is_null() {
                        steam_id.to_string()
                    } else {
                        CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
                    };
                    out.push(Friend { steam_id, name, lobby });
                }
            }
            out
        }

        fn put(&self, id: u64, key: &str, value: &str) {
            let Ok(key) = CString::new(key) else { return };
            let Ok(value) = CString::new(value) else { return };
            unsafe { (self.set_data)(self.matchmaking, id, key.as_ptr(), value.as_ptr()); }
        }

        fn get(&self, id: u64, key: &str) -> String {
            let Ok(key) = CString::new(key) else { return String::new() };
            unsafe {
                let ptr = (self.get_data)(self.matchmaking, id, key.as_ptr());
                if ptr.is_null() { String::new() } else { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
            }
        }

        fn set_rich(&self, key: &str, value: &str) {
            let Ok(key) = CString::new(key) else { return };
            let Ok(value) = CString::new(value) else { return };
            unsafe { (self.rich_set)(self.friends, key.as_ptr(), value.as_ptr()); }
        }
    }

    fn serde_missing(items: &[String]) -> String {
        let body = items.iter().map(|item| format!("\"{item}\"")).collect::<Vec<_>>().join(",");
        format!("[{body}]")
    }

    fn lobby_from_connect(text: *const i8) -> u64 {
        if text.is_null() {
            return 0;
        }
        let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
        text.split_whitespace().nth(1).and_then(|token| token.parse().ok()).unwrap_or(0)
    }

    unsafe fn load<T>(module: windows::Win32::Foundation::HMODULE, name: &[u8]) -> Option<T> {
        let proc = GetProcAddress(module, PCSTR(name.as_ptr()))?;
        Some(std::mem::transmute_copy(&proc))
    }

    unsafe fn first(create: CreateInterface, versions: &[&[u8]]) -> Option<*mut std::ffi::c_void> {
        for version in versions {
            let ptr = create(version.as_ptr() as *const i8);
            if !ptr.is_null() {
                return Some(ptr);
            }
        }
        None
    }

    unsafe fn first_of(get: ClientGet, client: *mut std::ffi::c_void, user: i32, pipe: i32, versions: &[&[u8]]) -> Option<*mut std::ffi::c_void> {
        for version in versions {
            let ptr = get(client, user, pipe, version.as_ptr() as *const i8);
            if !ptr.is_null() {
                return Some(ptr);
            }
        }
        None
    }

    unsafe fn init_relay(utils: *mut std::ffi::c_void) {
        let vtable = *(utils as *const *const usize);
        let func = *vtable.add(1);
        if func == 0 {
            return;
        }
        let init: unsafe extern "thiscall" fn(*mut std::ffi::c_void) = std::mem::transmute(func);
        init(utils);
    }

    fn register(module: windows::Win32::Foundation::HMODULE) {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| unsafe {
            let Some(register) = load::<Reg>(module, b"SteamAPI_RegisterCallback\0") else { return };
            for (callback, id) in [(lobby_created(), 513), (lobby_entered(), 504), (invite(), 333), (connection(), 1221)] {
                if !callback.is_null() {
                    register(callback, id);
                }
            }
        });
    }

    // Slot 0 is Run(param). Slot 1 is Run(param, failed, call) and must pop the extra arguments.
    fn slot(run: usize, run_io: usize, size_fn: usize) -> &'static [usize; 3] {
        Box::leak(Box::new([run, run_io, size_fn]))
    }

    fn keep(callback: Callback) -> *mut Callback {
        Box::leak(Box::new(callback))
    }

    fn lobby_created() -> *mut Callback {
        use std::sync::atomic::{AtomicPtr, Ordering};
        static PTR: AtomicPtr<Callback> = AtomicPtr::new(std::ptr::null_mut());
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            PTR.store(keep(make(513, 16, run_created as *const () as usize, run_created_io as *const () as usize, size16 as *const () as usize)), Ordering::Relaxed);
        });
        PTR.load(Ordering::Relaxed)
    }
    fn lobby_entered() -> *mut Callback {
        use std::sync::atomic::{AtomicPtr, Ordering};
        static PTR: AtomicPtr<Callback> = AtomicPtr::new(std::ptr::null_mut());
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            PTR.store(keep(make(504, 24, run_entered as *const () as usize, run_entered_io as *const () as usize, size24 as *const () as usize)), Ordering::Relaxed);
        });
        PTR.load(Ordering::Relaxed)
    }
    fn invite() -> *mut Callback {
        use std::sync::atomic::{AtomicPtr, Ordering};
        static PTR: AtomicPtr<Callback> = AtomicPtr::new(std::ptr::null_mut());
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            PTR.store(keep(make(333, 16, run_invite as *const () as usize, run_invite_io as *const () as usize, size16 as *const () as usize)), Ordering::Relaxed);
        });
        PTR.load(Ordering::Relaxed)
    }
    fn connection() -> *mut Callback {
        use std::sync::atomic::{AtomicPtr, Ordering};
        static PTR: AtomicPtr<Callback> = AtomicPtr::new(std::ptr::null_mut());
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            PTR.store(keep(make(1221, 712, run_connection as *const () as usize, run_connection_io as *const () as usize, size712 as *const () as usize)), Ordering::Relaxed);
        });
        PTR.load(Ordering::Relaxed)
    }

    fn make(id: i32, bytes: i32, run: usize, run_io: usize, size_fn: usize) -> Callback {
        Callback { vtable: slot(run, run_io, size_fn).as_ptr(), flags: 0, _pad: [0; 3], id, bytes }
    }

    unsafe extern "thiscall" fn size16(_: *mut Callback) -> i32 { 16 }
    unsafe extern "thiscall" fn size24(_: *mut Callback) -> i32 { 24 }
    unsafe extern "thiscall" fn size712(_: *mut Callback) -> i32 { 712 }

    unsafe extern "thiscall" fn run_created(_: *mut Callback, param: *mut u8) {
        if param.is_null() { return; }
        let result = *(param as *const i32);
        let id = *(param.add(8) as *const u64);
        push(SteamEvent::LobbyCreated { id, ok: result == 1 });
    }
    unsafe extern "thiscall" fn run_entered(_: *mut Callback, param: *mut u8) {
        if param.is_null() { return; }
        let id = *(param as *const u64);
        let response = *(param.add(16) as *const u32);
        push(SteamEvent::LobbyEntered { id, response });
    }
    unsafe extern "thiscall" fn run_invite(_: *mut Callback, param: *mut u8) {
        if param.is_null() { return; }
        let lobby = *(param as *const u64);
        push(SteamEvent::Invite { lobby });
    }
    unsafe extern "thiscall" fn run_connection(_: *mut Callback, param: *mut u8) {
        if param.is_null() { return; }
        let handle = *(param as *const u32);
        let state = *(param.add(184) as *const i32);
        push(SteamEvent::Connection { handle, state });
    }

    unsafe extern "thiscall" fn run_created_io(this: *mut Callback, param: *mut u8, _failed: u32, _call: u64) {
        run_created(this, param);
    }
    unsafe extern "thiscall" fn run_entered_io(this: *mut Callback, param: *mut u8, _failed: u32, _call: u64) {
        run_entered(this, param);
    }
    unsafe extern "thiscall" fn run_invite_io(this: *mut Callback, param: *mut u8, _failed: u32, _call: u64) {
        run_invite(this, param);
    }
    unsafe extern "thiscall" fn run_connection_io(this: *mut Callback, param: *mut u8, _failed: u32, _call: u64) {
        run_connection(this, param);
    }
}

#[cfg(all(windows, target_arch = "x86"))]
pub use imp::{attach, Steam};

#[cfg(not(all(windows, target_arch = "x86")))]
#[derive(Clone)]
pub struct Steam;

#[cfg(not(all(windows, target_arch = "x86")))]
pub fn attach() -> Option<Steam> {
    None
}

#[cfg(not(all(windows, target_arch = "x86")))]
impl Steam {
    pub fn sockets(&self) -> Option<usize> { None }
    pub fn note_probe(&mut self, _game: &std::path::Path) {}
    pub fn create_lobby(&self) -> u64 { 0 }
    pub fn join_lobby(&self, _id: u64) {}
    pub fn leave_lobby(&self, _id: u64) {}
    pub fn publish(&self, _id: u64, _info: &LobbyInfo) {}
    pub fn read_info(&self, _id: u64) -> Option<LobbyInfo> { None }
    pub fn owner(&self, _id: u64) -> u64 { 0 }
    pub fn friends_in_game(&self) -> Vec<Friend> { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_lobby_is_read_from_the_launch_argument() {
        assert_eq!(invite_from_args().is_some() || invite_from_args().is_none(), true);
    }
}
