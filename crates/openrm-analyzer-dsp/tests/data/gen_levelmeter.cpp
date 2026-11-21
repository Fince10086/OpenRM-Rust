// gen_levelmeter.cpp — LevelMeter 原版 C++ 黄金快照生成器（离线，仅供 Rust 移植比对）。
//
// 构建：
//   clang++ -std=c++17 -O2 -I <Analyzer>/src/dsp -I <common>/dsp \
//     tests/data/gen_levelmeter.cpp -lm -o /tmp/orm_lm
//   /tmp/orm_lm tests/data
//
// 场景序列与 Rust `level_meter_parity.rs` 完全一致（同正弦相位、同 512 块长）。

#include "LevelMeter.h"

#include <cmath>
#include <cstdio>
#include <string>
#include <vector>

using namespace iplug;

static constexpr double K_SR = 48000.0;
static constexpr int K_BLOCK = 512;

static void Sine(std::vector<float> &out, int n, double freq, double amp) {
  out.resize(n);
  for (int i = 0; i < n; ++i) {
    out[i] = (float)(amp * std::sin(2.0 * M_PI * freq * i / K_SR));
  }
}

static void Dump(FILE *f, const char *tag, const LevelMeter &m) {
  LevelMeter::Snapshot s;
  m.Store(s);
  std::fprintf(f,
               "%s peakL=%.6g peakR=%.6g trueL=%.6g trueR=%.6g rmsL=%.6g rmsR=%.6g "
               "vuL=%.6g vuR=%.6g vuHoldL=%.6g vuHoldR=%.6g persistL=%.6g persistR=%.6g "
               "holdL=%.6g holdR=%.6g holdSec=%.6g overL=%d overR=%d\n",
               tag, s.peakDbL, s.peakDbR, s.trueDbL, s.trueDbR, s.rmsDbL, s.rmsDbR,
               s.vuDbL, s.vuDbR, s.vuHoldDbL, s.vuHoldDbR, s.persistDbL, s.persistDbR,
               s.holdDbL, s.holdDbR, s.holdSec, s.overL, s.overR);
}

static void Run(LevelMeter &m, const std::vector<float> &x, int mode, double holdSec) {
  int pos = 0;
  while (pos < (int)x.size()) {
    int b = K_BLOCK;
    if (pos + b > (int)x.size())
      b = (int)x.size() - pos;
    m.Process(x.data() + pos, x.data() + pos, b, mode, holdSec);
    pos += b;
  }
}

int main(int argc, char **argv) {
  std::string dir = ".";
  if (argc > 1)
    dir = argv[1];
  FILE *f = std::fopen((dir + "/data_levelmeter.txt").c_str(), "w");
  std::fprintf(f, "# LevelMeter golden snapshots (sr=48000, block=512), exported from LevelMeter.h\n");

  // A: mode 1 稳态正弦 0.5 (−6.02 dBFS 峰值 / −9.03 dBFS RMS)
  {
    LevelMeter m;
    m.SetSampleRate(K_SR);
    std::vector<float> x;
    Sine(x, (int)(0.5 * K_SR), 1000.0, 0.5);
    Run(m, x, 1, 2.0);
    Dump(f, "A", m);
  }

  // B: 过载锁存 —— 1.2 幅度 0.2s，再静音 0.3s（over 应锁存）
  {
    LevelMeter m;
    m.SetSampleRate(K_SR);
    std::vector<float> x;
    Sine(x, (int)(0.2 * K_SR), 1000.0, 1.2);
    Run(m, x, 1, 2.0);
    std::vector<float> quiet((int)(0.3 * K_SR), 0.0f);
    Run(m, quiet, 1, 2.0);
    Dump(f, "B", m);
  }

  // C: mode 0 真峰值 —— 0.5 幅度 0.5s
  {
    LevelMeter m;
    m.SetSampleRate(K_SR);
    std::vector<float> x;
    Sine(x, (int)(0.5 * K_SR), 1000.0, 0.5);
    Run(m, x, 0, 2.0);
    Dump(f, "C", m);
  }

  // D: dBTP 持久锁存 —— 1.1 幅度 0.2s，再静音 0.3s（persist 应锁存 >0）
  {
    LevelMeter m;
    m.SetSampleRate(K_SR);
    std::vector<float> x;
    Sine(x, (int)(0.2 * K_SR), 1000.0, 1.1);
    Run(m, x, 0, 2.0);
    std::vector<float> quiet((int)(0.3 * K_SR), 0.0f);
    Run(m, quiet, 0, 2.0);
    Dump(f, "D", m);
  }

  // E: 保持衰减 —— 0.5s 0.5 幅度（holdSec=0.5），再静音 3.0s（hold 应已回落）
  {
    LevelMeter m;
    m.SetSampleRate(K_SR);
    std::vector<float> x;
    Sine(x, (int)(0.5 * K_SR), 1000.0, 0.5);
    Run(m, x, 1, 0.5);
    std::vector<float> quiet((int)(3.0 * K_SR), 0.0f);
    Run(m, quiet, 1, 0.5);
    Dump(f, "E", m);
  }

  // F: Reset 后全默认
  {
    LevelMeter m;
    m.SetSampleRate(K_SR);
    std::vector<float> x;
    Sine(x, (int)(0.2 * K_SR), 1000.0, 0.8);
    Run(m, x, 1, 2.0);
    m.Reset();
    Dump(f, "F", m);
  }

  std::fclose(f);
  std::printf("LevelMeter evidence written to %s\n", dir.c_str());
  return 0;
}
