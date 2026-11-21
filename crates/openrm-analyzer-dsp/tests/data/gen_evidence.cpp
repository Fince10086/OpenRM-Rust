// gen_evidence.cpp — 原版 C++ 引擎黄金数据生成器（离线，仅供 Rust 移植比对）。
//
// 直接 include 原版头文件，链接真实 WDL fft.c；输出文本到 tests/data/。
// 构建（在 OpenRM-Rust/crates/openrm-analyzer-dsp 目录）：
//   clang -c -std=gnu11 -O2 <WDL>/fft.c -o /tmp/orm_fft.o
//   clang++ -std=c++17 -O2 -I <iPlug>/IPlug -I <WDL> -I <common>/dsp \
//           -I <Analyzer>/src tests/data/gen_evidence.cpp /tmp/orm_fft.o -lm -o /tmp/orm_evidence
//   /tmp/orm_evidence tests/data
//
// 注意：TU 内 `#define private public` 仅为读取受保护成员做黄金转储，不影响算法本体。
#define private public
#define protected public

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include "IPlugConstants.h"
#include "IPlugEditorDelegate.h"

#include "SpectrumSTFT.h"
#include "dsp/VQTAnalyzer.h"
#include "dsp/HalfbandDec2.h"
#include "dsp/StereoScope.h"
#include "dsp/TestSignalGenerator.h"

using namespace iplug;
using orm::TestSignalGenerator;

// ---------------------------------------------------------------------------
// STFT 黄金数据
// ---------------------------------------------------------------------------
static void DumpStft(const std::string &dir) {
  FILE *f = std::fopen((dir + "/data_stft.txt").c_str(), "w");
  const int sizes[] = {2048, 4096, 8192};
  const int wtypes[] = {0, 1, 2};

  for (int N : sizes) {
    for (int wt : wtypes) {
      SpectrumSTFT<3> eng(N, std::max(1, N / 1024), wt,
                         SpectrumSTFT<3>::kCalDouble);
      std::fprintf(f, "=== WINDOW N=%d WT=%d ===\n", N, wt);
      std::fprintf(f, "hop %d num_bins %d scaling %.12g\n", eng.mHop,
                   eng.mNumBins, eng.mScaling);
      double sum = 0.0;
      for (int i = 0; i < N; ++i) sum += eng.mWindow[i];
      std::fprintf(f, "sum %.12g\n", sum);
      const int idx[] = {0, 1, N / 4, N / 2, 3 * N / 4, N - 1};
      for (int i : idx)
        std::fprintf(f, "w[%d]=%.12g\n", i, (double)eng.mWindow[i]);
    }
  }

  // 整窗 bin 中心正弦：N=4096, Hann, kCalDouble, A=1, bin k=64
  for (int N : sizes) {
    const int overlap = std::max(1, N / 1024);
    const int hop = N / overlap;
    SpectrumSTFT<3> eng(N, overlap, 0, SpectrumSTFT<3>::kCalDouble);
    const int k = 64;
    std::vector<double> x(N);
    for (int n = 0; n < N; ++n)
      x[n] = std::sin(2.0 * iplug::PI * k * n / N);

    // 逐 hop 喂原始样本，走真实 PrepareDataForUI（历史缓冲累积）
    for (int base = 0; base < N; base += hop) {
      ISenderData<3, std::array<float, 8192>> d;
      d.nChans = 3;
      d.chanOffset = 0;
      for (int c = 0; c < 3; ++c)
        for (int s = 0; s < hop; ++s)
          d.vals[c][s] = (float)x[base + s];
      eng.PrepareFrameUI(d);
    }
    std::fprintf(f, "=== SINE N=%d K=%d cal=kCalDouble ===\n", N, k);
    // 直接从最后一帧 d.vals 取：重放一次最后 hop，覆盖 d
    ISenderData<3, std::array<float, 8192>> d;
    d.nChans = 3;
    for (int c = 0; c < 3; ++c)
      for (int s = 0; s < hop; ++s)
        d.vals[c][s] = (float)x[N - hop + s];
    eng.PrepareFrameUI(d);
    for (int bin : {0, k - 1, k, k + 1})
      std::fprintf(f, "mag[%d] L=%.12g R=%.12g M=%.12g\n", bin,
                   (double)d.vals[0][bin], (double)d.vals[1][bin],
                   (double)d.vals[2][bin]);
  }

  // DC 标定：整窗直流 1.0
  {
    const int N = 4096;
    const int overlap = std::max(1, N / 1024);
    const int hop = N / overlap;
    SpectrumSTFT<3> eng(N, overlap, 0, SpectrumSTFT<3>::kCalDouble);
    ISenderData<3, std::array<float, 8192>> d;
    d.nChans = 3;
    for (int base = 0; base < N; base += hop) {
      for (int c = 0; c < 3; ++c)
        for (int s = 0; s < hop; ++s) d.vals[c][s] = 1.0f;
      eng.PrepareFrameUI(d);
    }
    std::fprintf(f, "=== DC N=%d cal=kCalDouble ===\n", N);
    std::fprintf(f, "dc L=%.12g\n", (double)d.vals[0][0]);
  }
  std::fclose(f);
}

// ---------------------------------------------------------------------------
// VQT 黄金数据
// ---------------------------------------------------------------------------
static void DumpVqt(const std::string &dir) {
  const int srs[] = {44100, 48000, 96000};
  const int gammas[] = {20, 10, 5};
  for (int sr : srs) {
    for (int g : gammas) {
      VQTAnalyzer<3> eng;
      eng.SetSampleRate(sr);
      eng.SetBpo(24);
      eng.SetGamma(g);
      eng.SetWindowType(1); // Analyzer 运行态默认 Clean/BH5
      eng.CheckRebuild();

      char path[512];
      std::snprintf(path, sizeof(path), "%s/data_vqt_%d_g%d.txt", dir.c_str(),
                     sr, g);
      FILE *f = std::fopen(path, "w");
      std::fprintf(f, "sr %d gamma %d bpo 24 bands %d\n", sr, g,
                   eng.NumBands());
      const auto &freqs = eng.BandFreqs();
      for (size_t b = 0; b < freqs.size(); ++b) {
        const auto &bd = eng.mBands[b];
        std::fprintf(f,
                     "b%zu fc %.12g layer %d winLen %d advance %d readOff %d "
                     "wsumInv %.12g kr0 %.12g ki0 %.12g kr1 %.12g ki1 %.12g\n",
                     b, freqs[b], bd.layer, bd.winLen, bd.advance, bd.readOff,
                     bd.wsumInv, bd.kernelRe[0], bd.kernelIm[0],
                     bd.kernelRe[1], bd.kernelIm[1]);
      }
      std::fclose(f);
    }
  }
}

// ---------------------------------------------------------------------------
// Halfband 黄金数据
// ---------------------------------------------------------------------------
static void DumpHalfband(const std::string &dir) {
  FILE *f = std::fopen((dir + "/data_halfband.txt").c_str(), "w");
  detail::HalfbandDec2 mp;
  mp.SetMinPhase(true);
  std::fprintf(f, "gd_per_stage %.12g\n",
               detail::HalfbandDec2::GdPerStage());
  std::fprintf(f, "=== MINPHASE taps=%d ===\n", detail::HalfbandDec2::kN);
  for (int i = 0; i < detail::HalfbandDec2::kN; ++i)
    std::fprintf(f, "t[%d]=%.12g\n", i, (double)mp.mTap[i]);

  detail::HalfbandDec2 lp;
  std::fprintf(f, "=== LINEAR taps=%d ===\n", detail::HalfbandDec2::kN);
  for (int i = 0; i < detail::HalfbandDec2::kN; ++i)
    std::fprintf(f, "t[%d]=%.12g\n", i, (double)lp.mTap[i]);
  std::fclose(f);
}

// ---------------------------------------------------------------------------
// TestSignal 黄金数据
// ---------------------------------------------------------------------------
static void DumpTestGen(const std::string &dir) {
  FILE *f = std::fopen((dir + "/data_testgen.txt").c_str(), "w");
  const int N = 64;
  for (int ty = 0; ty < orm::kNumGenSignals; ++ty) {
    TestSignalGenerator gen;
    TestSignalGenerator::Config c;
    c.type = (orm::ETestSignal)ty;
    c.freqHz = 1000.0;
    c.levelDb = -12.0;
    c.fftSize = 4096;
    c.sweepSec = 4.0;
    c.sweepF0 = 20.0;
    c.sweepF1 = 20000.0;
    gen.SetConfig(c);
    gen.SetSeed(0x12345678u);
    gen.Restart();

    std::vector<float> L(N), R(N);
    gen.Fill(L.data(), R.data(), N, 48000.0, true);

    std::fprintf(f, "=== TYPE %d ===\n", ty);
    for (int i = 0; i < N; ++i)
      std::fprintf(f, "s[%d] L=%.12g R=%.12g\n", i, (double)L[i],
                   (double)R[i]);
  }
  std::fclose(f);
}

int main(int argc, char **argv) {
  std::string dir = ".";
  if (argc > 1) dir = argv[1];
  WDL_fft_init();
  DumpStft(dir);
  DumpVqt(dir);
  DumpHalfband(dir);
  DumpTestGen(dir);
  std::printf("evidence written to %s\n", dir.c_str());
  return 0;
}
