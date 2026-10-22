/* PS5_RUFFLE_LIBCFIX_V1 — libc functions the console refuses to a title, given bodies here.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * realpath: the console's own answers EPERM in a sandboxed title (milestone 1, ruffle-ps5-plan.md
 * §5d), and std's Path::canonicalize — which Ruffle's navigator uses for its base path — fails on
 * it. A title's paths have no symlinks to resolve, so this one normalises the path lexically
 * (absolute, "." and ".." folded) and checks that it exists. Defined here, it wins over the stub
 * library's export (an executable's own definition binds first) and stays local (version script). */
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

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
