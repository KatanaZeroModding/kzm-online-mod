//! P2P socket thread. The game thread only pushes packets and reads the queue.
#![cfg_attr(not(all(windows, target_arch = "x86")), allow(dead_code, unused_imports))]

use crate::wire::{self, Message};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RELIABLE: i32 = 9;
const FAST: i32 = 5;

enum Cmd {
    Accept(u32),
    Send { bytes: Vec<u8>, reliable: bool },
    Stop,
}

pub struct Link {
    tx: Sender<Cmd>,
    incoming: Arc<Mutex<Vec<Message>>>,
    rtt_ms: Arc<std::sync::atomic::AtomicU32>,
}

impl Link {
    pub fn spawn(sockets: usize, host: bool, remote: u64) -> Self {
        let (tx, rx) = mpsc::channel();
        let incoming = Arc::new(Mutex::new(Vec::new()));
        let rtt_ms = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let incoming_thread = incoming.clone();
        let rtt_thread = rtt_ms.clone();
        std::thread::spawn(move || worker(sockets, host, remote, rx, incoming_thread, rtt_thread));
        Self { tx, incoming, rtt_ms }
    }

    pub fn send(&self, message: &Message) {
        let _ = self.tx.send(Cmd::Send {
            bytes: wire::encode(message),
            reliable: wire::reliable(message),
        });
    }

    pub fn accept(&self, handle: u32) {
        let _ = self.tx.send(Cmd::Accept(handle));
    }

    pub fn recv(&self) -> Vec<Message> {
        self.incoming.lock().map(|mut queue| std::mem::take(&mut *queue)).unwrap_or_default()
    }

    pub fn rtt_ms(&self) -> u32 {
        self.rtt_ms.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn worker(
    sockets: usize,
    host: bool,
    remote: u64,
    rx: Receiver<Cmd>,
    incoming: Arc<Mutex<Vec<Message>>>,
    rtt_ms: Arc<std::sync::atomic::AtomicU32>,
) {
    if sockets == 0 {
        return;
    }
    let this = sockets as *mut std::ffi::c_void;
    let listen = if host { vcall3(this, 2, 0, 0, 0) } else { 0 };
    let mut conn = if host { 0 } else { connect_p2p(this, remote) };
    let poll = vcall0(this, 24);
    let _ = listen;
    if conn != 0 && poll != 0 {
        let _ = vcall2(this, 26, conn, poll);
    }
    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Stop => return,
                Cmd::Accept(handle) => {
                    let result = vcall1(this, 4, handle);
                    if result == 1 || conn == 0 {
                        conn = handle;
                    }
                    if poll != 0 && conn != 0 {
                        let _ = vcall2(this, 26, conn, poll);
                    }
                }
                Cmd::Send { bytes, reliable } => {
                    if conn != 0 {
                        send_bytes(this, conn, &bytes, if reliable { RELIABLE } else { FAST });
                    }
                }
            }
        }
        if conn != 0 && poll != 0 {
            pull(this, poll, &incoming);
            if let Some(ping) = ping_ms(this, conn) {
                rtt_ms.store(ping, std::sync::atomic::Ordering::Relaxed);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(not(all(windows, target_arch = "x86")))]
fn worker(
    _sockets: usize,
    _host: bool,
    _remote: u64,
    rx: Receiver<Cmd>,
    _incoming: Arc<Mutex<Vec<Message>>>,
    _rtt_ms: Arc<std::sync::atomic::AtomicU32>,
) {
    while let Ok(cmd) = rx.recv() {
        if matches!(cmd, Cmd::Stop) {
            break;
        }
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn slot(this: *mut std::ffi::c_void, index: usize) -> usize {
    unsafe {
        let vtable = *(this as *const *const usize);
        *vtable.add(index)
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn vcall0(this: *mut std::ffi::c_void, index: usize) -> u32 {
    let func = slot(this, index);
    if func == 0 {
        return 0;
    }
    unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void) -> u32 = std::mem::transmute(func);
        call(this)
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn vcall1(this: *mut std::ffi::c_void, index: usize, arg: u32) -> u32 {
    let func = slot(this, index);
    if func == 0 {
        return 0;
    }
    unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32) -> u32 = std::mem::transmute(func);
        call(this, arg)
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn vcall2(this: *mut std::ffi::c_void, index: usize, a: u32, b: u32) -> u32 {
    let func = slot(this, index);
    if func == 0 {
        return 0;
    }
    unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32, u32) -> u32 = std::mem::transmute(func);
        call(this, a, b)
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn vcall3(this: *mut std::ffi::c_void, index: usize, a: u32, b: u32, c: u32) -> u32 {
    let func = slot(this, index);
    if func == 0 {
        return 0;
    }
    unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32, u32, u32) -> u32 = std::mem::transmute(func);
        call(this, a, b, c)
    }
}

#[cfg(all(windows, target_arch = "x86"))]
#[repr(C, packed)]
struct Identity {
    kind: i32,
    size: i32,
    steam_id: u64,
    rest: [u8; 120],
}

#[cfg(all(windows, target_arch = "x86"))]
fn connect_p2p(this: *mut std::ffi::c_void, remote: u64) -> u32 {
    let func = slot(this, 3);
    if func == 0 || remote == 0 {
        return 0;
    }
    let identity = Identity { kind: 16, size: 8, steam_id: remote, rest: [0; 120] };
    unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, *const Identity, i32, i32, *const u8) -> u32 = std::mem::transmute(func);
        call(this, &identity, 0, 0, std::ptr::null())
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn send_bytes(this: *mut std::ffi::c_void, conn: u32, bytes: &[u8], flags: i32) {
    let func = slot(this, 11);
    if func == 0 {
        return;
    }
    unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32, *const u8, u32, i32, *mut i64) -> i32 = std::mem::transmute(func);
        let _ = call(this, conn, bytes.as_ptr(), bytes.len() as u32, flags, std::ptr::null_mut());
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn pull(this: *mut std::ffi::c_void, poll: u32, incoming: &Mutex<Vec<Message>>) {
    let func = slot(this, 27);
    if func == 0 {
        return;
    }
    let mut slots = [std::ptr::null_mut::<u8>(); 16];
    let count = unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32, *mut *mut u8, i32) -> i32 = std::mem::transmute(func);
        call(this, poll, slots.as_mut_ptr(), slots.len() as i32)
    };
    if count <= 0 {
        return;
    }
    let mut decoded = Vec::new();
    for msg in slots.into_iter().take(count as usize) {
        if let Some(bytes) = take_message(msg) {
            if let Some(message) = wire::decode(&bytes) {
                decoded.push(message);
            }
        }
    }
    if let Ok(mut queue) = incoming.lock() {
        queue.extend(decoded);
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn take_message(msg: *mut u8) -> Option<Vec<u8>> {
    if msg.is_null() {
        return None;
    }
    unsafe {
        let data = *(msg as *const *const u8);
        let size = *(msg.add(4) as *const i32);
        let bytes = if data.is_null() || size <= 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(data, size as usize).to_vec()
        };
        let release = *(msg.add(180) as *const usize);
        if release != 0 {
            let free: unsafe extern "C" fn(*mut u8) = std::mem::transmute(release);
            free(msg);
        }
        if bytes.is_empty() { None } else { Some(bytes) }
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn ping_ms(this: *mut std::ffi::c_void, conn: u32) -> Option<u32> {
    let func = slot(this, 16);
    if func == 0 {
        return None;
    }
    let mut status = [0u8; 64];
    let result = unsafe {
        let call: unsafe extern "thiscall" fn(*mut std::ffi::c_void, u32, *mut u8, i32, *mut u8) -> i32 = std::mem::transmute(func);
        call(this, conn, status.as_mut_ptr(), 0, std::ptr::null_mut())
    };
    if result != 1 {
        return None;
    }
    let ping = i32::from_le_bytes(status[4..8].try_into().ok()?);
    if ping < 0 { None } else { Some(ping as u32) }
}
