//! フォントのファイルの大きさ、読み込んだ後のメモリー、グリフのキャッシュのメモリーを、分けて測る。
//!
//! 使い方(release で測る):
//!
//! ```text
//! cargo run --release -p seinas-text --example memory [フォントの置き場]
//! ```
//!
//! メモリーは、このプロセスの `/proc/self/status` の VmRSS と VmData の増え方で見る。
//! 1. 何もしていないとき
//! 2. フォントを読み込んだ後(`TextPainter::load`)
//! 3. 短いタイトルを 1 つ描いた後(ふだんの使い方)
//! 4. 漢字を 3,000 字(U+4E00 から順に)描いた後(グリフのキャッシュが増えた状態)

use std::path::PathBuf;

use seinas_text::{Canvas, FontFiles, Line, TextPainter, DEFAULT_SIZE};

/// VmRSS と VmData(kB)。
fn memory() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").expect("read /proc/self/status");
    let field = |name: &str| {
        status
            .lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    };
    (field("VmRSS:"), field("VmData:"))
}

fn report(label: &str, now: (u64, u64), before: (u64, u64)) {
    println!(
        "{label}: VmRSS {} kB (+{} kB), VmData {} kB (+{} kB)",
        now.0,
        now.0 as i64 - before.0 as i64,
        now.1,
        now.1 as i64 - before.1 as i64
    );
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "target/fonts".into());
    let files = FontFiles::in_dir(&dir);
    for path in [&files.ui, &files.fallback] {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        println!("file {}: {size} bytes", path.display());
    }

    let start = memory();
    report("start", start, start);
    let (mut painter, problems) = TextPainter::load(&files);
    assert!(problems.is_empty(), "{problems:?}");
    let loaded = memory();
    report("after loading the fonts", loaded, start);

    let (width, height) = (1200usize, 24usize);
    let mut pixels = vec![0u8; width * height * 4];
    let line = Line {
        size: DEFAULT_SIZE,
        color: [0xff, 0xff, 0xff],
        x: 0,
        top: 0,
        height: height as i32,
        max_width: width as i32,
    };
    painter.draw_line(
        &mut Canvas::new(&mut pixels, width, height),
        "zeyes ウィンドウのタイトル",
        line,
    );
    let title = memory();
    report("after drawing one title", title, loaded);

    // 40 字ずつ、3,000 字を描く(キャンバスは使い回す)。
    let kanji: Vec<char> = (0x4e00u32..0x4e00 + 3000)
        .filter_map(char::from_u32)
        .collect();
    for chunk in kanji.chunks(40) {
        let text: String = chunk.iter().collect();
        painter.draw_line(&mut Canvas::new(&mut pixels, width, height), &text, line);
    }
    let cached = memory();
    report("after drawing 3000 kanji", cached, title);
    println!(
        "glyph cache: about {} bytes per glyph",
        (cached.0.saturating_sub(title.0)) * 1024 / kanji.len() as u64
    );
}
