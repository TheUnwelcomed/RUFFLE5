/* PS5_RUST_SHIM_V1 — std on FreeBSD links -lexecinfo; the SDK has no libexecinfo. std's own
 * backtraces go through libunwind (the SDK's libunwind.a, offered as libgcc_s), so these two
 * are never called — they only have to exist for the link. */
#include <stddef.h>
int backtrace(void **buffer, int size) { (void)buffer; (void)size; return 0; }
void backtrace_symbols_fd(void *const *buffer, int size, int fd) { (void)buffer; (void)size; (void)fd; }
