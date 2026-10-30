//! Ban-safety guard: the app must never interact with the game process.
//! Fails if any forbidden Win32 identifier appears anywhere under src-tauri/src.
//! This file lives in tests/ (not src/) so its own list does not trip the check.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "SendInput",
    "keybd_event",
    "mouse_event",
    "SetWindowsHookEx",
    "OpenProcess",
    "ReadProcessMemory",
    "WriteProcessMemory",
    "FindWindow",
    "EnumWindows",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_forbidden_game_interaction_apis() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty(), "no source files found under {src:?}");

    let mut hits = Vec::new();
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for word in FORBIDDEN {
            if text.contains(word) {
                hits.push(format!("{}: {}", file.display(), word));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "forbidden identifiers found:\n{}",
        hits.join("\n")
    );
}
