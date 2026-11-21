// gen_pbt_rta.cpp — PBT/RTA 原版 C++ 黄金数据生成器（离线，仅供 Rust 移植比对）。
//
// 直接 include 原版头文件（经 shim_isender.h 替身 ISender），导出带中心频率、
// 滤波器系数、带中心校准增益、PBT 核长/层号/对齐偏移/复解调核。
//
// 构建（在 OpenRM-Rust/crates/openrm-analyzer-dsp 目录）：
//   clang++ -std=c++17 -O2 -I tests/data \
//     -I /Users/feizao/个人文件/plugin-dev/OpenRM/plugins/Analyzer/src/dsp \
//     tests/data/gen_pbt_rta.cpp -lm -o /tmp/orm_pbt_rta
//   /tmp/orm_pbt_rta tests/data
//
// 注意：TU 内 `#define private public` 仅为读取受保护成员做黄金转储，不影响算法本体。

#define private public
#define protected public

#include "shim_isender.h"

#include "PBTAnalyzer.h"
#include "RTAAnalyzer.h"

#include <cmath>
#include <cstdio>
#include <string>
#include <vector>

using namespace iplug;

static constexpr double K_FS = 48000.0;

// 与 RebuildBands 相同的带缘网格（诊断用：比对 Rust 边缘频率是否对齐）。
static std::vector<double> RtaEdges(int bpo, double fs) {
  const double kLn2 = 0.69314718055994530942;
  const double k = std::exp(kLn2 / bpo);
  double f = 15.625;
  while (f < 22.0)
    f *= k;
  const double fedg = 0.94 * 0.5 * fs;
  const int wi = (int)(0.5 + std::log(fedg / (1000.0 * std::sqrt(k))) / std::log(k));
  const double fli = 1.0 / (1000.0 * std::pow(k, wi));
  const double wr = (1.0 - (fedg / std::sqrt(k)) * fli) * fli * fli;
  std::vector<double> edges;
  double ff = (f / std::sqrt(k)) * (1.0 - wr * f * f);
  for (;;) {
    const double top = f * std::sqrt(k) * (1.0 - wr * f * f);
    if (f > 20000.0 || top > fedg + 1.0)
      break;
    edges.push_back(ff);
    ff = top;
    f *= k;
  }
  edges.push_back(ff);
  return edges;
}

static void DumpRta(const std::string &dir) {
  FILE *f = std::fopen((dir + "/data_rta.txt").c_str(), "w");
  std::fprintf(f, "# RTA golden tables (fs=48000), exported from RTAAnalyzer.h\n");
  std::fprintf(f, "# modes: 0=1/6, 1=1/12, 2=1/24; K0/K1/K2 length=bands+1; GAIN length=bands\n");

  const int bpos[] = {6, 12, 24};
  for (int mode = 0; mode < 3; ++mode) {
    RTAAnalyzer<3> eng;
    eng.SetSampleRate(K_FS);
    eng.SetOctaveMode(mode);
    eng.CheckRebuild();

    const int nb = eng.NumBands();
    std::fprintf(f, "RTA mode=%d bands=%d env_alpha=%.12g\n", mode, nb,
                 (double)eng.mEnvAlphaBlk);
    std::fprintf(f, "FREQ");
    for (int b = 0; b < nb; ++b)
      std::fprintf(f, " %.12g", eng.BandFreqs()[b]);
    std::fprintf(f, "\n");

    std::vector<double> edges = RtaEdges(bpos[mode], K_FS);
    std::fprintf(f, "EDGES");
    for (double e : edges)
      std::fprintf(f, " %.12g", e);
    std::fprintf(f, "\n");

    std::fprintf(f, "K0");
    for (int j = 0; j <= nb; ++j)
      std::fprintf(f, " %.12g", (double)eng.mK0[j]);
    std::fprintf(f, "\nK1");
    for (int j = 0; j <= nb; ++j)
      std::fprintf(f, " %.12g", (double)eng.mK1[j]);
    std::fprintf(f, "\nK2");
    for (int j = 0; j <= nb; ++j)
      std::fprintf(f, " %.12g", (double)eng.mK2[j]);
    std::fprintf(f, "\nGAIN");
    for (int b = 0; b < nb; ++b)
      std::fprintf(f, " %.12g", (double)eng.mInvGain[b]);
    std::fprintf(f, "\n");
  }
  std::fclose(f);
}

static void DumpPbt(const std::string &dir) {
  FILE *f = std::fopen((dir + "/data_pbt.txt").c_str(), "w");
  std::fprintf(f, "# PBT golden tables (fs=48000), exported from PBTAnalyzer.h\n");
  std::fprintf(f, "# lf: 40/20/10 Hz; per band: layer winLen readOff wsumInv + full Re/Im kernel\n");

  const int lfs[] = {40, 20, 10};
  for (int lf : lfs) {
    PBTAnalyzer<3> eng;
    eng.SetSampleRate(K_FS);
    eng.SetLfWidth(lf);
    eng.CheckRebuild();

    const int nb = eng.NumBands();
    std::fprintf(f, "PBT lf=%d bands=%d\n", lf, nb);
    std::fprintf(f, "FREQ");
    for (int b = 0; b < nb; ++b)
      std::fprintf(f, " %.12g", eng.BandFreqs()[b]);
    std::fprintf(f, "\n");

    for (int b = 0; b < nb; ++b) {
      const auto &bd = eng.mBands[b];
      std::fprintf(f, "BAND %d layer %d winLen %d readOff %d wsumInv %.12g klen %d\n",
                    b, bd.layer, bd.winLen, bd.readOff, (double)bd.wsumInv,
                    (int)bd.kernelRe.size());
      std::fprintf(f, "KR");
      for (int n = 0; n < bd.winLen; ++n)
        std::fprintf(f, " %.12g", (double)bd.kernelRe[n]);
      std::fprintf(f, "\nKI");
      for (int n = 0; n < bd.winLen; ++n)
        std::fprintf(f, " %.12g", (double)bd.kernelIm[n]);
      std::fprintf(f, "\n");
    }
  }
  std::fclose(f);
}

int main(int argc, char **argv) {
  std::string dir = ".";
  if (argc > 1)
    dir = argv[1];
  DumpRta(dir);
  DumpPbt(dir);
  std::printf("PBT/RTA evidence written to %s\n", dir.c_str());
  return 0;
}
