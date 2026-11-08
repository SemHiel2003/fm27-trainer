// Testbeeld voor de MSVC-RTTI-herkenning van FM27 Trainer: klassen zoals FM27 ze heeft
// (namespace db). Opnieuw maken (macOS, met de Rust-toolchain):
//   clang --target=x86_64-pc-windows-msvc -O1 -c rtti_test.cpp -o rtti_test.obj
//   SR=$(rustc --print sysroot); DYLD_LIBRARY_PATH=$SR/lib \
//     $SR/lib/rustlib/aarch64-apple-darwin/bin/rust-lld -flavor link /dll /noentry \
//     /nodefaultlib /force:unresolved /out:rtti_test.dll rtti_test.obj
namespace db {
class PERSON { public: virtual ~PERSON() {} int uid = 1; };
class NON_PLAYER : public PERSON { public: virtual int role() { return 1; } int x = 2; };
class HUMAN_NON_PLAYER : public NON_PLAYER { public: int role() override { return 7; } long long y = 3; };
class PLAYER : public PERSON { public: int morale = 10; };
struct OTHER { virtual void f() {} };
class PLAYER_AND_NON_PLAYER : public PLAYER, public OTHER { public: int z = 4; };
}
db::HUMAN_NON_PLAYER g_human;
db::PLAYER g_player;
db::PLAYER_AND_NON_PLAYER g_coach;
extern "C" __declspec(dllexport) void* objs(int i) {
  void* o[3] = {&g_human, &g_player, &g_coach};
  return o[i];
}
