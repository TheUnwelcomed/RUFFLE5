/* PS5_RUST_TITLE_STANDIN_V1 — C in the Rust library's place, to test the packaging tail without a
 * Rust toolchain: writes the report file and toasts, like src/lib.rs would. */
#include <stdio.h>
#include <string.h>
int ps5_notify(const char *msg);
int ps5_rust_hello_main(const char *scratch_dir, const char *report_path)
{
    char line[256];
    snprintf(line, sizeof line, "stand-in: C in the Rust library's place (scratch %s)", scratch_dir);
    ps5_notify("Rust Hello (stand-in) is running on this PS5");
    FILE *f = report_path ? fopen(report_path, "w") : NULL;
    if (f) { fputs(line, f); fputc('\n', f); fclose(f); }
    ps5_notify(f ? "Rust Hello stand-in: 1 ok, 0 failed (report written)" : "Rust Hello stand-in: report not written");
    return 0;
}
