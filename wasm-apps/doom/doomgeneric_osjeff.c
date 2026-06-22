/* OSjeff platform layer for doomgeneric: maps DOOM's display/input/timing onto
 * the OSjeff WASM host ABI. Compiled to wasm32-wasi with wasi-sdk and run by the
 * OSjeff WASM app engine. `render` runs one game tick (driven by the OS frame
 * pump); `on_key` queues input. */

#include "doomgeneric.h"
#include "doomkeys.h"
#include <stdint.h>

__attribute__((import_module("host"), import_name("blit")))
void host_blit(const unsigned char *p, int w, int h, int dx, int dy);
__attribute__((import_module("host"), import_name("time_ms")))
long long host_time_ms(void);

#define DGW DOOMGENERIC_RESX
#define DGH DOOMGENERIC_RESY

static unsigned char rgba[DGW * DGH * 4];

/* Tiny key ring buffer filled by on_key, drained by DG_GetKey. */
static unsigned char key_buf[64];
static int key_head, key_tail;

void DG_Init(void) {}

void DG_DrawFrame(void) {
    for (int i = 0; i < DGW * DGH; i++) {
        uint32_t p = DG_ScreenBuffer[i]; /* 0x00RRGGBB */
        rgba[i * 4 + 0] = (unsigned char)(p >> 16);
        rgba[i * 4 + 1] = (unsigned char)(p >> 8);
        rgba[i * 4 + 2] = (unsigned char)(p);
        rgba[i * 4 + 3] = 255;
    }
    host_blit(rgba, DGW, DGH, 8, 56);
}

void DG_SleepMs(uint32_t ms) { (void)ms; }

uint32_t DG_GetTicksMs(void) { return (uint32_t)host_time_ms(); }

int DG_GetKey(int *pressed, unsigned char *key) {
    if (key_head == key_tail) {
        return 0;
    }
    unsigned char k = key_buf[key_tail];
    key_tail = (key_tail + 1) & 63;
    *pressed = 1;
    *key = k;
    return 1;
}

void DG_SetWindowTitle(const char *title) { (void)title; }

/* Map an ASCII byte from host.on_key to a DOOM key code. */
static unsigned char to_doom_key(int code) {
    switch (code) {
        case 'w': case 'W': return KEY_UPARROW;
        case 's': case 'S': return KEY_DOWNARROW;
        case 'a': case 'A': return KEY_LEFTARROW;
        case 'd': case 'D': return KEY_RIGHTARROW;
        case ' ': return KEY_FIRE;
        case 'e': case 'E': return KEY_USE;
        case 10: return KEY_ENTER;
        case 27: return KEY_ESCAPE;
        default: return (unsigned char)code;
    }
}

static int dg_started;

__attribute__((export_name("render")))
void render(void) {
    if (!dg_started) {
        dg_started = 1;
        char arg0[] = "doom";
        char *argv[] = {arg0};
        doomgeneric_Create(1, argv);
    }
    doomgeneric_Tick();
}

__attribute__((export_name("on_key")))
void on_key(int code) {
    int next = (key_head + 1) & 63;
    if (next != key_tail) {
        key_buf[key_head] = to_doom_key(code);
        key_head = next;
    }
}
