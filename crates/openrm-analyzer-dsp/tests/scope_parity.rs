//! scope_parity — StereoScope 离线回归，移植自
//! `OpenRM/plugins/Analyzer/src/dsp/StereoScope.h`。
//!
//! 验证 hop=1024 的左右样本拼接顺序：帧内 `vals[i] = [L[i], R[i], 0]`，
//! 与原版 ProcessBlock 逐样本攒包顺序一致。

use openrm_analyzer_dsp::stereo_scope::StereoScope;

#[test]
fn l_r_sample_order_matches_cpp() {
    const HOP: usize = 1024;
    let l: Vec<f32> = (0..HOP).map(|i| i as f32 / HOP as f32).collect();
    let r: Vec<f32> = (0..HOP).map(|i| -(i as f32) / HOP as f32).collect();
    let m = vec![0.0_f32; HOP];

    let mut scope = StereoScope::new();
    scope.process_block(&l, &r, &m);
    let frame = scope.take_frame().expect("攒满 1024 应有一帧");

    assert_eq!(frame.vals.len(), HOP);
    assert!(frame.freqs.is_empty());
    for i in 0..HOP {
        assert_eq!(frame.vals[i][0], l[i], "L[{i}] 顺序");
        assert_eq!(frame.vals[i][1], r[i], "R[{i}] 顺序");
        assert_eq!(frame.vals[i][2], 0.0, "M 路应为 0");
    }
}

#[test]
fn second_hop_overwrites_latest_frame() {
    const HOP: usize = 1024;
    let l1 = vec![0.25_f32; HOP];
    let r1 = vec![-0.25_f32; HOP];
    let l2 = vec![0.75_f32; HOP];
    let r2 = vec![-0.75_f32; HOP];
    let m = vec![0.0_f32; HOP];

    let mut scope = StereoScope::new();
    scope.process_block(&l1, &r1, &m);
    scope.process_block(&l2, &r2, &m);
    let frame = scope.take_frame().expect("第二 hop 出帧");
    for v in &frame.vals {
        assert_eq!(v[0], 0.75);
        assert_eq!(v[1], -0.75);
    }
    // 取走后不再残留旧帧。
    assert!(scope.take_frame().is_none());
}
