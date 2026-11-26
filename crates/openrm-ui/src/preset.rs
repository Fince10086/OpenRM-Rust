//! 两个插件共用的预设库、撤销栈与渐变状态。
//!
//! 预设槽总数固定 24：右侧 16 个（9…24）+ 底部 quick 8 个（1…8），
//! 与 C++ 版 BandPass 的编号和拖动交换语义一致。Fusion 直接复用同一套。

use std::time::Instant;

pub const SLOT_COUNT: usize = 24;
pub const QUICK_COUNT: usize = 8;

#[derive(Clone, Default)]
pub struct PresetBank {
    /// 显示的 24 个槽，未保存的槽为 None。
    pub slots: Vec<Option<Vec<f64>>>,
    /// 槽位编号（拖动交换时同步交换）。
    pub numbers: Vec<usize>,
    /// 当前选中的槽下标与 quick 位置。
    pub current: usize,
    pub quick: usize,
}

impl PresetBank {
    pub fn empty() -> Self {
        Self {
            slots: vec![None; SLOT_COUNT],
            numbers: (1..=SLOT_COUNT).collect(),
            current: 0,
            quick: 0,
        }
    }

    pub fn from_factory(list: Vec<Vec<f64>>) -> Self {
        let mut slots: Vec<Option<Vec<f64>>> = list.into_iter().take(SLOT_COUNT).map(Some).collect();
        while slots.len() < SLOT_COUNT {
            slots.push(None);
        }
        Self {
            slots,
            numbers: (1..=SLOT_COUNT).collect(),
            current: 0,
            quick: 0,
        }
    }

    pub fn label(&self, idx: usize) -> String {
        format!("{}", self.numbers.get(idx).copied().unwrap_or(idx + 1))
    }

    /// 显示顺序下的标签（网格 16 个 + quick 8 个）。
    pub fn grid_labels(&self) -> Vec<String> {
        (QUICK_COUNT..SLOT_COUNT).map(|i| self.label(i)).collect()
    }

    pub fn quick_labels(&self) -> Vec<String> {
        (0..QUICK_COUNT).map(|i| self.label(i)).collect()
    }

    /// 实际槽内容：`numbers[idx]-1` 指向存档槽。
    pub fn slot(&self, idx: usize) -> Option<&Vec<f64>> {
        let n = self.numbers.get(idx).copied().unwrap_or(idx + 1);
        self.slots.get(n.saturating_sub(1)).and_then(|s| s.as_ref())
    }

    pub fn save(&mut self, idx: usize, snapshot: Vec<f64>) {
        let n = self.numbers.get(idx).copied().unwrap_or(idx + 1);
        if let Some(slot) = self.slots.get_mut(n.saturating_sub(1)) {
            *slot = Some(snapshot);
        }
    }

    pub fn swap(&mut self, a: usize, b: usize) {
        if a == b || a >= SLOT_COUNT || b >= SLOT_COUNT {
            return;
        }
        self.numbers.swap(a, b);
    }

    pub fn to_json(&self) -> serde_json::Value {
        let slots: Vec<serde_json::Value> = self
            .slots
            .iter()
            .map(|s| match s {
                Some(v) => serde_json::json!(v),
                None => serde_json::Value::Null,
            })
            .collect();
        serde_json::json!({ "numbers": self.numbers, "slots": slots })
    }

    pub fn from_json(j: &serde_json::Value, expected: usize) -> Option<Self> {
        let mut bank = Self::empty();
        let arr = j.get("slots")?.as_array()?;
        let mut slots = Vec::with_capacity(SLOT_COUNT);
        for item in arr.iter().take(SLOT_COUNT) {
            if item.is_null() {
                slots.push(None);
            } else {
                let vals = item.as_array()?;
                if vals.len() < expected {
                    slots.push(None);
                } else {
                    slots.push(Some(
                        vals.iter().take(expected).map(|v| v.as_f64().unwrap_or(0.0)).collect(),
                    ));
                }
            }
        }
        while slots.len() < SLOT_COUNT {
            slots.push(None);
        }
        bank.slots = slots;
        if let Some(nums) = j.get("numbers").and_then(|v| v.as_array()) {
            for (i, n) in nums.iter().take(SLOT_COUNT).enumerate() {
                if let Some(v) = n.as_u64() {
                    bank.numbers[i] = v as usize;
                }
            }
        }
        Some(bank)
    }
}

/// 100 步上限的撤销/重做栈，存的是原始单位快照。
#[derive(Default)]
pub struct History {
    pub undo: Vec<Vec<f64>>,
    pub redo: Vec<Vec<f64>>,
}

impl History {
    pub fn push(&mut self, snap: Vec<f64>) {
        self.undo.push(snap);
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: Vec<f64>) -> Option<Vec<f64>> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    pub fn redo(&mut self, current: Vec<f64>) -> Option<Vec<f64>> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
}

/// 预设切换的渐变状态。归一化空间的插值由插件层完成（truce 的 range 在那边）。
pub struct Fade {
    pub from: Vec<f64>,
    pub to: Vec<f64>,
    pub start: Instant,
    pub dur: f64,
}

impl Fade {
    pub fn new(from: Vec<f64>, to: Vec<f64>, dur: f64) -> Self {
        Self {
            from,
            to,
            start: Instant::now(),
            dur: dur.max(0.01),
        }
    }

    /// 0..1 的进度；满 1 表示结束。
    pub fn progress(&self) -> f64 {
        (self.start.elapsed().as_secs_f64() / self.dur).clamp(0.0, 1.0)
    }
}
