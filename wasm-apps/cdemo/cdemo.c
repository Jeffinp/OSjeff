/* cdemo.c — a C program compiled to wasm32 (with wasi-sdk's clang) and run
 * natively by OSjeff's WebAssembly app engine. Freestanding: no libc, it talks
 * only to the OS host ABI. This is the C→wasm→native-app pipeline that a ported
 * C game (DOOM) will ride on. */

#define W 320
#define H 180

__attribute__((import_module("host"), import_name("fill_rect")))
void fill_rect(int x, int y, int w, int h, int rgb);
__attribute__((import_module("host"), import_name("draw_text")))
void draw_text(int x, int y, const char *p, int len, int rgb, int scale);
__attribute__((import_module("host"), import_name("blit")))
void blit(const unsigned char *p, int w, int h, int dx, int dy);
__attribute__((import_module("host"), import_name("time_ms")))
long long time_ms(void);

static unsigned char fb[W * H * 4];
static int hue = 0;

static const char TITLE[] = "cdemo.c  -  C compilado para wasm (wasi-sdk)";

__attribute__((export_name("render")))
void render(void) {
    unsigned int t = (unsigned int)time_ms();
    unsigned int ph = t / 16u + (unsigned int)hue;
    for (int y = 0; y < H; y++) {
        for (int x = 0; x < W; x++) {
            int o = (y * W + x) * 4;
            unsigned int v = (unsigned int)(x * x + y * y) + ph;
            unsigned int s = (unsigned int)(x - y) + t / 24u;
            fb[o] = (unsigned char)s;
            fb[o + 1] = (unsigned char)v;
            fb[o + 2] = (unsigned char)((v + s) >> 1);
            fb[o + 3] = 255;
        }
    }
    fill_rect(0, 0, 960, 40, 0x654FF0);
    draw_text(14, 9, TITLE, (int)sizeof(TITLE) - 1, 0xFFFFFF, 2);
    blit(fb, W, H, 8, 56);
}

__attribute__((export_name("on_key")))
void on_key(int code) {
    if (code == 32) {
        hue += 40;
    }
}
