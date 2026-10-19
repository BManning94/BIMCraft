//! Running a program an update needs (the new build to try, hdiutil, ditto, codesign, open): with no input, its output kept to a few
//! KiB for a message, and never for longer than a limit, because a program that hangs must not hang an update.

use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How much of what a program prints is kept: it's for a message, not for use.
const KEEP: usize = 16 << 10;

/// How long to wait for a program's output once it has gone: a child it left behind may hold the pipe open for good.
const LINGER: Duration = Duration::from_secs(2);

/// Runs `command` to the end, or kills it at `limit`. What it printed when it exits with success; an error that says why, and what
/// it printed on stderr, when it doesn't.
pub fn run(command: &mut Command, limit: Duration) -> Result<String, String> {
    let program = command.get_program().to_owned();
    let name = Path::new(&program).file_name().unwrap_or(&program).to_string_lossy().into_owned();
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = spawn(command).map_err(|e| format!("{}: {e}", program.to_string_lossy()))?;
    let (out, err) = (Tail::of(child.stdout.take()), Tail::of(child.stderr.take()));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < limit => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} took longer than {limit:?}"));
            }
            Err(e) => return Err(format!("{name}: {e}")),
        }
    };
    let (stdout, stderr) = (out.text(), err.text());
    if status.success() {
        return Ok(stdout);
    }
    let how = status.code().map_or("was stopped by a signal".to_string(), |code| format!("failed with exit code {code}"));
    Err(format!("{name} {how}: {}", stderr.trim()))
}

/// Whether `said`, what a new build printed for `--version`, is its name and `version`; if not, an error that shows what it said.
pub fn says_version(said: &str, version: &str) -> Result<(), String> {
    if said.trim() == crate::version_line(version) {
        return Ok(());
    }
    Err(format!("the new build says `{}`, not {version}", said.trim().chars().take(80).collect::<String>()))
}

/// Starts `command`. A program written a moment ago can be "busy": a process forked meanwhile holds the file open for writing until
/// it execs, which takes a moment, so that is waited out a few times.
fn spawn(command: &mut Command) -> io::Result<Child> {
    for _ in 0..4 {
        match command.spawn() {
            Err(e) if e.kind() == io::ErrorKind::ExecutableFileBusy => thread::sleep(Duration::from_millis(100)),
            other => return other,
        }
    }
    command.spawn()
}

/// The start of what a pipe carries, read on a thread so the program is never stuck on a full pipe.
struct Tail {
    kept: Arc<Mutex<Vec<u8>>>,
    ended: mpsc::Receiver<()>,
}

impl Tail {
    fn of(source: Option<impl Read + Send + 'static>) -> Tail {
        let kept = Arc::new(Mutex::new(Vec::new()));
        let (sender, ended) = mpsc::channel();
        if let Some(mut source) = source {
            let kept = Arc::clone(&kept);
            thread::spawn(move || {
                let mut buf = [0; 4096];
                while let Ok(n @ 1..) = source.read(&mut buf) {
                    if let Ok(mut kept) = kept.lock() {
                        let room = KEEP.saturating_sub(kept.len()).min(n);
                        kept.extend_from_slice(buf.get(..room).unwrap_or_default());
                    }
                }
                let _ = sender.send(());
            });
        }
        Tail { kept, ended }
    }

    /// What was kept, once the pipe has closed, or a moment after the program has gone if something still holds it open.
    fn text(self) -> String {
        let _ = self.ended.recv_timeout(LINGER);
        let kept = self.kept.lock().map(|kept| kept.clone()).unwrap_or_default();
        String::from_utf8_lossy(&kept).into_owned()
    }
}
