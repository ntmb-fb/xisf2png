//! Single-instance handoff. Windows Explorer starts one process per selected
//! file when a right-click verb is used; file managers on Linux may do the
//! same. The first process to bind becomes the window; every later one sends
//! its paths to it and exits, so a multi-selection ends up as one file list in
//! one window.
//!
//! * Unix: per-user Unix domain socket inside a `0700` runtime/cache directory
//!   (no exposed TCP port, isolated per user, raw `OsStr` bytes preserved).
//! * Windows: localhost TCP on a fixed port for atomic Explorer multi-select
//!   election, with strict ACK verification and bounded payload reads.
//!
//! Protocol: `HEADER` (`xisf2png-open 2\n`) followed by NUL-delimited absolute
//! paths. The primary acknowledges with a single `k` byte.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const HEADER: &[u8] = b"xisf2png-open 2\n";
const ACK: u8 = b'k';
const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

pub enum Claim {
    /// This process is the primary window. `Some(listener)` if listening for
    /// handoffs, or `None` when running standalone without IPC.
    Primary(Option<Listener>),
    /// Paths were forwarded to an existing window; caller should exit.
    Forwarded,
}

fn encode_handoff(paths: &[PathBuf]) -> Vec<u8> {
    let mut buf = Vec::from(HEADER);
    for p in paths {
        let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        let bytes = path_to_bytes(&abs);
        if bytes.is_empty() || bytes.contains(&0) {
            continue;
        }
        buf.extend_from_slice(bytes);
        buf.push(0);
    }
    buf
}

fn decode_handoff(reader: &mut impl Read) -> Option<Vec<PathBuf>> {
    let mut buf = Vec::new();
    reader.take(MAX_PAYLOAD_BYTES).read_to_end(&mut buf).ok()?;
    let rest = buf.strip_prefix(HEADER)?;
    let paths = rest
        .split(|&b| b == 0)
        .filter(|slice| !slice.is_empty())
        .filter_map(bytes_to_path)
        .collect();
    Some(paths)
}

#[cfg(unix)]
fn path_to_bytes(p: &Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes()
}

#[cfg(unix)]
fn bytes_to_path(bytes: &[u8]) -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    Some(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn path_to_bytes(p: &Path) -> &[u8] {
    p.to_str().map_or(&[], str::as_bytes)
}

#[cfg(not(unix))]
fn bytes_to_path(bytes: &[u8]) -> Option<PathBuf> {
    std::str::from_utf8(bytes).ok().map(PathBuf::from)
}

// ---------------------------------------------------------------------------
// Unix: per-user Unix domain socket
// ---------------------------------------------------------------------------
#[cfg(unix)]
mod imp {
    use super::{decode_handoff, encode_handoff, Claim, ACK, IO_TIMEOUT};
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::DirBuilderExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use eframe::egui;

    pub struct Listener {
        inner: UnixListener,
        path: PathBuf,
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    fn socket_path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| {
                let home = std::env::var_os("HOME").map(PathBuf::from)?;
                if !home.is_absolute() {
                    return None;
                }
                #[cfg(target_os = "macos")]
                let dir = home.join("Library/Caches");
                #[cfg(not(target_os = "macos"))]
                let dir = home.join(".cache");
                Some(dir)
            })?;
        let dir = base.join("xisf2png");
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(&dir).ok()?;
        Some(dir.join("instance.sock"))
    }

    pub fn claim(paths: &[PathBuf]) -> Claim {
        let Some(sock) = socket_path() else {
            return Claim::Primary(None);
        };
        claim_at(&sock, paths)
    }

    pub(super) fn claim_at(sock: &Path, paths: &[PathBuf]) -> Claim {
        match UnixListener::bind(sock) {
            Ok(inner) => Claim::Primary(Some(Listener {
                inner,
                path: sock.to_path_buf(),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if forward_to(sock, paths) {
                    return Claim::Forwarded;
                }
                // Stale socket left behind after a crash: remove and retry once.
                let _ = std::fs::remove_file(sock);
                match UnixListener::bind(sock) {
                    Ok(inner) => Claim::Primary(Some(Listener {
                        inner,
                        path: sock.to_path_buf(),
                    })),
                    Err(_) if forward_to(sock, paths) => Claim::Forwarded,
                    Err(_) => Claim::Primary(None),
                }
            }
            Err(_) => Claim::Primary(None),
        }
    }

    pub(super) fn forward_stream(stream: &mut UnixStream, msg: &[u8]) -> bool {
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        if stream.write_all(msg).is_ok() {
            let _ = stream.shutdown(Shutdown::Write);
            let mut ack = [0u8; 1];
            if matches!(stream.read(&mut ack), Ok(1)) && ack[0] == ACK {
                return true;
            }
        }
        false
    }

    pub(super) fn forward_to(sock: &Path, paths: &[PathBuf]) -> bool {
        let msg = encode_handoff(paths);
        for attempt in 0..5 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(150));
            }
            let Ok(mut stream) = UnixStream::connect(sock) else {
                continue;
            };
            if forward_stream(&mut stream, &msg) {
                return true;
            }
        }
        false
    }

    pub fn serve(listener: Listener, tx: Sender<Vec<PathBuf>>, ctx: egui::Context) {
        std::thread::spawn(move || {
            for mut stream in listener.inner.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let Some(paths) = decode_handoff(&mut stream) else {
                    continue;
                };
                let _ = stream.write_all(&[ACK]);
                if !paths.is_empty() && tx.send(paths).is_err() {
                    break;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            }
            drop(listener);
        });
    }
}

// ---------------------------------------------------------------------------
// Windows: localhost TCP (atomic for Explorer multi-select storms)
// ---------------------------------------------------------------------------
#[cfg(not(unix))]
mod imp {
    use super::{decode_handoff, encode_handoff, Claim, ACK, IO_TIMEOUT};
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use eframe::egui;

    const PORT: u16 = 47811;

    pub type Listener = TcpListener;

    pub fn claim(paths: &[PathBuf]) -> Claim {
        match TcpListener::bind(("127.0.0.1", PORT)) {
            Ok(listener) => Claim::Primary(Some(listener)),
            Err(_) => {
                if forward(paths) {
                    Claim::Forwarded
                } else {
                    // Port taken by something that is not us: run standalone.
                    Claim::Primary(None)
                }
            }
        }
    }

    fn forward(paths: &[PathBuf]) -> bool {
        let msg = encode_handoff(paths);
        for attempt in 0..5 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(150));
            }
            let Ok(mut stream) = TcpStream::connect_timeout(
                &([127, 0, 0, 1], PORT).into(),
                Duration::from_millis(500),
            ) else {
                continue;
            };
            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            if stream.write_all(&msg).is_ok() {
                let _ = stream.shutdown(Shutdown::Write);
                let mut ack = [0u8; 1];
                if matches!(stream.read(&mut ack), Ok(1)) && ack[0] == ACK {
                    return true;
                }
            }
        }
        false
    }

    pub fn serve(listener: Listener, tx: Sender<Vec<PathBuf>>, ctx: egui::Context) {
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                let Some(paths) = decode_handoff(&mut stream) else {
                    continue;
                };
                let _ = stream.write_all(&[ACK]);
                if !paths.is_empty() && tx.send(paths).is_err() {
                    break;
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            }
        });
    }
}

pub use imp::{claim, serve, Listener};

/// Keep only paths from the command line that actually exist.
pub fn paths_from_args() -> Vec<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_handoff_preserves_newlines_and_empty() {
        let paths = vec![
            PathBuf::from("/tmp/M31.xisf"),
            PathBuf::from("/tmp/line1\nline2.fits"),
        ];
        let encoded = encode_handoff(&paths);
        let decoded = decode_handoff(&mut Cursor::new(encoded)).unwrap();
        assert_eq!(decoded, paths);

        let empty = encode_handoff(&[]);
        let decoded_empty = decode_handoff(&mut Cursor::new(empty)).unwrap();
        assert!(decoded_empty.is_empty());
    }

    #[test]
    fn rejects_wrong_header() {
        let mut bad = Cursor::new(b"GET / HTTP/1.1\r\n\r\n");
        assert!(decode_handoff(&mut bad).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unix_forward_requires_valid_ack() {
        use std::io::Write;
        use std::os::unix::net::UnixStream;

        let msg = encode_handoff(&[PathBuf::from("/tmp/a.xisf")]);

        // Case 1: peer sends a bogus ACK (simulates unrelated service).
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = server.read_to_end(&mut sink);
            let _ = server.write_all(b"X");
        });
        assert!(
            !imp::forward_stream(&mut client, &msg),
            "forward must fail when peer does not return expected ACK"
        );
        handle.join().unwrap();

        // Case 2: peer decodes handoff and returns expected ACK.
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = std::thread::spawn(move || {
            let decoded = decode_handoff(&mut server);
            let _ = server.write_all(&[ACK]);
            decoded
        });
        assert!(
            imp::forward_stream(&mut client, &msg),
            "forward must succeed when peer returns expected ACK"
        );
        let decoded = handle.join().unwrap().unwrap();
        assert_eq!(decoded, vec![PathBuf::from("/tmp/a.xisf")]);
    }

    #[cfg(unix)]
    #[test]
    fn unix_claim_reclaims_stale_socket() {
        use std::os::unix::net::UnixListener;

        let sock =
            std::env::temp_dir().join(format!("xisf2png-stale-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock);
        // Bind and drop without removing the socket file to simulate a crash.
        let listener = UnixListener::bind(&sock).unwrap();
        drop(listener);
        assert!(sock.exists());

        match imp::claim_at(&sock, &[]) {
            Claim::Primary(Some(claimed)) => drop(claimed),
            _ => panic!("expected stale socket to be reclaimed"),
        }
        assert!(!sock.exists(), "dropping Listener should clean up socket");
    }
}
