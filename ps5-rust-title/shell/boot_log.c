/* PS5_RUST_TITLE_BOOTLOG_V1 — evo_dns.c (the console resolver's getaddrinfo) logs through a weak
 * evo_boot_log; the converter wants every import defined, so here it is: one line per call into
 * /download0/ps5-rust-hello/boot.log (and stderr). */
#include <stdarg.h>
#include <stdio.h>
void evo_boot_log(const char *fmt, ...)
{
    static FILE *f;
    if (!f) f = fopen("/app0/ps5-rust-hello/boot.log", "a");
    if (!f) f = fopen("/download0/ps5-rust-hello/boot.log", "a");
    va_list ap;
    va_start(ap, fmt); vfprintf(stderr, fmt, ap); fputc('\n', stderr); va_end(ap);
    if (f) { va_start(ap, fmt); vfprintf(f, fmt, ap); fputc('\n', f); fflush(f); va_end(ap); }
}
