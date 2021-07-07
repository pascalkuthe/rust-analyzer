//! rust-analyzer relies heavily on source code generation.
//!
//! Things like feature documentation or assist tests are implemented by
//! processing rust-analyzer's own source code and generating the appropriate
//! output. See `sourcegen_` tests in various crates.
//!
//! This crate contains utilities to make this kind of source-gen easy.

use std::{
    fmt, fs, mem,
    path::{Path, PathBuf},
};

use xshell::{cmd, pushenv};

pub fn list_rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut res = list_files(dir);
    res.retain(|it| {
        it.file_name().unwrap_or_default().to_str().unwrap_or_default().ends_with(".rs")
    });
    res
}

pub fn list_files(dir: &Path) -> Vec<PathBuf> {
    let mut res = Vec::new();
    let mut work = vec![dir.to_path_buf()];
    while let Some(dir) = work.pop() {
        for entry in dir.read_dir().unwrap() {
            let entry = entry.unwrap();
            let file_type = entry.file_type().unwrap();
            let path = entry.path();
            let is_hidden =
                path.file_name().unwrap_or_default().to_str().unwrap_or_default().starts_with('.');
            if !is_hidden {
                if file_type.is_dir() {
                    work.push(path)
                } else if file_type.is_file() {
                    res.push(path)
                }
            }
        }
    }
    res
}

pub struct CommentBlock {
    pub id: String,
    pub line: usize,
    pub contents: Vec<String>,
}

impl CommentBlock {
    pub fn extract(tag: &str, text: &str) -> Vec<CommentBlock> {
        assert!(tag.starts_with(char::is_uppercase));

        let tag = format!("{}:", tag);
        let mut res = Vec::new();
        for (line, mut block) in do_extract_comment_blocks(text, true) {
            let first = block.remove(0);
            if let Some(id) = first.strip_prefix(&tag) {
                let id = id.trim().to_string();
                let block = CommentBlock { id, line, contents: block };
                res.push(block);
            }
        }
        res
    }

    pub fn extract_untagged(text: &str) -> Vec<CommentBlock> {
        let mut res = Vec::new();
        for (line, block) in do_extract_comment_blocks(text, false) {
            let id = String::new();
            let block = CommentBlock { id, line, contents: block };
            res.push(block);
        }
        res
    }
}

fn do_extract_comment_blocks(
    text: &str,
    allow_blocks_with_empty_lines: bool,
) -> Vec<(usize, Vec<String>)> {
    let mut res = Vec::new();

    let prefix = "// ";
    let lines = text.lines().map(str::trim_start);

    let mut block = (0, vec![]);
    for (line_num, line) in lines.enumerate() {
        if line == "//" && allow_blocks_with_empty_lines {
            block.1.push(String::new());
            continue;
        }

        let is_comment = line.starts_with(prefix);
        if is_comment {
            block.1.push(line[prefix.len()..].to_string());
        } else {
            if !block.1.is_empty() {
                res.push(mem::take(&mut block));
            }
            block.0 = line_num + 2;
        }
    }
    if !block.1.is_empty() {
        res.push(block)
    }
    res
}

#[derive(Debug)]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.file.strip_prefix(&project_root()).unwrap().display().to_string();
        let path = path.replace('\\', "/");
        let name = self.file.file_name().unwrap();
        write!(
            f,
            "https://github.com/rust-analyzer/rust-analyzer/blob/master/{}#L{}[{}]",
            path,
            self.line,
            name.to_str().unwrap()
        )
    }
}

fn ensure_rustfmt() {
    let version = cmd!("rustfmt --version").read().unwrap_or_default();
    if !version.contains("stable") {
        panic!(
            "Failed to run rustfmt from toolchain 'stable'. \
                 Please run `rustup component add rustfmt --toolchain stable` to install it.",
        )
    }
}

pub fn reformat(text: String) -> String {
    let _e = pushenv("RUSTUP_TOOLCHAIN", "stable");
    ensure_rustfmt();
    let rustfmt_toml = project_root().join("rustfmt.toml");
    let mut stdout = cmd!("rustfmt --config-path {rustfmt_toml} --config fn_single_line=true")
        .stdin(text)
        .read()
        .unwrap();
    if !stdout.ends_with('\n') {
        stdout.push('\n');
    }
    stdout
}

pub fn add_preamble(generator: &'static str, mut text: String) -> String {
    let preamble = format!("//! Generated by `{}`, do not edit by hand.\n\n", generator);
    text.insert_str(0, &preamble);
    text
}

/// Checks that the `file` has the specified `contents`. If that is not the
/// case, updates the file and then fails the test.
pub fn ensure_file_contents(file: &Path, contents: &str) {
    if let Ok(old_contents) = fs::read_to_string(file) {
        if normalize_newlines(&old_contents) == normalize_newlines(contents) {
            // File is already up to date.
            return;
        }
    }

    let display_path = file.strip_prefix(&project_root()).unwrap_or(file);
    eprintln!(
        "\n\x1b[31;1merror\x1b[0m: {} was not up-to-date, updating\n",
        display_path.display()
    );
    if std::env::var("CI").is_ok() {
        eprintln!("    NOTE: run `cargo test` locally and commit the updated files\n");
    }
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(file, contents).unwrap();
    panic!("some file was not up to date and has been updated, simply re-run the tests")
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n")
}

pub fn project_root() -> PathBuf {
    let dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(dir).parent().unwrap().parent().unwrap().to_owned()
}