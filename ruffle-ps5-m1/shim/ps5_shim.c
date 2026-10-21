/* PS5_RUST_SHIM_V4 — the C side of Rust-on-PS5 (ps5-payload-dev SDK).
 * v2 (2026-10-20): title directories read with getdents/64 KiB (as PS5_RetroArch), and probes that
 *     name the failing call. getdirentries(16 KiB) is EINVAL on /app0/…; getdents(64 KiB) works.
 * v3 (2026-10-20): the probe showed socket(|SOCK_CLOEXEC) works in a title and ioctl(FIONBIO) is
 *     EACCES — std's set_nonblocking. v2's socket wrap is gone (it only added an fcntl that gave
 *     EINVAL); FIONBIO goes through fcntl(O_NONBLOCK) instead (--wrap=ioctl, title mode).
 * v4 (2026-10-20): v9 showed fcntl(F_SETFL, O_NONBLOCK) is EACCES too (F_GETFL then reports 0x6:
 *     FreeBSD's F_SETFL sets f_flag first and only then calls the socket's FIONBIO, which the
 *     sandbox refuses — the socket stays blocking, connect returned 0). Same result as Mihawk's
 *     measurement on 13.40 (PS5_RetroArch evidence/network-elevation: EACCES in the sandbox, works
 *     after Lapy elevation). The route a retail game has is libSceNet's SCE_NET_SO_NBIO, a socket
 *     option (0x1200 at SOL_SOCKET, shadPS4 net_types.h), not an ioctl: FIONBIO is turned into
 *     that here, and the probe measures it raw and through sceNetSetsockopt.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * Three jobs, all verified against the SDK's exported symbols (see README, "What the SDK has"):
 *  1. The libc crate, built with --cfg libc_unstable_freebsd_version="11", asks for FreeBSD's
 *     versioned compat names (fstat@FBSD_1.0 …). The console's plain fstat/stat/readdir/… ARE
 *     the FreeBSD-11 layout, so each versioned name is a thin wrapper over the plain one.
 *     lld resolves a `.symver` definition against the literal reference rustc emits
 *     (checked with the SDK's clang 18 / lld 18: the reference binds to the wrapper, the
 *     wrapper calls the import through the GOT — no recursion).
 *  2. A few functions std references that the SDK does not export (accept4, getpeereid,
 *     elf_aux_info, pthread_setname_np). Only the ones a program uses are pulled in.
 *  3. Small helpers for the milestone-1 test: a notification on the TV, and C-side stat /
 *     readdir counts to check the Rust side's struct layouts against.
 */
#include <sys/types.h>
#include <sys/stat.h>
#include <sys/socket.h>
#include <sys/mount.h>
#include <sys/event.h>
#include <sys/wait.h>
#include <dirent.h>
#include <fcntl.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <pthread.h>
#include <pthread_np.h>
#include <unistd.h>

/* ---- 1. versioned FreeBSD names -> the console's plain functions ----------------------- */
#define FBSD_ALIAS(ret, name, ver, params, args) \
    ret ps5_##name##_compat params { return name args; } \
    __asm__(".symver ps5_" #name "_compat, " #name "@" ver)

int fstat(int, struct stat *);
int stat(const char *, struct stat *);
int lstat(const char *, struct stat *);
int fstatat(int, const char *, struct stat *, int);
int mknod(const char *, mode_t, dev_t);
int mknodat(int, const char *, mode_t, dev_t);
struct dirent *readdir(DIR *);
int readdir_r(DIR *, struct dirent *, struct dirent **);
int statfs(const char *, struct statfs *);
int fstatfs(int, struct statfs *);
int getmntinfo(struct statfs **, int);
int kevent(int, const struct kevent *, int, struct kevent *, int, const struct timespec *);
pid_t wait4(pid_t, int *, int, struct rusage *);
int rand(void);
void srand(unsigned);

FBSD_ALIAS(int, fstat, "FBSD_1.0", (int fd, struct stat *st), (fd, st));
FBSD_ALIAS(int, stat, "FBSD_1.0", (const char *p, struct stat *st), (p, st));
FBSD_ALIAS(int, lstat, "FBSD_1.0", (const char *p, struct stat *st), (p, st));
FBSD_ALIAS(int, fstatat, "FBSD_1.1", (int fd, const char *p, struct stat *st, int fl), (fd, p, st, fl));
FBSD_ALIAS(int, mknod, "FBSD_1.0", (const char *p, mode_t m, dev_t d), (p, m, d));
FBSD_ALIAS(int, mknodat, "FBSD_1.1", (int fd, const char *p, mode_t m, dev_t d), (fd, p, m, d));
FBSD_ALIAS(struct dirent *, readdir, "FBSD_1.0", (DIR *d), (d));
FBSD_ALIAS(int, readdir_r, "FBSD_1.0", (DIR *d, struct dirent *e, struct dirent **r), (d, e, r));
FBSD_ALIAS(int, statfs, "FBSD_1.0", (const char *p, struct statfs *s), (p, s));
FBSD_ALIAS(int, fstatfs, "FBSD_1.0", (int fd, struct statfs *s), (fd, s));
FBSD_ALIAS(int, getmntinfo, "FBSD_1.0", (struct statfs **m, int fl), (m, fl));
FBSD_ALIAS(int, kevent, "FBSD_1.0", (int kq, const struct kevent *in, int nin, struct kevent *out, int nout, const struct timespec *t), (kq, in, nin, out, nout, t));
FBSD_ALIAS(pid_t, wait4, "FBSD_1.0", (pid_t p, int *st, int o, struct rusage *r), (p, st, o, r));
FBSD_ALIAS(int, rand, "FBSD_1.0", (void), ());
FBSD_ALIAS(void, srand, "FBSD_1.0", (unsigned s), (s));
/* glob@FBSD_1.0 / globfree@FBSD_1.0 are left out: the SDK has no glob, and std never calls it. */
/* the post-ino64 name, in case a crate is built without the freebsd11 cfg */
int ps5_readdir_r_15(DIR *d, struct dirent *e, struct dirent **r) { return readdir_r(d, e, r); }
__asm__(".symver ps5_readdir_r_15, readdir_r@FBSD_1.5");

/* ---- 2. functions std references that the SDK does not export ------------------------- */
#ifndef SOCK_CLOEXEC
#define SOCK_CLOEXEC 0x10000000
#endif
#ifndef SOCK_NONBLOCK
#define SOCK_NONBLOCK 0x20000000
#endif
/* TcpListener::accept on FreeBSD: accept4(fd, addr, len, SOCK_CLOEXEC). The console has accept. */
int accept4(int fd, struct sockaddr *addr, socklen_t *len, int flags)
{
    int s = accept(fd, addr, len);
    if (s < 0) return s;
    if (flags & SOCK_CLOEXEC) fcntl(s, F_SETFD, FD_CLOEXEC);
    if (flags & SOCK_NONBLOCK) fcntl(s, F_SETFL, fcntl(s, F_GETFL, 0) | O_NONBLOCK);
    return s;
}
/* UnixStream::peer_cred — no unix-socket peer credentials here */
int getpeereid(int s, uid_t *euid, gid_t *egid) { (void)s; (void)euid; (void)egid; errno = ENOSYS; return -1; }
/* std_detect's AT_HWCAP lookup — "not found" makes it fall back to cpuid, which is what we want */
int elf_aux_info(int aux, void *buf, int buflen) { (void)aux; (void)buf; (void)buflen; errno = ENOENT; return ENOENT; }
/* std's process-group kill (Child::kill in unix.rs); libc's killpg is kill(-pgrp). The payload link
 * never needs it (--gc-sections drops the code), the title link has no gc-sections and does. */
#include <signal.h>
int killpg(pid_t pgrp, int sig) { return kill(-pgrp, sig); }
/* std's Command::current_dir looks these up as weak symbols (true weak linkage on ELF targets).
 * A payload loader binds an absent weak symbol to NULL; the title converter refuses any undefined
 * symbol no stub exports, weak or not. ENOSYS = "not here" (posix_spawn actions return errno). */
#include <spawn.h>
int posix_spawn_file_actions_addchdir_np(posix_spawn_file_actions_t *fa, const char *path) { (void)fa; (void)path; return ENOSYS; }
int posix_spawn_file_actions_addchdir(posix_spawn_file_actions_t *fa, const char *path) { (void)fa; (void)path; return ENOSYS; }
/* Linux spelling; std on FreeBSD uses pthread_set_name_np, which the SDK has */
int pthread_setname_np(pthread_t t, const char *name) { pthread_set_name_np(t, name); return 0; }

/* std's backtrace symbolizer walks loaded objects with dl_iterate_phdr; the SDK has none, and a
 * payload has no file on disk to read symbols from anyway. "No objects" = addresses only. */
struct dl_phdr_info;
int dl_iterate_phdr(int (*cb)(struct dl_phdr_info *, unsigned long, void *), void *data) { (void)cb; (void)data; return 0; }

/* ---- 3. helpers for the milestone-1 test ---------------------------------------------- */
typedef struct notify_request { char useless1[45]; char message[3075]; } notify_request_t;
int sceKernelSendNotificationRequest(int, notify_request_t *, size_t, int);

/* a toast on the TV, like the SDK's notify sample */
int ps5_notify(const char *msg)
{
    notify_request_t req;
    memset(&req, 0, sizeof req);
    strncpy(req.message, msg, sizeof req.message - 1);
    return sceKernelSendNotificationRequest(0, &req, sizeof req, 0);
}
/* what the console's own stat says the file size is (Rust compares its metadata().len() to this) */
long long ps5_c_stat_size(const char *path)
{
    struct stat st;
    if (stat(path, &st) != 0) return -1;
    return (long long)st.st_size;
}
/* how many entries the console's own readdir sees (Rust compares read_dir().count() to this);
 * -errno when opendir fails or readdir stops on an error */
long ps5_c_dir_count(const char *path)
{
    DIR *d = opendir(path);
    if (!d) return -errno;
    long n = 0;
    errno = 0;
    while (readdir(d)) { n++; errno = 0; }
    int e = errno;
    closedir(d);
    return e ? -e : n;
}
long ps5_c_sizeof_stat(void) { return (long)sizeof(struct stat); }
long ps5_c_sizeof_dirent(void) { return (long)sizeof(struct dirent); }

/* ---- 3b. probes: the primitive calls one at a time, each with its errno, into a string -----
 * v7 on the console (2026-10-20): read_dir gave one error and no names, the C count was 0, and
 * connect_timeout said "Permission denied" — neither report said which call. These do. */
#include <sys/ioctl.h>
#include <netinet/in.h>
#include <poll.h>
#define PROBE_PUT(...) do { int w_ = snprintf(out + n, n < (int)size ? size - (size_t)n : 0, __VA_ARGS__); if (w_ > 0) n += w_; } while (0)
#define PROBE_ERR(r) do { if ((r) < 0) PROBE_PUT("(errno %d)", errno); } while (0)
#ifdef PS5_TITLE
int __real_ioctl(int, unsigned long, ...);  /* the import itself; __wrap_ioctl is below */
#define PROBE_IOCTL __real_ioctl
#else
#define PROBE_IOCTL ioctl
#endif
/* open O_DIRECTORY, then getdirentries with 16 KiB (v1..v7's reader) and getdents with 64 KiB
 * (PS5_RetroArch's reader, platform/src/directory.c of Mihawk's SDK fork), then the names */
long ps5_c_dir_probe(const char *path, char *out, size_t size)
{
    int n = 0;
    char *buf = (char *)malloc(64 * 1024);
    if (!buf) return -1;
    int fd = open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    PROBE_PUT("open(O_DIRECTORY)=%d", fd); PROBE_ERR(fd);
    if (fd < 0) { free(buf); return -1; }
    long base = 0;
    int r1 = getdirentries(fd, buf, 16384, &base);
    PROBE_PUT(" getdirentries(16K)=%d", r1); PROBE_ERR(r1);
    lseek(fd, 0, SEEK_SET);
    int r2 = getdents(fd, buf, 64 * 1024);
    PROBE_PUT(" getdents(64K)=%d", r2); PROBE_ERR(r2);
    int got = r2 > 0 ? r2 : 0;
    if (r2 <= 0 && r1 > 0) { lseek(fd, 0, SEEK_SET); base = 0; got = getdirentries(fd, buf, 16384, &base); if (got < 0) got = 0; }
    PROBE_PUT(" names:");
    long count = 0;
    for (int pos = 0; pos + 8 <= got;) {
        /* FreeBSD-11 record: u32 fileno, u16 reclen, u8 type, u8 namlen, name, NUL */
        unsigned reclen = (unsigned char)buf[pos + 4] | ((unsigned char)buf[pos + 5] << 8);
        unsigned namlen = (unsigned char)buf[pos + 7];
        if (reclen < 8 + namlen + 1 || pos + (int)reclen > got) { PROBE_PUT(" [bad record at %d]", pos); break; }
        PROBE_PUT(" %.*s", (int)namlen, buf + pos + 8);
        count++;
        pos += (int)reclen;
    }
    close(fd);
    free(buf);
    return count;
}
/* Sony's own non-blocking switch, the one libSceNet gives a game: SCE_NET_SO_NBIO = 0x1200 at
 * SOL_SOCKET (shadPS4, src/core/libraries/net/net_types.h: ORBIS_NET_SOL_SOCKET 0xFFFF,
 * ORBIS_NET_SO_NBIO 0x1200; settable and gettable on every socket kind). */
#define PS5_SO_NBIO 0x1200
#include <sys/event.h>
int sceNetSetsockopt(int s, int level, int optname, const void *optval, unsigned int optlen);
static void probe_addr(struct sockaddr_in *sa, const char *ip, int port)
{
    memset(sa, 0, sizeof *sa);
    sa->sin_family = AF_INET;
    sa->sin_port = htons((unsigned short)port);
    unsigned q[4] = {0, 0, 0, 0};              /* dotted quad by hand: no inet_addr import needed */
    if (sscanf(ip, "%u.%u.%u.%u", &q[0], &q[1], &q[2], &q[3]) != 4) return;
    unsigned char *ap = (unsigned char *)&sa->sin_addr.s_addr;
    ap[0] = (unsigned char)q[0]; ap[1] = (unsigned char)q[1]; ap[2] = (unsigned char)q[2]; ap[3] = (unsigned char)q[3];
}
/* connect, poll(POLLOUT), SO_ERROR — on a non-blocking socket connect is -1/EINPROGRESS (36);
 * on a blocking one it is 0 (that is the tell). 0 = connected. */
static long probe_connect(int fd, const struct sockaddr_in *sa, char *out, size_t size, int *np)
{
    int n = *np;
    long ok = -1;
    int r = connect(fd, (const struct sockaddr *)sa, sizeof *sa);
    PROBE_PUT(" connect=%d", r); PROBE_ERR(r);
    if (r == 0 || errno == EINPROGRESS) {
        struct pollfd p = { fd, POLLOUT, 0 };
        r = poll(&p, 1, 5000);
        PROBE_PUT(" poll=%d", r); PROBE_ERR(r);
        if (r > 0) {
            int err = 0; socklen_t len = sizeof err;
            r = getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &len);
            PROBE_PUT(" SO_ERROR=%d", err); PROBE_ERR(r);
            if (r == 0 && err == 0) ok = 0;
        } else if (r == 0) PROBE_PUT("(timeout)");
    }
    *np = n;
    return ok;
}
/* Four sockets, one question each, every call with its errno:
 *  A  setsockopt(SOL_SOCKET, SO_NBIO, 1) through libkernel, then a non-blocking connect
 *  B  the same through libSceNet's sceNetSetsockopt (SCE error code if negative)
 *  C  the BSD ways, for the record: F_GETFD/F_SETFD, ioctl(FIONBIO) raw, F_SETFL(O_NONBLOCK),
 *     then ioctl(FIONBIO) through the wrap (a title) and a connect
 *  D  kqueue + kevent on a connected socket (what mio/tokio would need)
 * v9 on the console: ioctl(FIONBIO) and F_SETFL(O_NONBLOCK) both -1/errno 13, connect=0 after. */
long ps5_c_net_probe(const char *ip, int port, char *out, size_t size)
{
    int n = 0, r;
    struct sockaddr_in sa;
    probe_addr(&sa, ip, port);
    long ok_a = -1, ok_b = -1, ok_c = -1;
    int one = 1, zero = 0;

    int a = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    PROBE_PUT("A socket=%d", a); PROBE_ERR(a);
    if (a >= 0) {
        int got = -1; socklen_t len = sizeof got;
        r = setsockopt(a, SOL_SOCKET, PS5_SO_NBIO, &one, sizeof one);
        PROBE_PUT(" setsockopt(SO_NBIO,1)=%d", r); PROBE_ERR(r);
        r = getsockopt(a, SOL_SOCKET, PS5_SO_NBIO, &got, &len);
        PROBE_PUT(" getsockopt(SO_NBIO)=%d", r); PROBE_ERR(r); if (r == 0) PROBE_PUT("->%d", got);
        ok_a = probe_connect(a, &sa, out, size, &n);
        r = setsockopt(a, SOL_SOCKET, PS5_SO_NBIO, &zero, sizeof zero);
        PROBE_PUT(" setsockopt(SO_NBIO,0)=%d", r); PROBE_ERR(r);
        close(a);
    }

    int b = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    PROBE_PUT(" | B socket=%d", b); PROBE_ERR(b);
    if (b >= 0) {
        r = sceNetSetsockopt(b, 0xffff, PS5_SO_NBIO, &one, sizeof one);
        if (r < 0) PROBE_PUT(" sceNetSetsockopt(SO_NBIO,1)=0x%08x", (unsigned)r); else PROBE_PUT(" sceNetSetsockopt(SO_NBIO,1)=%d", r);
        ok_b = probe_connect(b, &sa, out, size, &n);
        close(b);
    }

    int c = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
    PROBE_PUT(" | C socket=%d", c); PROBE_ERR(c);
    if (c >= 0) {
        r = fcntl(c, F_GETFD);
        PROBE_PUT(" F_GETFD=%d", r); PROBE_ERR(r);
        r = fcntl(c, F_SETFD, FD_CLOEXEC);
        PROBE_PUT(" F_SETFD=%d", r); PROBE_ERR(r);
        r = PROBE_IOCTL(c, FIONBIO, &one);
        PROBE_PUT(" ioctl(FIONBIO)=%d", r); PROBE_ERR(r);
        int fl = fcntl(c, F_GETFL);
        PROBE_PUT(" F_GETFL=0x%x", fl); PROBE_ERR(fl);
        if (fl >= 0) {
            r = fcntl(c, F_SETFL, fl | O_NONBLOCK);
            PROBE_PUT(" F_SETFL(O_NONBLOCK)=%d", r); PROBE_ERR(r);
            fl = fcntl(c, F_GETFL);
            PROBE_PUT(" F_GETFL=0x%x", fl); PROBE_ERR(fl);
        }
        r = ioctl(c, FIONBIO, &one);              /* through __wrap_ioctl in a title */
        PROBE_PUT(" wrapped-FIONBIO=%d", r); PROBE_ERR(r);
        ok_c = probe_connect(c, &sa, out, size, &n);

        int kq = kqueue();
        PROBE_PUT(" | D kqueue=%d", kq); PROBE_ERR(kq);
        if (kq >= 0) {
            struct kevent ev, got;
            struct timespec ts = { 1, 0 };
            EV_SET(&ev, c, EVFILT_WRITE, EV_ADD | EV_ONESHOT, 0, 0, 0);
            r = kevent(kq, &ev, 1, &got, 1, &ts);
            PROBE_PUT(" kevent(EVFILT_WRITE)=%d", r); PROBE_ERR(r);
            if (r > 0) PROBE_PUT("(ident=%d filter=%d)", (int)got.ident, (int)got.filter);
            close(kq);
        }
        close(c);
    }
    PROBE_PUT(" => A %s, B %s, C %s", ok_a == 0 ? "connected" : "no", ok_b == 0 ? "connected" : "no", ok_c == 0 ? "connected" : "no");
    return ok_a == 0 || ok_b == 0 || ok_c == 0 ? 0 : -1;
}

/* ---- 4. title mode (-DPS5_TITLE): no raw syscalls -------------------------------------------
 * The SDK's static libc.a implements some POSIX calls with the `syscall` instruction itself
 * (mman.o, mount.o, syscalls.o). A payload may do that; a title may not — the console kills it
 * with SYSTEM_ILLEGAL_FUNCTION_CALL (0xa002030a). Seen 2026-10-20: std's thread::sleep calls
 * clock_nanosleep, libc.a's is a raw syscall, the title died right after the first toast.
 * Everything std can reach that way is given a libkernel-backed body here, or ENOSYS.
 * The shim is first on the link line, so these win over libc.a's. */
#ifdef PS5_TITLE
#include <stdarg.h>
#include <poll.h>
#include <unistd.h>
#include <time.h>
#include <sys/time.h>
#include <sys/resource.h>
#ifndef TIMER_ABSTIME
#define TIMER_ABSTIME 1
#endif
#ifndef AT_FDCWD
#define AT_FDCWD -100
#endif
#ifndef AT_SYMLINK_NOFOLLOW
#define AT_SYMLINK_NOFOLLOW 0x200
#endif
#ifndef UTIME_NOW
#define UTIME_NOW -1
#define UTIME_OMIT -2
#endif
int clock_gettime(clockid_t, struct timespec *);
int nanosleep(const struct timespec *, struct timespec *);
int futimes(int, const struct timeval *);
int utimes(const char *, const struct timeval *);
int chmod(const char *, mode_t);
pid_t getpid(void);

/* std's sleep: clock_nanosleep(CLOCK_MONOTONIC, 0, rq, rm); sleep_until uses TIMER_ABSTIME */
int clock_nanosleep(clockid_t clk, int flags, const struct timespec *rq, struct timespec *rm)
{
    if (!rq) return EINVAL;
    struct timespec rel = *rq;
    if (flags & TIMER_ABSTIME) {
        struct timespec now;
        if (clock_gettime(clk, &now) != 0) return errno;
        rel.tv_sec = rq->tv_sec - now.tv_sec; rel.tv_nsec = rq->tv_nsec - now.tv_nsec;
        if (rel.tv_nsec < 0) { rel.tv_nsec += 1000000000L; rel.tv_sec--; }
        if (rel.tv_sec < 0) return 0;
        rm = 0;
    }
    return nanosleep(&rel, rm) == 0 ? 0 : errno;
}
/* std: DirEntry::metadata, symlink_metadata via fstatat(AT_FDCWD, path, st, flags) */
int fstatat(int fd, const char *path, struct stat *st, int flags)
{
    if (fd != AT_FDCWD && path[0] != '/') { errno = ENOSYS; return -1; }
    return (flags & AT_SYMLINK_NOFOLLOW) ? lstat(path, st) : stat(path, st);
}
static int ps5_ts_to_tv(const struct timespec ts[2], struct timeval tv[2])
{
    for (int i = 0; i < 2; i++) {
        if (ts[i].tv_nsec == UTIME_OMIT) { errno = ENOSYS; return -1; }   /* needs the old value; not here */
        if (ts[i].tv_nsec == UTIME_NOW) { struct timespec now; clock_gettime(CLOCK_REALTIME, &now); tv[i].tv_sec = now.tv_sec; tv[i].tv_usec = now.tv_nsec / 1000; }
        else { tv[i].tv_sec = ts[i].tv_sec; tv[i].tv_usec = ts[i].tv_nsec / 1000; }
    }
    return 0;
}
int futimens(int fd, const struct timespec ts[2])
{
    if (!ts) return futimes(fd, 0);
    struct timeval tv[2]; if (ps5_ts_to_tv(ts, tv)) return -1;
    return futimes(fd, tv);
}
int utimensat(int fd, const char *path, const struct timespec ts[2], int flags)
{
    (void)flags;
    if (fd != AT_FDCWD && path[0] != '/') { errno = ENOSYS; return -1; }
    if (!ts) return utimes(path, 0);
    struct timeval tv[2]; if (ps5_ts_to_tv(ts, tv)) return -1;
    return utimes(path, tv);
}
int fchmodat(int fd, const char *path, mode_t mode, int flags)
{
    (void)flags;
    if (fd != AT_FDCWD && path[0] != '/') { errno = ENOSYS; return -1; }
    return chmod(path, mode);
}
pid_t getpgid(pid_t p) { (void)p; return getpid(); }
pid_t getpgrp(void) { return getpid(); }
/* the *at family on AT_FDCWD or an absolute path maps to the plain call; a real directory fd has
 * no libkernel route here (no F_GETPATH), so that case says ENOSYS instead of trapping */
#ifndef AT_REMOVEDIR
#define AT_REMOVEDIR 0x800
#endif
int open(const char *, int, ...);
int unlink(const char *); int rmdir(const char *); int mkdir(const char *, mode_t);
int rename(const char *, const char *); int pipe(int[2]); int access(const char *, int);
int sched_yield(void);
#define PS5_AT_OK(fd, path) ((fd) == AT_FDCWD || (path)[0] == '/')
int openat(int fd, const char *path, int flags, ...)
{
    if (!PS5_AT_OK(fd, path)) { errno = ENOSYS; return -1; }
    mode_t mode = 0;
    if (flags & O_CREAT) { va_list ap; va_start(ap, flags); mode = (mode_t)va_arg(ap, int); va_end(ap); }
    return open(path, flags, mode);
}
int unlinkat(int fd, const char *path, int flags)
{
    if (!PS5_AT_OK(fd, path)) { errno = ENOSYS; return -1; }
    return (flags & AT_REMOVEDIR) ? rmdir(path) : unlink(path);
}
int mkdirat(int fd, const char *path, mode_t mode)
{
    if (!PS5_AT_OK(fd, path)) { errno = ENOSYS; return -1; }
    return mkdir(path, mode);
}
int renameat(int fa, const char *a, int fb, const char *b)
{
    if (!PS5_AT_OK(fa, a) || !PS5_AT_OK(fb, b)) { errno = ENOSYS; return -1; }
    return rename(a, b);
}
int faccessat(int fd, const char *path, int mode, int flags)
{
    (void)flags;
    if (!PS5_AT_OK(fd, path)) { errno = ENOSYS; return -1; }
    return access(path, mode);
}
/* std's anonymous pipes: pipe2(fds, O_CLOEXEC) */
int pipe2(int fds[2], int flags)
{
    if (pipe(fds) != 0) return -1;
    for (int i = 0; i < 2; i++) {
        if (flags & O_CLOEXEC) fcntl(fds[i], F_SETFD, FD_CLOEXEC);
        if (flags & O_NONBLOCK) fcntl(fds[i], F_SETFL, fcntl(fds[i], F_GETFL, 0) | O_NONBLOCK);
    }
    return 0;
}
int lutimes(const char *path, const struct timeval tv[2]) { return utimes(path, tv); }
int lchmod(const char *path, mode_t mode) { return chmod(path, mode); }
mode_t umask(mode_t m) { (void)m; return 022; }
int yield(void) { return sched_yield(); }
int posix_fadvise(int fd, off_t off, off_t len, int advice) { (void)fd; (void)off; (void)len; (void)advice; return 0; }
int ppoll(struct pollfd *fds, nfds_t n, const struct timespec *to, const sigset_t *sigmask)
{
    (void)sigmask;
    int ms = to ? (int)(to->tv_sec * 1000 + to->tv_nsec / 1000000) : -1;
    return poll(fds, n, ms);
}
/* no equivalent in libkernel: say so instead of trapping */
/* ---- what a title's own libc refuses (PS5_RetroArch's findings, src/platform_wraps.c, confirmed
 * here 2026-10-20): libSceLibcInternal's opendir is refused, its getcwd jumps to address 0
 * (it needs __getcwd, which only the system-process kernel library exports), realpath is EPERM.
 * Directories are read on an O_DIRECTORY fd the way PS5_RetroArch's title reads them
 * (platform/src/directory.c in Mihawk's SDK fork): libkernel's getdents into a 64 KiB buffer —
 * "mounted title directories need a larger read than the synthetic root". v1..v7 used
 * getdirentries with 16 KiB and got an error on /app0/ps5-rust-hello (2026-10-20: read_dir gave
 * one Err, the C count 0; ps5_c_dir_probe now records both calls' errno). The cwd is /app0. */
struct ps5_dir { int fd; int len; int pos; char buf[64 * 1024]; };
DIR *fdopendir(int fd)
{
    struct ps5_dir *d = (struct ps5_dir *)calloc(1, sizeof *d);
    if (!d) return 0;
    d->fd = fd;
    return (DIR *)d;
}
DIR *opendir(const char *path)
{
    int fd = open(path, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (fd < 0) return 0;
    DIR *d = fdopendir(fd);
    if (!d) { int e = errno; close(fd); errno = e; }
    return d;
}
struct dirent *readdir(DIR *dir)
{
    struct ps5_dir *d = (struct ps5_dir *)dir;
    for (;;) {
        if (d->pos >= d->len) {
            int n = getdents(d->fd, d->buf, (int)sizeof d->buf);
            if (n <= 0) return 0;   /* n < 0: errno is getdents'; std reads it after the NULL */
            d->len = n; d->pos = 0;
        }
        /* FreeBSD-11 record (the SDK's struct dirent): u32 fileno, u16 reclen, u8 type, u8 namlen, name, NUL */
        struct dirent *e = (struct dirent *)(d->buf + d->pos);
        if (d->len - d->pos < 8 || e->d_reclen < 8 + e->d_namlen + 1 || e->d_reclen > d->len - d->pos
            || e->d_name[e->d_namlen] != 0) { errno = EIO; return 0; }
        d->pos += e->d_reclen;
        if (e->d_fileno != 0) return e;
    }
}
int closedir(DIR *dir)
{
    struct ps5_dir *d = (struct ps5_dir *)dir;
    int r = close(d->fd);
    free(d);
    return r;
}
void rewinddir(DIR *dir)
{
    struct ps5_dir *d = (struct ps5_dir *)dir;
    lseek(d->fd, 0, SEEK_SET);
    d->len = d->pos = 0;
}
int dirfd(DIR *dir) { return ((struct ps5_dir *)dir)->fd; }
int readdir_r(DIR *dir, struct dirent *entry, struct dirent **result)
{
    struct dirent *e = readdir(dir);
    if (e) { memcpy(entry, e, e->d_reclen); *result = entry; } else *result = 0;
    return 0;
}
char *getcwd(char *buf, size_t size)
{
    static const char cwd[] = "/app0";
    if (!buf) { buf = (char *)malloc(size ? size : sizeof cwd); if (!buf) return 0; if (!size) size = sizeof cwd; }
    if (size < sizeof cwd) { errno = ERANGE; return 0; }
    memcpy(buf, cwd, sizeof cwd);
    return buf;
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunused-parameter"
#define PS5_ENOSYS(ret, name, params) ret name params { errno = ENOSYS; return (ret)-1; }
PS5_ENOSYS(ssize_t, readlink, (const char *p, char *b, size_t n))
PS5_ENOSYS(ssize_t, readlinkat, (int fd, const char *p, char *b, size_t n))
PS5_ENOSYS(int, fchownat, (int fd, const char *p, uid_t u, gid_t g, int fl))
PS5_ENOSYS(int, symlinkat, (const char *a, int fd, const char *b))
PS5_ENOSYS(int, mkfifoat, (int fd, const char *p, mode_t m))
PS5_ENOSYS(int, fchdir, (int fd))
PS5_ENOSYS(int, posix_fallocate, (int fd, off_t off, off_t len))
PS5_ENOSYS(int, statfs, (const char *p, struct statfs *s))
PS5_ENOSYS(int, fstatfs, (int fd, struct statfs *s))
PS5_ENOSYS(int, getmntinfo, (struct statfs **m, int fl))
PS5_ENOSYS(int, mknod, (const char *p, mode_t m, dev_t d))
PS5_ENOSYS(int, mknodat, (int fd, const char *p, mode_t m, dev_t d))
PS5_ENOSYS(int, mkfifo, (const char *p, mode_t m))
PS5_ENOSYS(int, link, (const char *a, const char *b))
PS5_ENOSYS(int, linkat, (int fa, const char *a, int fb, const char *b, int fl))
PS5_ENOSYS(int, symlink, (const char *a, const char *b))
PS5_ENOSYS(int, chown, (const char *p, uid_t u, gid_t g))
PS5_ENOSYS(int, fchown, (int fd, uid_t u, gid_t g))
PS5_ENOSYS(int, lchown, (const char *p, uid_t u, gid_t g))
PS5_ENOSYS(int, chroot, (const char *p))
PS5_ENOSYS(pid_t, fork, (void))
PS5_ENOSYS(pid_t, wait4, (pid_t p, int *st, int o, struct rusage *r))
PS5_ENOSYS(pid_t, setsid, (void))
PS5_ENOSYS(int, setpgid, (pid_t p, pid_t g))
PS5_ENOSYS(int, setgid, (gid_t g))
int syscall(int n, ...) { (void)n; errno = ENOSYS; return -1; }
#pragma clang diagnostic pop

/* ---- ioctl: linked with --wrap=ioctl (build-title.sh) --------------------------------------
 * std's Socket::set_nonblocking is ioctl(fd, FIONBIO, &on) (sys/net/connection/socket/unix.rs);
 * connect_timeout, which Ruffle's networking goes through, starts with it. A sandboxed title may
 * not make a socket non-blocking the BSD way: ioctl(FIONBIO) is EACCES, and so is
 * fcntl(F_SETFL, O_NONBLOCK), which ends in the same FIONBIO inside the kernel (v8/v9 on this
 * console; Mihawk measured the same on 13.40, PS5_RetroArch evidence/network-elevation — it
 * passes only after Lapy's elevation, which is why RetroArch's online features need the daemon).
 * A game sets non-blocking through libSceNet's SCE_NET_SO_NBIO instead, a socket option, so
 * FIONBIO becomes setsockopt(SOL_SOCKET, 0x1200) here; a descriptor that is not a socket, or a
 * kernel without the option, gets the fcntl route (a payload, or an elevated title). Everything
 * else goes to libkernel's ioctl as before. */
int __wrap_ioctl(int fd, unsigned long request, ...)
{
    va_list ap;
    va_start(ap, request);
    void *arg = va_arg(ap, void *);
    va_end(ap);
    if (request == FIONBIO) {
        int on = arg ? (*(int *)arg != 0) : 0;
        if (setsockopt(fd, SOL_SOCKET, PS5_SO_NBIO, &on, sizeof on) == 0) return 0;
        if (errno != ENOTSOCK && errno != ENOPROTOOPT && errno != EINVAL) return -1;
        int fl = fcntl(fd, F_GETFL);
        if (fl == -1) return -1;
        return fcntl(fd, F_SETFL, on ? (fl | O_NONBLOCK) : (fl & ~O_NONBLOCK));
    }
    return __real_ioctl(fd, request, arg);
}
#endif /* PS5_TITLE */
