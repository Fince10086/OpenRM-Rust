//! 参数层 parity 测试：Rust 表 ↔ 原版 C++ 源码逐项比对。
//!
//! 权威来源（同级仓库 `OpenRM/plugins/Analyzer/src`）：
//! - `Params.h`：枚举 id 顺序、档位常量数组
//! - `Analyzer.cpp`：`GetParam(kX)->InitInt/InitBool` 的宿主名、默认值、档位范围
//! - `Analyzer.h`：`kRangeDb` 与各 `Current*()` 换算口径
//!
//! 运行：`cargo test -p orm-analyzer --test param_parity`
//!
//! 原版源码目录可用环境变量 `ORM_ORIGINAL_SRC` 覆盖；默认按仓库相对路径
//! `../../OpenRM/plugins/Analyzer/src` 解析。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use truce::prelude::Params;

use orm_analyzer::params::{
    write_state_file, AnalyzerParams, HOLD_SECS, LOUD_SCALE_LU, LOUD_TARGETS_LUFS,
    PBT_LF_RES_OPTIONS, RANGE_DB, RES_OPTIONS, RTA_BAND_COUNTS, RTA_BPO_OPTIONS,
    SLOPE_BAND_DB, SLOPE_FFT_DB, SPEED_RELEASE_LIN, SPEED_RELEASE_LOG, VQT_GAMMA_OPTIONS,
};

// ---------------------------------------------------------------------------
// 原版源码解析
// ---------------------------------------------------------------------------

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

/// 符号常量解析表（来自 Params.h / Analyzer.cpp 的枚举与 kNum* 计数）。
fn symbols() -> BTreeMap<&'static str, f64> {
    BTreeMap::from([
        ("kSpeedMIN", 0.0),
        ("kSpeedSLOW", 1.0),
        ("kSpeedMED", 2.0),
        ("kSpeedFAST", 3.0),
        ("kSpeedMAX", 4.0),
        ("kFFTWindowHann", 0.0),
        ("kFFTWindowClean", 1.0),
        ("kModeFFT", 0.0),
        ("kModeVQT", 1.0),
        ("kModePBT", 2.0),
        ("kModeRTA", 3.0),
        ("kChanModePWR", 0.0),
        ("kChanModeLR", 1.0),
        ("kChanModeSUM", 2.0),
        ("kLevelModeDBTP", 0.0),
        ("kLevelModeDBFS", 1.0),
        ("kNumSpeedOptions", 5.0),
        ("kNumResOptions", 3.0),
        ("kNumPbtLfResOptions", 3.0),
        ("kNumModes", 4.0),
        ("kNumChanModes", 3.0),
        ("kNumLevelModes", 2.0),
        ("kNumHoldTimeOptions", 3.0),
        ("kNumVQTGammaOptions", 3.0),
        ("kNumSlopeOptions", 3.0),
        ("kNumFFTWindows", 2.0),
        ("kNumRtaOctaveOptions", 3.0),
        ("kNumLoudPresets", 4.0),
        ("kNumLoudScaleOptions", 2.0),
    ])
}

/// 解析 `enum EParams { kSpeed, ..., kNumParams };` 的有序标识名（剔除 kNumParams）。
fn parse_enum_order(src: &str) -> Vec<String> {
    let block = src
        .split("enum EParams")
        .nth(1)
        .and_then(|s| s.split(';').next())
        .expect("EParams block");
    block
        .lines()
        .flat_map(|line| line.split(','))
        .map(|t| t.trim())
        .filter_map(|t| t.split(|c: char| c == '/' || c == '=').next()?.trim().strip_prefix('k'))
        .filter(|name| !name.is_empty() && *name != "NumParams")
        .map(|s| format!("k{s}"))
        .collect()
}

/// 解析 `constexpr <T> kName[] = { a, b, c };` 数值数组。
fn parse_array(src: &str, name: &str) -> Vec<f64> {
    let needle = format!("{name}[");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("array {name} not found"));
    let lb = src[start..]
        .find('{')
        .expect("array open brace");
    let rb = src[start + lb..]
        .find('}')
        .expect("array close brace");
    let body = &src[start + lb + 1..start + lb + rb];
    body.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| {
            let t = t.trim_end_matches("f").trim_end_matches("F");
            t.parse::<f64>().unwrap_or_else(|_| panic!("bad number {t}"))
        })
        .collect()
}

struct InitEntry {
    /// 原版枚举标识（kSpeed 等）。
    param: String,
    /// 宿主参数名（InitInt 第一个字符串参数）。
    host_name: String,
    default: f64,
    min: f64,
    max: f64,
    is_bool: bool,
}

fn eval_expr(expr: &str, sym: &BTreeMap<&str, f64>) -> f64 {
    let expr = expr.trim();
    if let Some(v) = expr.parse::<f64>().ok() {
        return v;
    }
    if expr == "false" {
        return 0.0;
    }
    if expr == "true" {
        return 1.0;
    }
    if let Some((l, r)) = expr.split_once('-') {
        let l = sym.get(l.trim()).copied().unwrap_or_else(|| {
            panic!("unknown symbol {}", l.trim())
        });
        let r = r
            .trim()
            .parse::<f64>()
            .ok()
            .or_else(|| sym.get(r.trim()).copied())
            .unwrap_or_else(|| panic!("bad expr {expr}"));
        return l - r;
    }
    *sym.get(expr).unwrap_or_else(|| panic!("unknown symbol {expr}"))
}

fn parse_inits(src: &str) -> Vec<InitEntry> {
    let sym = symbols();
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("GetParam(") else {
            continue;
        };
        let param = rest
            .split(')')
            .next()
            .unwrap()
            .trim()
            .to_string();
        if let Some(inner) = line.split("InitInt(").nth(1) {
            let args: Vec<String> = inner
                .trim_end_matches(");")
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            // "Name", default, min, max, ""
            let host_name = args[0].trim_matches('"').to_string();
            let default = eval_expr(&args[1], &sym);
            let min = eval_expr(&args[2], &sym);
            let max = eval_expr(&args[3], &sym);
            out.push(InitEntry {
                param,
                host_name,
                default,
                min,
                max,
                is_bool: false,
            });
        } else if let Some(inner) = line.split("InitBool(").nth(1) {
            let args: Vec<String> = inner
                .trim_end_matches(");")
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            let host_name = args[0].trim_matches('"').to_string();
            let default = eval_expr(&args[1], &sym);
            out.push(InitEntry {
                param,
                host_name,
                default,
                min: 0.0,
                max: 1.0,
                is_bool: true,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[test]
fn param_ids_names_defaults_match_original() {
    let dir = original_src_dir();
    let params_h = fs::read_to_string(dir.join("Params.h")).expect("Params.h");
    let analyzer_cpp = fs::read_to_string(dir.join("Analyzer.cpp")).expect("Analyzer.cpp");

    let order = parse_enum_order(&params_h);
    let inits = parse_inits(&analyzer_cpp);

    assert_eq!(
        order.len(),
        inits.len(),
        "枚举数与 Init 表条目数不一致"
    );
    assert_eq!(order.len(), orm_analyzer::params::NUM_PARAMS);

    let p = AnalyzerParams::default();
    let infos = p.param_infos();
    assert_eq!(infos.len(), order.len(), "Rust 参数数量不符");

    // Init 表按 param 枚举名索引。
    let init_by_param: BTreeMap<&str, &InitEntry> =
        inits.iter().map(|e| (e.param.as_str(), e)).collect();

    for (i, enum_name) in order.iter().enumerate() {
        let info = &infos[i];
        let init = init_by_param
            .get(enum_name.as_str())
            .unwrap_or_else(|| panic!("{enum_name} 没有 Init 条目"));

        assert_eq!(info.id, i as u32, "id 顺序不符: {enum_name}");
        assert_eq!(
            info.name, init.host_name,
            "[{enum_name}] 宿主参数名不符"
        );

        let plain = p
            .get_plain(i as u32)
            .unwrap_or_else(|| panic!("{enum_name} get_plain 失败"));
        assert!(
            (plain - init.default).abs() < 1e-9,
            "[{enum_name}] 默认值不符: Rust {plain} / C++ {}",
            init.default
        );

        // 档位计数：Enum 范围 count；Bool 为 Discrete 0..1。
        match info.range {
            truce::prelude::ParamRange::Enum { count } => {
                let expected = (init.max - init.min + 1.0) as usize;
                assert_eq!(count, expected, "[{enum_name}] 档位数不符");
            }
            truce::prelude::ParamRange::Discrete { min, max } => {
                assert!(init.is_bool, "[{enum_name}] Discrete 范围应为 Bool");
                assert_eq!((min, max), (0, 1), "[{enum_name}] Bool 范围应为 0..1");
            }
            _ => {
                panic!("[{enum_name}] 未知参数范围类型");
            }
        }
    }
}

#[test]
fn option_tables_match_original_constants() {
    let dir = original_src_dir();
    let params_h = fs::read_to_string(dir.join("Params.h")).expect("Params.h");
    let analyzer_h = fs::read_to_string(dir.join("Analyzer.h")).expect("Analyzer.h");

    let check = |rust: &[f64], cpp: &[f64], label: &str| {
        assert_eq!(rust.len(), cpp.len(), "{label} 长度不符");
        for (a, b) in rust.iter().zip(cpp.iter()) {
            assert!((a - b).abs() < 1e-9, "{label} 数值不符: {a} vs {b}");
        }
    };

    check(
        &SPEED_RELEASE_LOG,
        &parse_array(&params_h, "kSpeedReleaseLog"),
        "SPEED_RELEASE_LOG",
    );
    check(
        &SPEED_RELEASE_LIN,
        &parse_array(&params_h, "kSpeedReleaseLin"),
        "SPEED_RELEASE_LIN",
    );
    check(
        &RES_OPTIONS.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &parse_array(&params_h, "kResOptions"),
        "RES_OPTIONS",
    );
    check(
        &PBT_LF_RES_OPTIONS.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &parse_array(&params_h, "kPbtLfResOptions"),
        "PBT_LF_RES_OPTIONS",
    );
    check(
        &RTA_BPO_OPTIONS.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &parse_array(&params_h, "kRtaOctaveOptions"),
        "RTA_BPO_OPTIONS",
    );
    check(
        &VQT_GAMMA_OPTIONS,
        &parse_array(&params_h, "kVQTGammaOptions"),
        "VQT_GAMMA_OPTIONS",
    );
    check(
        &HOLD_SECS,
        &parse_array(&params_h, "kHoldTimeSecs"),
        "HOLD_SECS",
    );
    check(
        &LOUD_TARGETS_LUFS,
        &parse_array(&params_h, "kLoudTargets"),
        "LOUD_TARGETS_LUFS",
    );
    check(
        &LOUD_SCALE_LU,
        &parse_array(&params_h, "kLoudScaleOffsets"),
        "LOUD_SCALE_LU",
    );
    check(
        &SLOPE_FFT_DB,
        &parse_array(&params_h, "kSlopeDbFFT"),
        "SLOPE_FFT_DB",
    );
    check(
        &SLOPE_BAND_DB,
        &parse_array(&params_h, "kSlopeDbLog"),
        "SLOPE_BAND_DB",
    );
    check(
        &RANGE_DB.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &parse_array(&analyzer_h, "kRangeDb"),
        "RANGE_DB",
    );

    // RTA 带数仅作信息展示，与 Strings.h 文案 59/118/236 对齐。
    assert_eq!(RTA_BAND_COUNTS, [59, 118, 236]);
}

#[test]
fn normalized_mapping_is_index_over_n_minus_one() {
    let p = AnalyzerParams::default();
    let infos = p.param_infos();

    for (i, info) in infos.iter().enumerate() {
        let truce::prelude::ParamRange::Enum { count } = info.range else {
            continue; // Bool 不参与枚举归一化
        };
        for idx in 0..count {
            p.set_plain(i as u32, idx as f64);
            let plain = p.get_plain(i as u32).unwrap();
            assert_eq!(plain, idx as f64, "id {i} plain 往返失败");
            let norm = p.get_normalized(i as u32).unwrap();
            let expected = if count <= 1 {
                0.0
            } else {
                idx as f64 / (count - 1) as f64
            };
            assert!(
                (norm - expected).abs() < 1e-9,
                "id {i} 档 {idx} 归一化不符: {norm} / {expected}"
            );
        }
    }
}

#[test]
fn state_file_roundtrip_and_validation() {
    let dir = original_src_dir();
    let params_h = fs::read_to_string(dir.join("Params.h")).expect("Params.h");
    let order = parse_enum_order(&params_h);

    let p = AnalyzerParams::default();
    // 拨动几个非默认档，确认快照按 id 顺序保存。
    p.set_plain(0, 3.0); // Speed FAST
    p.set_plain(4, 0.0); // LfRes L
    p.set_plain(10, 1.0); // Freeze on
    p.set_plain(19, 3.0); // LoudPreset -24
    let snapshot = p.snapshot();
    assert_eq!(snapshot.len(), order.len());

    let tmp = std::env::temp_dir().join("orm-analyzer-parity.orm");
    write_state_file(&tmp, &snapshot).expect("write state");
    let read_back = orm_analyzer::params::read_state_file(&tmp).expect("read state");
    assert_eq!(read_back, snapshot, "状态文件往返不一致");
    p.apply_snapshot(&read_back)
        .expect("完整快照应装载成功");

    // 魔数校验：去掉头部必须拒绝。
    let raw = fs::read_to_string(&tmp).unwrap();
    let bad = std::env::temp_dir().join("orm-analyzer-parity-bad.orm");
    fs::write(&bad, &raw[orm_analyzer::params::STATE_MAGIC.len()..]).unwrap();
    assert!(
        orm_analyzer::params::read_state_file(&bad).is_err(),
        "非 analyzer 头文件应被拒绝"
    );

    // 档位数不匹配必须报错（原版 ReadStateFileFrom 同口径：IO 层读数组，
    // 装载层 `ApplySnapshot` 比对 `kNumParams`）。
    let mut short = snapshot.clone();
    short.pop();
    let short_path = std::env::temp_dir().join("orm-analyzer-parity-short.orm");
    write_state_file(&short_path, &short).unwrap();
    let short_back = orm_analyzer::params::read_state_file(&short_path).unwrap();
    assert!(
        p.apply_snapshot(&short_back).is_err(),
        "参数数量不符应报错"
    );

    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(&bad);
    let _ = fs::remove_file(&short_path);
}

#[test]
fn cxx_exporter_matches_rust_tables() {
    // 编译一个 include 原版 Params.h 的小程序，直接由 C++ 编译器导出枚举计数与
    // 常量数组；编译/运行失败（无编译器）时跳过，不判失败。
    let dir = original_src_dir();
    if !dir.join("Params.h").exists() {
        eprintln!("原版源码目录不存在，跳过 C++ 导出比对: {dir:?}");
        return;
    }

    let cc = std::env::var("CXX").unwrap_or_else(|_| "clang++".to_string());
    let exporter = r#"
#include <cstdio>
#include "Params.h"
int main() {
    printf("kNumParams=%d\n", (int)kNumParams);
    printf("speed_log");
    for (double v : kSpeedReleaseLog) printf(" %.17g", v);
    printf("\n");
    printf("speed_lin");
    for (double v : kSpeedReleaseLin) printf(" %.17g", v);
    printf("\n");
    printf("res");
    for (int v : kResOptions) printf(" %d", v);
    printf("\n");
    printf("lfres");
    for (int v : kPbtLfResOptions) printf(" %d", v);
    printf("\n");
    printf("rta_bpo");
    for (int v : kRtaOctaveOptions) printf(" %d", v);
    printf("\n");
    printf("gamma");
    for (int v : kVQTGammaOptions) printf(" %d", v);
    printf("\n");
    printf("hold");
    for (double v : kHoldTimeSecs) printf(" %.17g", v);
    printf("\n");
    printf("targets");
    for (double v : kLoudTargets) printf(" %.17g", v);
    printf("\n");
    printf("scale");
    for (double v : kLoudScaleOffsets) printf(" %.17g", v);
    printf("\n");
    printf("slope_fft");
    for (double v : kSlopeDbFFT) printf(" %.17g", v);
    printf("\n");
    printf("slope_band");
    for (double v : kSlopeDbLog) printf(" %.17g", v);
    printf("\n");
    return 0;
}
"#;

    let work = std::env::temp_dir().join("orm-analyzer-cxx-export");
    fs::create_dir_all(&work).unwrap();
    let src = work.join("export.cc");
    let bin = work.join("export");
    fs::write(&src, exporter).unwrap();

    let compile = Command::new(&cc)
        .arg("-I")
        .arg(&dir)
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .output();
    let Ok(compile) = compile else {
        eprintln!("{cc} 不可用，跳过 C++ 导出比对");
        return;
    };
    if !compile.status.success() {
        eprintln!(
            "C++ 导出编译失败（跳过）: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        return;
    }

    let out = Command::new(&bin).output().expect("run exporter");
    let stdout = String::from_utf8_lossy(&out.stdout);

    let mut lines = stdout.lines();
    let count_line = lines.next().unwrap();
    assert_eq!(count_line, format!("kNumParams={}", orm_analyzer::params::NUM_PARAMS));

    let parse = |line: &str| -> Vec<f64> {
        line.split_whitespace()
            .skip(1)
            .map(|v| v.parse::<f64>().unwrap())
            .collect()
    };
    let next = |lines: &mut std::str::Lines<'_>, label: &str| {
        let l = lines.next().unwrap_or_else(|| panic!("缺少 {label} 行"));
        assert!(l.starts_with(label), "行标签不符: {l}");
        parse(l)
    };

    let cpp_speed_log = next(&mut lines, "speed_log");
    let cpp_speed_lin = next(&mut lines, "speed_lin");
    let cpp_res = next(&mut lines, "res");
    let cpp_lf = next(&mut lines, "lfres");
    let cpp_bpo = next(&mut lines, "rta_bpo");
    let cpp_gamma = next(&mut lines, "gamma");
    let cpp_hold = next(&mut lines, "hold");
    let cpp_targets = next(&mut lines, "targets");
    let cpp_scale = next(&mut lines, "scale");
    let cpp_slope_fft = next(&mut lines, "slope_fft");
    let cpp_slope_band = next(&mut lines, "slope_band");

    let eq = |a: &[f64], b: &[f64], label: &str| {
        assert_eq!(a.len(), b.len(), "{label} 长度");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-9, "{label}: {x} vs {y}");
        }
    };
    eq(&SPEED_RELEASE_LOG, &cpp_speed_log, "speed_log");
    eq(&SPEED_RELEASE_LIN, &cpp_speed_lin, "speed_lin");
    eq(
        &RES_OPTIONS.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &cpp_res,
        "res",
    );
    eq(
        &PBT_LF_RES_OPTIONS.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &cpp_lf,
        "lfres",
    );
    eq(
        &RTA_BPO_OPTIONS.iter().map(|v| *v as f64).collect::<Vec<_>>(),
        &cpp_bpo,
        "rta_bpo",
    );
    eq(&VQT_GAMMA_OPTIONS, &cpp_gamma, "gamma");
    eq(
        &orm_analyzer::params::HOLD_SECS,
        &cpp_hold,
        "hold",
    );
    eq(&LOUD_TARGETS_LUFS, &cpp_targets, "targets");
    eq(&LOUD_SCALE_LU, &cpp_scale, "scale");
    eq(&SLOPE_FFT_DB, &cpp_slope_fft, "slope_fft");
    eq(&SLOPE_BAND_DB, &cpp_slope_band, "slope_band");

    let _ = fs::remove_file(&src);
    let _ = fs::remove_file(&bin);
}
