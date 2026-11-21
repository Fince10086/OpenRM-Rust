// shim_isender.h — 离线黄金数据导出用的最小 ISender 替身。
//
// 原版 PBT/RTA 头文件继承 iPlug2 `ISender`；本替身只提供编译期所需的
// `ISenderData` / `ISender` 形态，不接入任何 iPlug2 依赖。仅供 tests/data 下的
// 离线导出小程序使用，不进入插件产物。

#ifndef ORM_SHIM_ISENDER_H
#define ORM_SHIM_ISENDER_H

#include <array>

#define BEGIN_IPLUG_NAMESPACE namespace iplug {
#define END_IPLUG_NAMESPACE }

namespace iplug {

constexpr int kNoTag = 0;
using sample = float;

template <int MAXNC, typename T>
struct ISenderData {
  int ctrlTag = kNoTag;
  int nChans = MAXNC;
  int chanOffset = 0;
  std::array<T, MAXNC> vals;

  ISenderData() : vals{} {}
  ISenderData(int t, int n, int o) : ctrlTag(t), nChans(n), chanOffset(o), vals{} {}
};

template <int MAXNC, int QUEUE_SIZE, typename T>
class ISender {
public:
  using Data = ISenderData<MAXNC, T>;
  void PushData(const Data&) {}
  virtual void PrepareDataForUI(Data&) {}

protected:
  Data mLastData;
};

} // namespace iplug

#endif // ORM_SHIM_ISENDER_H
