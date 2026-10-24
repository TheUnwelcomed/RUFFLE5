/* PS5_RUFFLE_LIBCFIX_V2 — libc functions the console refuses to a title, given bodies here.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * V2 (1.000.021): dup3, procctl and dx_host_call, which the link of a build with DirPlayer named as
 * undefined (build/undefined-symbols.txt). They are referenced, not called: Cargo builds
 * ruffle_ps5 as staticlib + rlib, and for that pair it does not run LTO, so every function of a
 * crate reaches the archive and lld (no --gc-sections) keeps whole crate objects. rustix's
 * wrappers (`io::dup3`, the procctl-based `process` calls; rustix comes with async-std's async-io,
 * async-process, async-signal) and DirPlayer's Xtra SDK (`host_env`, the WebAssembly plugin import
 * `dirplayer_xtra_host::dx_host_call`) are in those objects. Each gets a body that behaves as
 * FreeBSD's would, or reports "not supported", in case anything ever does call it.
 *
 * realpath: the console's own answers EPERM in a sandboxed title (milestone 1, ruffle-ps5-plan.md
 * §5d), and std's Path::canonicalize — which Ruffle's navigator uses for its base path — fails on
 * it. A title's paths have no symlinks to resolve, so this one normalises the path lexically
 * (absolute, "." and ".." folded) and checks that it exists. Defined here, it wins over the stub
 * library's export (an executable's own definition binds first) and stays local (version script). */
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define PS5_PATH_MAX 1024

char *realpath(const char *path, char *resolved)
{
    char in[PS5_PATH_MAX], out[PS5_PATH_MAX];
    if (path == NULL) { errno = EINVAL; return NULL; }
    if (path[0] == '/') {
        if (strlen(path) >= sizeof in) { errno = ENAMETOOLONG; return NULL; }
        strcpy(in, path);
    } else {
        /* the title's working directory is /app0 (milestone 1: current_dir = /app0) */
        if (strlen(path) + 6 >= sizeof in) { errno = ENAMETOOLONG; return NULL; }
        strcpy(in, "/app0/");
        strcat(in, path);
    }
    size_t n = 0;
    out[n++] = '/';
    for (char *p = in; *p;) {
        while (*p == '/') p++;
        if (!*p) break;
        char *seg = p;
        while (*p && *p != '/') p++;
        size_t len = (size_t)(p - seg);
        if (len == 1 && seg[0] == '.') continue;
        if (len == 2 && seg[0] == '.' && seg[1] == '.') {
            while (n > 1 && out[n - 1] != '/') n--;   /* drop the last component */
            if (n > 1) n--;                           /* and its slash */
            continue;
        }
        if (n > 1) out[n++] = '/';
        if (n + len >= sizeof out) { errno = ENAMETOOLONG; return NULL; }
        memcpy(out + n, seg, len);
        n += len;
    }
    out[n] = '\0';
    struct stat st;
    if (stat(out, &st) != 0) return NULL;   /* errno from stat: ENOENT for a missing path */
    if (resolved == NULL) {
        resolved = malloc(n + 1);
        if (resolved == NULL) { errno = ENOMEM; return NULL; }
    }
    memcpy(resolved, out, n + 1);
    return resolved;
}

/* dup3(2) (FreeBSD 10+ libc; the SDK's FreeBSD-11 libc.a lacks it): dup2, then close-on-exec if
 * asked. As FreeBSD's: EINVAL for equal descriptors or a flag other than O_CLOEXEC. */
int dup3(int oldfd, int newfd, int flags)
{
    if (oldfd == newfd || (flags & ~O_CLOEXEC) != 0) { errno = EINVAL; return -1; }
    if (dup2(oldfd, newfd) < 0) return -1;
    if ((flags & O_CLOEXEC) && fcntl(newfd, F_SETFD, FD_CLOEXEC) < 0) {
        int e = errno;
        close(newfd);
        errno = e;
        return -1;
    }
    return newfd;
}

/* procctl(2): process-control requests (reapers, tracing, death signals). A title controls no other
 * process; answer as a kernel without the call would. (idtype_t is an int-sized enum, id_t 64-bit.) */
int procctl(int idtype, int64_t id, int cmd, void *data)
{
    (void)idtype; (void)id; (void)cmd; (void)data;
    errno = ENOSYS;
    return -1;
}

/* DirPlayer's Xtra SDK: the host call a WebAssembly Xtra plugin imports from the browser page
 * (xtra-sdk/src/host_env.rs). No plugins run here; 0 is its "void" answer (pointer 0, length 0). */
uint64_t dx_host_call(uint32_t op_id, const uint8_t *args_ptr, uint32_t args_len)
{
    (void)op_id; (void)args_ptr; (void)args_len;
    return 0;
}
