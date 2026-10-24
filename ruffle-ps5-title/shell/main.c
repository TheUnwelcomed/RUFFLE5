/* PS5_RUFFLE_SHELL_V5 — the C shell of the Ruffle PS5 title.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Small on purpose: it does the few things that must happen from the title executable itself,
 * then hands control to Rust.
 *   1. ps5_fp_ieee(): a title starts with flush-to-zero/denormals-are-zero on (MXCSR 0x9fe0); the
 *      platform layer resets the IEEE state every other system starts in (ps5platform/fp.h).
 *      Threads inherit it through the platform's pthread_create wrap.
 *   2. sceAgcInit(8) from here, before any Vulkan. RADV's PS5 winsys calls it again from the
 *      archive; a second call is tolerated (PS5_Vulkan docs/M5_PHASE_B.md). Logged, not fatal.
 *   3. sceSystemServiceHideSplashScreen(): the system's splash sits over the frame the title is
 *      about to present into (PS5_RetroArch src/display.cpp, PS5_Vulkan src/demo_renderer.cpp).
 *   4. Expose the RADV archive's ICD entry (vk_icdGetInstanceProcAddr) to Rust, which builds ash's
 *      Entry from it (src/gpu.rs). PS5_RetroArch resolves the whole API through this symbol.
 *   5. A toast and a /data/ruffle5/ruffle.log for progress, since a title has no stdout anyone sees.
 * V4: Ruffle5's own data folder, /data/ruffle5 (the other Flash player for the PS5 uses /data/ruffle:
 * the two no longer overwrite each other's log), the "Custom Mappings" folder beside games/, and the
 * console's local hour (badges).
 * V5 (1.000.022): the USB keyboard and mouse system modules load first thing in main(), before the
 * title opens /data or touches the GPU (shell/hid.c V2), and their answers are logged.
 */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>

/* ---- platform layer (libps5platform.a, the SDK fork PS5_Vulkan builds against) ---- */
void ps5_fp_ieee(void);

/* ---- system modules ---- */
int32_t sceAgcInit(uint32_t version);
int sceSystemServiceHideSplashScreen(void);

/* ---- the RADV archive's ICD entry (linked whole by build-title.sh) ---- */
void *vk_icdGetInstanceProcAddr(void *instance, const char *name);
const void *ps5_vk_get_instance_proc_addr(void)
{
    return (const void *)vk_icdGetInstanceProcAddr;
}

/* ---- a toast on the TV ---- */
typedef struct notify_request { char useless1[45]; char message[3075]; } notify_request_t;
int sceKernelSendNotificationRequest(int, notify_request_t *, size_t, int);
void ps5_notify(const char *msg)
{
    notify_request_t req;
    memset(&req, 0, sizeof req);
    strncpy(req.message, msg, sizeof req.message - 1);
    sceKernelSendNotificationRequest(0, &req, sizeof req, 0);
}

/* ---- a log in the title's data folder (FTP: /data/ruffle5/ruffle.log) ---- */
#define DATA_DIR "/data/ruffle5"
/* Where the menu looks for games, and where every controls mapping lives: the title's own folder
 * (PPSA99620 is sce_sys/param.json's titleId; build-title.sh checks the two agree). */
#define GAMES_DIR "/data/homebrew/PPSA99620/games"
#define MAPPINGS_DIR "/data/homebrew/PPSA99620/Custom Mappings"
static FILE *g_log;
void ps5_log(const char *msg)
{
    if (g_log) { fprintf(g_log, "%s\n", msg); fflush(g_log); }
    fprintf(stderr, "%s\n", msg);
}

/* The console's local hour, 0-23 (-1 if the clock cannot be read). */
int ps5_local_hour(void)
{
    time_t now = time(NULL);
    struct tm tm;
    if (now == (time_t)-1 || !localtime_r(&now, &tm))
        return -1;
    return tm.tm_hour;
}

/* USB keyboard and mouse system modules (shell/hid.c). */
void ps5_hid_preload(void);
int32_t ps5_hid_sysmodule_rc(int which);

/* Rust entry (src/lib.rs). */
int ruffle_ps5_main(const char *data_dir, const char *games_dir, const char *mappings_dir);

int main(int argc, char **argv)
{
    (void)argc; (void)argv;
    ps5_fp_ieee();
    /* before /data and the GPU: see shell/hid.c */
    ps5_hid_preload();
    mkdir(DATA_DIR, 0777);
    g_log = fopen(DATA_DIR "/ruffle.log", "w");
    ps5_log("ruffle-ps5 shell (PS5_RUFFLE_SHELL_V5) up");

    char line[160];
    snprintf(line, sizeof line, "usb: sysmodule keyboard 0x0106 = 0x%08x, mouse 0x00a9 = 0x%08x (loaded first)",
             (unsigned)ps5_hid_sysmodule_rc(0), (unsigned)ps5_hid_sysmodule_rc(1));
    ps5_log(line);
    int32_t agc = sceAgcInit(8);
    snprintf(line, sizeof line, "sceAgcInit(8) from the title executable = 0x%08x", (unsigned)agc);
    ps5_log(line);
    if (agc != 0) {
        snprintf(line, sizeof line, "ruffle-ps5: sceAgcInit failed 0x%08x", (unsigned)agc);
        ps5_notify(line);
        /* carry on: RADV calls it again and the real reason, if any, surfaces there */
    }

    int hidden = sceSystemServiceHideSplashScreen();
    snprintf(line, sizeof line, "sceSystemServiceHideSplashScreen = 0x%08x", (unsigned)hidden);
    ps5_log(line);

    mkdir(GAMES_DIR, 0777);
    mkdir(MAPPINGS_DIR, 0777);
    int failures = ruffle_ps5_main(DATA_DIR, GAMES_DIR, MAPPINGS_DIR);
    snprintf(line, sizeof line, "ruffle-ps5 returned %d", failures);
    ps5_log(line);
    if (g_log) fclose(g_log);
    return failures;
}
