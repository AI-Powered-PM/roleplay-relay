//! File logging. Never log the API key, request/response bodies, or chat text
//! (chat text only when `debugLogChat` is on; the call sites check it).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1_000_000;
const MAX_FILES: usize = 5;

/// `app.log` is the live file. When it would pass `max_bytes` it becomes `app.1.log`, the old
/// `app.1.log` becomes `app.2.log`, and so on; the file that would be number `max_files` is
/// deleted. Total on disk: at most `max_files` files of `max_bytes` each.
pub struct RotatingFile {
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
    /// `None` only while rotating.
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    pub fn new(dir: &Path, max_bytes: u64, max_files: usize) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = Self::open_live(dir)?;
        let size = file.metadata()?.len();
        Ok(Self { dir: dir.into(), max_bytes, max_files, file: Some(file), size })
    }

    fn open_live(dir: &Path) -> io::Result<File> {
        OpenOptions::new().create(true).append(true).open(dir.join("app.log"))
    }

    fn numbered(&self, n: usize) -> PathBuf {
        self.dir.join(format!("app.{n}.log"))
    }

    fn rotate(&mut self) -> io::Result<()> {
        let _ = fs::remove_file(self.numbered(self.max_files - 1));
        for n in (1..self.max_files - 1).rev() {
            let from = self.numbered(n);
            if from.exists() {
                let _ = fs::rename(&from, self.numbered(n + 1));
            }
        }
        // Windows will not rename an open file: close the live one first.
        self.file = None;
        let _ = fs::rename(self.dir.join("app.log"), self.numbered(1));
        self.file = Some(Self::open_live(&self.dir)?);
        self.size = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.size > 0 && self.size + buf.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let file = match self.file.as_mut() {
            Some(f) => f,
            None => return Err(io::Error::other("log file closed")),
        };
        let n = file.write(buf)?;
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), |f| f.flush())
    }
}

/// Synchronous writes: log lines are tiny and rare, and a non-blocking writer only flushes on a
/// clean exit, which loses lines when the app is killed.
pub fn init(logs_dir: &Path) -> Result<(), String> {
    let writer = RotatingFile::new(logs_dir, MAX_BYTES, MAX_FILES)
        .map_err(|e| format!("log file: {e}"))?;
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(writer))
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .map_err(|e| format!("subscriber: {e}"))
}

/// With `panic = "abort"` a panic kills the app without a trace. Log message and location (the
/// panic text comes from our code or Tauri, never from chat text or keys), then run the default hook.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-text panic payload".to_owned());
        let location = info
            .location()
            .map_or_else(|| "unknown location".to_owned(), |l| format!("{}:{}", l.file(), l.line()));
        tracing::error!("panic at {location}: {message}");
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn lines_reach_the_file_immediately() {
        let dir = temp("log");
        let mut w = RotatingFile::new(&dir, MAX_BYTES, MAX_FILES).unwrap();
        w.write_all(b"probe line\n").unwrap();
        let text = fs::read_to_string(dir.join("app.log")).unwrap();
        assert!(text.contains("probe line"), "log was: {text:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotates_by_size_and_keeps_five_files() {
        let dir = temp("rot");
        let mut w = RotatingFile::new(&dir, 100, 5).unwrap();
        for i in 0..40 {
            w.write_all(format!("line {i:03} {}\n", "x".repeat(40)).as_bytes()).unwrap();
        }
        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["app.1.log", "app.2.log", "app.3.log", "app.4.log", "app.log"]);
        for n in &names {
            assert!(fs::metadata(dir.join(n)).unwrap().len() <= 100, "{n} too big");
        }
        // Newest line is in the live file.
        assert!(fs::read_to_string(dir.join("app.log")).unwrap().contains("line 039"));
        let _ = fs::remove_dir_all(&dir);
    }
}
