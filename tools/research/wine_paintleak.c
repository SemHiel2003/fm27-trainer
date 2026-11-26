// Minimal reproduction of the Wine bug that app/src/wine.rs works around: a delivered RDW_INTERNALPAINT leaves
// QS_PAINT set for good, so an idle MsgWaitForMultipleObjectsEx(..., MWMO_INPUTAVAILABLE) wait returns at once
// (a busy loop). A counted invalidation (RDW_INVALIDATE | RDW_NOERASE) does not leak.
//
// Build: x86_64-w64-mingw32-gcc -O2 -o wine_paintleak.exe wine_paintleak.c -luser32 -lgdi32
// Run under Wine/Proton; the results go to wine_paintleak.out next to the exe. Under Proton Experimental 11.0-100
// and GE-Proton 11-12 (26 Nov 2026):
//   invalidate 1:     QS_PAINT before=0x20 after=0 WM_PAINT delivered=1
//   after invalidate: spins(INPUTAVAILABLE)/200ms=1
//   internal paint 1: QS_PAINT before=0x20 after=0x20 WM_PAINT delivered=1
//   after:            spins(INPUTAVAILABLE)/200ms=9513 spins(no flag)/200ms=1
//   after validate:   QS_PAINT=0x20
// On Windows, QS_PAINT is expected to be 0 after every WM_PAINT and spins to stay at ~1.
#include <windows.h>
#include <stdio.h>
#include <string.h>

static int paints;
static LRESULT CALLBACK proc(HWND h, UINT m, WPARAM w, LPARAM l) {
    if (m == WM_PAINT) { PAINTSTRUCT ps; BeginPaint(h, &ps); EndPaint(h, &ps); paints++; return 0; }
    return DefWindowProcW(h, m, w, l);
}
static void pump(void) { MSG msg; while (PeekMessageW(&msg, 0, 0, 0, PM_REMOVE)) DispatchMessageW(&msg); }
static unsigned qs_paint(void) { return HIWORD(GetQueueStatus(QS_PAINT)); }
static long spins(int flags) {  // wakeups of an idle 200 ms wait
    long n = 0; DWORD t = GetTickCount();
    while (GetTickCount() - t < 200) { MsgWaitForMultipleObjectsEx(0, NULL, 200, QS_ALLINPUT, flags); pump(); n++; }
    return n;
}

int main(void) {
    {   // results go next to the exe (stdout is not captured under Proton)
        char path[MAX_PATH]; GetModuleFileNameA(0, path, MAX_PATH);
        char *dot = strrchr(path, '.'); if (dot) strcpy(dot, ".out");
        freopen(path, "w", stdout);
    }
    WNDCLASSW wc = {0}; wc.lpfnWndProc = proc; wc.hInstance = GetModuleHandleW(0); wc.lpszClassName = L"leak";
    RegisterClassW(&wc);
    HWND h = CreateWindowW(L"leak", L"leak", WS_OVERLAPPEDWINDOW | WS_VISIBLE, 0, 0, 200, 200, 0, 0, wc.hInstance, 0);
    for (int i = 0; i < 20; i++) { pump(); Sleep(10); }
    printf("start:            QS_PAINT=%#x paints=%d spins(INPUTAVAILABLE)/200ms=%ld\n", qs_paint(), paints, spins(MWMO_INPUTAVAILABLE));
    for (int k = 1; k <= 3; k++) {  // the substitute: a counted update region instead of the internal flag
        int p = paints;
        RedrawWindow(h, NULL, NULL, RDW_INVALIDATE | RDW_NOERASE);
        unsigned before = qs_paint();
        pump();
        printf("invalidate %d:     QS_PAINT before=%#x after=%#x WM_PAINT delivered=%d\n", k, before, qs_paint(), paints - p);
    }
    printf("after invalidate: spins(INPUTAVAILABLE)/200ms=%ld\n", spins(MWMO_INPUTAVAILABLE));
    for (int k = 1; k <= 3; k++) {
        int p = paints;
        RedrawWindow(h, NULL, NULL, RDW_INTERNALPAINT);
        unsigned before = qs_paint();
        pump();
        printf("internal paint %d: QS_PAINT before=%#x after=%#x WM_PAINT delivered=%d\n", k, before, qs_paint(), paints - p);
    }
    printf("after:            spins(INPUTAVAILABLE)/200ms=%ld spins(no flag)/200ms=%ld\n", spins(MWMO_INPUTAVAILABLE), spins(0));
    RedrawWindow(h, NULL, NULL, RDW_VALIDATE | RDW_NOINTERNALPAINT | RDW_ALLCHILDREN);
    printf("after validate:   QS_PAINT=%#x\n", qs_paint());
    fflush(stdout);
    DestroyWindow(h);
    return 0;
}
