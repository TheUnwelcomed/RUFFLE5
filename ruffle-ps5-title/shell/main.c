/* PS5_RUFFLE_SHELL_V1 — the C shell of the Ruffle PS5 title.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Small on purpose: it does the three things that must happen from the title executable itself,
 * then hands control to Rust.
 *   1. sceAgcInit(8) from here — from a dlopen'd module it is refused (PS5_Vulkan docs; and the
 *      payload GL probe confirmed the executable call too, in a payload process). In a title it
 *      succeeds, and RADV tolerates the second call it makes from the archive.
 *   2. Expose the RADV archive's ICD entry (vk_icdGetInstanceProcAddr) to Rust, which builds ash's
 *      Entry from it (src/gpu.rs). PS5_RetroArch resolves the whole API through this symbol.
 *   3. A toast and a /data/ruffle/ruffle.log for progress, since a title has no stdout anyone sees.
 */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>

/* ---- the title's one GPU bring-up call, before any Vulkan ---- */
int32_t sceAgcInit(uint32_t version);

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
}

/* Rust entry (src/lib.rs). */
int ruffle_ps5_main(const char *data_dir);

int main(int argc, char **argv)
{
    (void)argc; (void)argv;
    mkdir(DATA_DIR, 0777);
    g_log = fopen(DATA_DIR "/ruffle.log", "w");

    int32_t agc = sceAgcInit(8);
    char line[160];
    snprintf(line, sizeof line, "sceAgcInit(8) from the title executable = 0x%08x", (unsigned)agc);
    ps5_log(line);
    if (agc != 0) {
        snprintf(line, sizeof line, "ruffle-ps5: sceAgcInit failed 0x%08x", (unsigned)agc);
        ps5_notify(line);
        /* carry on: RADV calls it again and the real reason, if any, surfaces there */
    }

    int failures = ruffle_ps5_main(DATA_DIR);
    snprintf(line, sizeof line, "ruffle-ps5 returned %d", failures);
    ps5_log(line);
    if (g_log) fclose(g_log);
    return failures;
}
