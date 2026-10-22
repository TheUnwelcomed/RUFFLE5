/* PS5_RUFFLE_PAD_V1 — the controller and the speaker, for the Ruffle title.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Thin C over the system modules, as PS5_RetroArch's src/input_ps5.cpp and src/audio_ps5.cpp drive
 * them — the calls, the 120-byte pad sample and its offsets, and the AudioOut parameters are the
 * ones verified on hardware there (and by ProsperoLight / PS5_Proton before it). Nothing is guessed:
 *   pad:   sceUserServiceInitialize, sceUserServiceGetInitialUser, scePadInit,
 *          scePadOpen(user, 0, 0, NULL) (retried: the pad is not always published when a title
 *          started from the shell asks), scePadRead(handle, samples, 64) → the newest sample.
 *   audio: sceAudioOutInit, sceAudioOutOpen(0xff, MAIN=0, 0, 256 frames, 48000 Hz, S16 stereo=1),
 *          sceAudioOutOutput(port, 256 interleaved s16 stereo frames) — blocks until queued.
 * The Rust side (src/input.rs, src/audio.rs) does the mapping and the mixing.
 */
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int32_t sceUserServiceInitialize(const void *params);
int32_t sceUserServiceGetInitialUser(int32_t *user_id);
int32_t scePadInit(void);
int32_t scePadOpen(int32_t user_id, int32_t port_type, int32_t index, const void *param);
int32_t scePadRead(int32_t handle, void *samples, int32_t capacity);
int32_t sceAudioOutInit(void);
int32_t sceAudioOutOpen(int32_t user_id, int32_t type, int32_t index, uint32_t len, uint32_t freq, uint32_t param);
int32_t sceAudioOutOutput(int32_t handle, const void *data);

/* One sample, as the console writes it (input_ps5.cpp: static_asserts on every offset). */
struct pad_sample {
    uint32_t buttons;
    uint8_t left_x, left_y, right_x, right_y;
    uint8_t left_trigger, right_trigger;
    uint8_t reserved_to_touch[42];
    uint8_t touch_count;
    uint8_t reserved_touch[7];
    struct { uint16_t x, y; uint8_t id, reserved[3]; } touches[2];
    int32_t connected;
    uint64_t timestamp_us;
    uint8_t extension[16];
    uint8_t connected_count;
    uint8_t remaining[15];
};
_Static_assert(sizeof(struct pad_sample) == 120, "pad samples are 120 bytes");
_Static_assert(offsetof(struct pad_sample, touch_count) == 0x34, "touch count at 0x34");
_Static_assert(offsetof(struct pad_sample, touches) == 0x3c, "touches at 0x3c");
_Static_assert(offsetof(struct pad_sample, connected) == 0x4c, "connected at 0x4c");
_Static_assert(offsetof(struct pad_sample, timestamp_us) == 0x50, "timestamp at 0x50");

/* What Rust reads (src/input.rs mirrors this, repr(C)). */
struct ps5_pad_state {
    uint32_t buttons;
    uint8_t left_x, left_y, right_x, right_y;
    uint8_t left_trigger, right_trigger;
    uint8_t touch_count;
    uint8_t touch_id;
    uint16_t touch_x, touch_y;
    int32_t connected;
};

static int32_t g_user = -1;
static int32_t g_pad = -1;
static struct pad_sample g_samples[64];

/* 0: a pad is open. <0: the step that failed, as -(1..4), with the module's result in *detail. */
int ps5_pad_init(int32_t *detail)
{
    /* As input_ps5.cpp: the result is noted, not fatal (the service may already be up). */
    (void)sceUserServiceInitialize(NULL);
    int32_t r = sceUserServiceGetInitialUser(&g_user);
    if (r < 0) { *detail = r; return -2; }
    r = scePadInit();
    if (r < 0) { *detail = r; return -3; }
    *detail = g_user;
    return 0;
}

/* Try once to open the first controller of the initial user (Rust retries with a pause). */
int ps5_pad_open(int32_t *detail)
{
    if (g_pad >= 0) return 0;
    int32_t h = scePadOpen(g_user, 0, 0, NULL);
    *detail = h;
    if (h < 0) return -4;
    g_pad = h;
    return 0;
}

/* 1: *out is the newest sample since the last read. 0: nothing new. <0: the read failed. */
int ps5_pad_read(struct ps5_pad_state *out)
{
    if (g_pad < 0) return -1;
    int32_t n = scePadRead(g_pad, g_samples, 64);
    if (n == 0) return 0;
    if (n < 0 || n > 64) return -2;
    const struct pad_sample *s = &g_samples[n - 1];
    memset(out, 0, sizeof *out);
    out->buttons = s->buttons;
    out->left_x = s->left_x; out->left_y = s->left_y;
    out->right_x = s->right_x; out->right_y = s->right_y;
    out->left_trigger = s->left_trigger; out->right_trigger = s->right_trigger;
    out->touch_count = s->touch_count;
    out->touch_id = s->touches[0].id;
    out->touch_x = s->touches[0].x; out->touch_y = s->touches[0].y;
    out->connected = s->connected;
    return 1;
}

/* The main output: 256-frame grains of interleaved s16 stereo at 48 kHz. Returns the port (>= 0). */
int32_t ps5_audio_open(int32_t *detail)
{
    int32_t r = sceAudioOutInit();
    if (r != 0 && (uint32_t)r != 0x8026000eu /* already initialised (audio_ps5.cpp) */) { *detail = r; return -1; }
    int32_t port = sceAudioOutOpen(0xff, 0, 0, 256, 48000, 1);
    *detail = port;
    return port < 0 ? -2 : port;
}

int32_t ps5_audio_output(int32_t port, const int16_t *frames)
{
    return sceAudioOutOutput(port, frames);
}
