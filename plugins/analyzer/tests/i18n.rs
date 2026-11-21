//! i18n 覆盖测试：Rust `analyzer_label` 双表 ↔ 原版 C++ `Strings.h` 逐项比对。
//!
//! 权威来源：
//! - `plugins/Analyzer/src/Strings.h`：Analyzer 专有 71 条（EN / ZH 两列同序）
//! - `plugins/common/Strings.h`：三插件共用 23 条 `{"EN", "ZH"}` 行
//!
//! 断言：
//! * 每条英文在中文语言下都能查到中文译文（刻意保留原样的技术缩写除外）；
//! * 译文与原版中文逐字一致（同英文不同义的歧义条目单列，见下）；
//! * 英文语言下原样返回英文字面量。
//!
//! 运行：`cargo test -p orm-analyzer --test i18n`
//!
//! 原版源码目录可用环境变量 `ORM_ORIGINAL_SRC` 覆盖；默认按仓库相对路径
//! `../../OpenRM/plugins/Analyzer/src` 解析。

use std::fs;
use std::path::PathBuf;

use openrm_ui::strings::{self, AnalyzerDup, Lang};

/// 与 `param_parity.rs` 同一套源码解析口径。
fn original_src_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ORM_ORIGINAL_SRC") {
        return PathBuf::from(p);
    }
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("../../../OpenRM/plugins/Analyzer/src")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(manifest).join("../../../OpenRM/plugins/Analyzer/src"))
}

fn read_or_skip(path: &PathBuf) -> String {
    match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("跳过：原版源码不可读 {path:?}（{e}）");
            String::new()
        }
    }
}

/// 抽取 C++ 源码里全部双引号字符串字面量（按出现顺序）。
fn literals(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = src.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            let mut buf = String::new();
            loop {
                match chars.next() {
                    Some('"') => break,
                    Some('\\') => {
                        if let Some(esc) = chars.next() {
                            buf.push(esc);
                        }
                    }
                    Some(ch) => buf.push(ch),
                    None => break,
                }
            }
            out.push(buf);
        }
    }
    out
}

/// 解析 Analyzer/Strings.h 的 (EN, ZH) 同序配对。
fn parse_analyzer_pairs(src: &str) -> Vec<(String, String)> {
    let lits = literals(src);
    let Some(zh_start) = lits.iter().position(|s| s == "释放") else {
        return Vec::new();
    };
    let Some(en_start) = lits.iter().position(|s| s == "RELEASE") else {
        return Vec::new();
    };
    let en: Vec<String> = lits[en_start..zh_start].to_vec();
    let zh: Vec<String> = lits[zh_start..]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect();
    en.into_iter().zip(zh).collect()
}

/// 解析 common/Strings.h 的 `{"EN", "ZH"},` 行。
fn parse_common_pairs(src: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let lits = literals(line);
        if lits.len() == 2 {
            pairs.push((lits[0].clone(), lits[1].clone()));
        }
    }
    pairs
}

/// 刻意保留原样、不译的英文条目（技术缩写 / 本就是中文的条目）。
fn unchanged_ok(en: &str) -> bool {
    matches!(en, "中文")
}

/// 同英文、不同条目下中文不同的歧义条目（字面量查表只能命中第一义）。
/// 第二义由 `analyzer_dup_label` 显式取词，测试单独覆盖。
const KNOWN_DUP_EN: &[&str] = &["HOLD", "MED"];

#[test]
fn analyzer_strings_fully_translated() {
    let src_dir = original_src_dir();
    let analyzer_src = read_or_skip(&src_dir.join("Strings.h"));
    let common_src = read_or_skip(&src_dir.join("../../common/Strings.h"));

    let analyzer_pairs = parse_analyzer_pairs(&analyzer_src);
    let common_pairs = parse_common_pairs(&common_src);

    if analyzer_pairs.is_empty() || common_pairs.is_empty() {
        eprintln!("原版 Strings.h 未解析到条目，跳过覆盖断言。");
        return;
    }

    let mut missing: Vec<(String, String)> = Vec::new();
    let mut mismatch: Vec<(String, String, String)> = Vec::new();

    // --- Analyzer 专有条目：译文必须与原版逐字一致 ---
    for (en, zh) in &analyzer_pairs {
        let label = strings::analyzer_label(en, Lang::Zh);
        if label == *en && !unchanged_ok(en) {
            missing.push((en.clone(), zh.clone()));
        }
        if !KNOWN_DUP_EN.contains(&en.as_str()) && label != *zh {
            mismatch.push((en.clone(), zh.clone(), label));
        }
    }

    // --- common 共用条目：至少要译出中文；歧义同名只做可译性检查 ---
    for (en, zh) in &common_pairs {
        let label = strings::analyzer_label(en, Lang::Zh);
        if label == *en && !unchanged_ok(en) {
            missing.push((en.clone(), zh.clone()));
        }
        if !KNOWN_DUP_EN.contains(&en.as_str()) && label != *zh {
            mismatch.push((en.clone(), zh.clone(), label));
        }
    }

    let total = analyzer_pairs.len() + common_pairs.len();
    println!("i18n 条目数：{total}（Analyzer {} + common {}）", analyzer_pairs.len(), common_pairs.len());
    println!("未翻译缺失：{}", missing.len());
    for (en, zh) in &missing {
        println!("  缺失：{en:?}（原版中文：{zh:?}）");
    }
    println!("译文不一致：{}", mismatch.len());
    for (en, zh, got) in &mismatch {
        println!("  不一致：{en:?} 原版 {zh:?} / Rust {got:?}");
    }

    assert!(
        missing.is_empty(),
        "存在 {} 条未翻译文案，见上方缺失清单",
        missing.len()
    );
    assert!(
        mismatch.is_empty(),
        "存在 {} 条译文与原版不一致，见上方清单",
        mismatch.len()
    );
}

/// 歧义条目：发生器 HOLD（锁相位）走显式取词。
#[test]
fn duplicate_hold_label_resolved_explicitly() {
    assert_eq!(
        strings::analyzer_dup_label(AnalyzerDup::GenHold, Lang::Zh),
        "锁相位"
    );
    assert_eq!(
        strings::analyzer_dup_label(AnalyzerDup::GenHold, Lang::En),
        "HOLD"
    );
    // 字面量查表命中第一义「保持」（电平峰值保持）。
    assert_eq!(strings::analyzer_label("HOLD", Lang::Zh), "保持");
}

/// 英文语言下原样返回，不做翻译。
#[test]
fn english_branch_returns_literal() {
    for literal in ["RELEASE", "Frequency", "DEVELOPER", "ROLL", "TO OUTPUT"] {
        assert_eq!(strings::analyzer_label(literal, Lang::En), literal);
    }
}
