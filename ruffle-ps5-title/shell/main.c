/* PS5_RUFFLE_SHELL_V2 — the C shell of the Ruffle PS5 title.
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
 *   5. A toast and a /data/ruffle/ruffle.log for progress, since a title has no stdout anyone sees.
 */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

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

/* ---- a growing log in the title's data folder (FTP: /data/ruffle/ruffle.log) ---- */
#define DATA_DIR "/data/ruffle"
static FILE *g_log;
void ps5_log(const char *msg)
{
    if (g_log) { fprintf(g_log, "%s\n", msg); fflush(g_log); }
    fprintf(stderr, "%s\n", msg);
}

/* Rust entry (src/lib.rs). */
int ruffle_ps5_main(const char *data_dir);

int main(int argc, char **argv)
{
    (void)argc; (void)argv;
    ps5_fp_ieee();
    mkdir(DATA_DIR, 0777);
    g_log = fopen(DATA_DIR "/ruffle.log", "w");
    ps5_log("ruffle-ps5 shell (PS5_RUFFLE_SHELL_V2) up");

    char line[160];
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

    int failures = ruffle_ps5_main(DATA_DIR);
    snprintf(line, sizeof line, "ruffle-ps5 returned %d", failures);
    ps5_log(line);
    if (g_log) fclose(g_log);
    return failures;
}
