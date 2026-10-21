/* PS5_RUST_TITLE_SHELL_V1 — the C entry of the "Rust Hello" title.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * The title's CRT (native-app/app_crt.cpp) runs the init arrays and calls main. main checks the
 * import table build-title.sh generated (the console's loader binds an import it does not export
 * to NULL, silently — a crash later with no message), then hands over to the Rust library, which
 * runs the same ten checks as the payload, writes its report under /app0/ps5-rust-hello, and toasts the tally.
 */
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <errno.h>

int ps5_rust_hello_main(const char *scratch_dir, const char *report_path);   /* Rust, src/lib.rs */
int ps5_notify(const char *msg);                                              /* shim */
int ps5_import_count(void);                                                   /* generated */
const char *ps5_import_null(int i);                                           /* generated */
int sceNetInit(void);                                                         /* libSceNet */

/* Where the report goes. /app0 is the running title's own folder — with ShadowMount+ that is
 * /data/homebrew/PPSA99610 over FTP, where PS5_RetroArch keeps its content and saves too.
 * /download0 is the title's private download area, not visible from FTP's root. */
#define DIR_APP0 "/app0/ps5-rust-hello"
#define DIR_DL0  "/download0/ps5-rust-hello"

int main(int argc, char **argv)
{
    (void)argc; (void)argv;
    char msg[256];
    int unbound = 0;
    for (int i = 0; i < ps5_import_count(); i++) {
        const char *name = ps5_import_null(i);
        if (!name) continue;
        if (unbound == 0) snprintf(msg, sizeof msg, "Rust Hello: import not bound by the loader: %s", name);
        unbound++;
    }
    if (unbound) {
        ps5_notify(msg);
        if (unbound > 1) { snprintf(msg, sizeof msg, "Rust Hello: %d imports unbound in all", unbound); ps5_notify(msg); }
    }
    /* BSD sockets in a title answer EACCES until the process has done sceNetInit (PS5_RetroArch's
     * scraper and WebUI do the same before their first socket; Rust Hello's net check got
     * "Permission denied" without it, 2026-10-20). */
    int net = sceNetInit();
    snprintf(msg, sizeof msg, "Rust Hello: sceNetInit = 0x%x", net);
    ps5_notify(msg);
    const char *dir = DIR_APP0;
    if (mkdir(DIR_APP0, 0777) != 0 && errno != EEXIST) { dir = DIR_DL0; mkdir(DIR_DL0, 0777); }
    char report[256];
    snprintf(report, sizeof report, "%s/report.txt", dir);
    snprintf(msg, sizeof msg, "Rust Hello: report goes to %s", dir);
    ps5_notify(msg);
    return ps5_rust_hello_main(dir, report);
}
