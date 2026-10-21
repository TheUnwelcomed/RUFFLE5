/* PS5_GL_PROBE_V4 — OpenGL ES on the PS5 through pacbrew's Mesa (zink on RADV, EGL "videoout"),
 * as an elfldr payload: the GPU path Ruffle-on-PS5 milestone 2 renders through, proven on its own
 * before any Rust is involved.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * What it does, each step written to /data/ruffle/gl-probe.log (and to the sender's socket):
 *   1. SDL_Init(video + game controller): SDL's PS5 video driver, the display mode it reports.
 *   2. An OpenGL ES 3.0 window (fallback ES 2.0): SDL dlopen()s libEGL.so.1 and libGLESv2.so.2
 *      from LD_LIBRARY_PATH — this payload sets it to /data/ruffle/lib — or /user/homebrew/lib.
 *   3. GL_VENDOR / GL_RENDERER / GL_VERSION / GLSL version, the limits ruffle_render_glow reads
 *      (MAX_TEXTURE_SIZE, MAX_SAMPLES) and the extension it requires (OES_element_index_uint).
 *   4. Ruffle's shader dialect (GLSL ES 1.00, "#version 100") compiled and linked.
 *   5. A spinning triangle for ~6 seconds with vsync, frame-time statistics.
 *   6. A toast with the renderer, the version and the frame rate.
 * Every GL function is fetched with SDL_GL_GetProcAddress, exactly as glow does from Rust.
 *
 * v2 (2026-10-20): v1 on the console stopped at "Could not initialize EGL" — SDL's words for
 * eglInitialize failing, after libEGL and libGLESv2 had loaded. Mesa says why on stderr, which a
 * payload started from a launcher has nowhere to show: so stdout and stderr now go into the log
 * file, EGL/RADV/zink debug output is switched on, and when SDL fails the same stack is driven by
 * hand — libvulkan.so.1 (RADV) alone, then libEGL.so.1 alone — each call with its own result.
 *
 * v3 (2026-10-20): v2's log said it all: RADV itself is fine in the payload (AGC initialised, the
 * "PlayStation 5 GPU (RADV NAVI21)", one VK_KHR_display display), but zink's vkCreateDevice got
 * VK_ERROR_OUT_OF_DEVICE_MEMORY — RADV's ps5 winsys returns that when a direct-memory allocation
 * (sceKernelAllocateDirectMemory in [0, sceKernelGetDirectMemorySize()), type 12, then
 * sceKernelMapDirectMemory) fails, and RADV_DEBUG=info showed vram_size = 0 MB. Direct memory is
 * budgeted per process: John Törnblom's elfldr and selfldr both call budget_set(0) (syscall 0x23b)
 * in the child before exec, the payload manager that launched v1/v2 (pid 145, uid 1, euid 0) may
 * not. So v3 measures the pool and allocates the way RADV does, then — if that fails — calls
 * budget_set(0) itself and measures again, before SDL runs. A payload may make raw syscalls.
 *
 * v4 (2026-10-20): through websrv (Homebrew page and payload page alike) the pool is 12 GB, and
 * the stop moved into Sony's GPU library: "[Agc] FS Table offset has shifted. This is not
 * survivable." then sceAgcInit(8) = 0x8a6c002f, no device. In the payload manager's process
 * (uid 1, small pool) the same call succeeded. PS5_Vulkan's notes (docs/M5_PHASE_B.md, driver/
 * ps5vk_device.c): sceAgcInit made from code in anonymous memory (a dlopen'd module) failed
 * where the same call from the executable succeeded, and AGC tolerates a second sceAgcInit(8).
 * RADV calls it from libvulkan.so.1, which the SDK's loader maps into anonymous memory. So v4
 * calls sceAgcInit(8) from this executable (a linked import, loaded with the ELF) before
 * anything else, and if that is refused too, tries the other budget ids, logging each answer.
 */
#define VK_NO_PROTOTYPES 1
#include <vulkan/vulkan.h>
#include <EGL/egl.h>
#include <dlfcn.h>
#include <SDL2/SDL.h>
#include <stdint.h>
int32_t sceAgcInit(uint32_t version);
#include <GLES3/gl3.h>
#include <errno.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define LIB_DIR  "/data/ruffle/lib"
#define LOG_DIR  "/data/ruffle"
#define LOG_PATH LOG_DIR "/gl-probe.log"
#define RUN_SECONDS 6.0

/* ---- a toast on the TV ---------------------------------------------------------------------- */
typedef struct notify_request { char useless1[45]; char message[3075]; } notify_request_t;
int sceKernelSendNotificationRequest(int, notify_request_t *, size_t, int);
static void toast(const char *msg)
{
    notify_request_t req;
    memset(&req, 0, sizeof req);
    strncpy(req.message, msg, sizeof req.message - 1);
    sceKernelSendNotificationRequest(0, &req, sizeof req, 0);
}

/* ---- the log: a file and stdout ------------------------------------------------------------- */
static FILE *g_log;
static int g_log_is_stdout;
static void say(const char *fmt, ...)
{
    char line[1024];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(line, sizeof line, fmt, ap);
    va_end(ap);
    if (!g_log_is_stdout) { printf("%s\n", line); fflush(stdout); }
    if (g_log) { fprintf(g_log, "%s\n", line); fflush(g_log); }
}

/* ---- the stack by hand, for when SDL's one line is not enough ------------------------------- */
static void diagnose_vulkan(void)
{
    say("diag  libvulkan.so.1 (RADV) on its own:");
    void *vk = dlopen("libvulkan.so.1", RTLD_NOW | RTLD_GLOBAL);
    if (!vk) { const char *e = dlerror(); say("      dlopen(libvulkan.so.1) failed: %s", e ? e : "?"); return; }
    say("      dlopen(libvulkan.so.1) ok");
    PFN_vkGetInstanceProcAddr gipa = (PFN_vkGetInstanceProcAddr)dlsym(vk, "vkGetInstanceProcAddr");
    if (!gipa) { say("      no vkGetInstanceProcAddr"); return; }
    PFN_vkEnumerateInstanceVersion eiv = (PFN_vkEnumerateInstanceVersion)gipa(NULL, "vkEnumerateInstanceVersion");
    uint32_t ver = 0;
    if (eiv) { eiv(&ver); say("      instance version %u.%u.%u", VK_VERSION_MAJOR(ver), VK_VERSION_MINOR(ver), VK_VERSION_PATCH(ver)); }
    PFN_vkEnumerateInstanceExtensionProperties eiep = (PFN_vkEnumerateInstanceExtensionProperties)gipa(NULL, "vkEnumerateInstanceExtensionProperties");
    uint32_t n = 0;
    int have_display = 0, have_surface = 0;
    if (eiep && eiep(NULL, &n, NULL) == VK_SUCCESS && n > 0 && n < 256) {
        VkExtensionProperties *ext = calloc(n, sizeof *ext);
        if (ext && eiep(NULL, &n, ext) == VK_SUCCESS) {
            for (uint32_t i = 0; i < n; i++) {
                if (!strcmp(ext[i].extensionName, "VK_KHR_display")) have_display = 1;
                if (!strcmp(ext[i].extensionName, "VK_KHR_surface")) have_surface = 1;
            }
        }
        free(ext);
    }
    say("      %u instance extensions, VK_KHR_surface %s, VK_KHR_display %s", n, have_surface ? "yes" : "NO", have_display ? "yes" : "NO");
    PFN_vkCreateInstance ci = (PFN_vkCreateInstance)gipa(NULL, "vkCreateInstance");
    const char *exts[] = { "VK_KHR_surface", "VK_KHR_display" };
    VkApplicationInfo app = { VK_STRUCTURE_TYPE_APPLICATION_INFO, NULL, "ps5-gl-probe", 1, "none", 0, VK_API_VERSION_1_1 };
    VkInstanceCreateInfo info = { VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, NULL, 0, &app, 0, NULL,
                                  have_display && have_surface ? 2u : 0u, exts };
    VkInstance inst = VK_NULL_HANDLE;
    VkResult r = ci ? ci(&info, NULL, &inst) : VK_ERROR_INITIALIZATION_FAILED;
    say("      vkCreateInstance%s = %d", have_display && have_surface ? "(+surface,+display)" : "", (int)r);
    if (r != VK_SUCCESS) return;
    PFN_vkEnumeratePhysicalDevices epd = (PFN_vkEnumeratePhysicalDevices)gipa(inst, "vkEnumeratePhysicalDevices");
    PFN_vkGetPhysicalDeviceProperties gpdp = (PFN_vkGetPhysicalDeviceProperties)gipa(inst, "vkGetPhysicalDeviceProperties");
    PFN_vkGetPhysicalDeviceDisplayPropertiesKHR gpddp = (PFN_vkGetPhysicalDeviceDisplayPropertiesKHR)gipa(inst, "vkGetPhysicalDeviceDisplayPropertiesKHR");
    PFN_vkDestroyInstance di = (PFN_vkDestroyInstance)gipa(inst, "vkDestroyInstance");
    uint32_t nd = 0;
    r = epd ? epd(inst, &nd, NULL) : VK_ERROR_INITIALIZATION_FAILED;
    say("      vkEnumeratePhysicalDevices = %d, %u device(s)", (int)r, nd);
    if (r == VK_SUCCESS && nd > 0) {
        VkPhysicalDevice devs[4];
        if (nd > 4) nd = 4;
        epd(inst, &nd, devs);
        for (uint32_t i = 0; i < nd; i++) {
            VkPhysicalDeviceProperties p;
            memset(&p, 0, sizeof p);
            if (gpdp) gpdp(devs[i], &p);
            say("      device %u: %s, API %u.%u.%u, driver 0x%x, type %d", i, p.deviceName,
                VK_VERSION_MAJOR(p.apiVersion), VK_VERSION_MINOR(p.apiVersion), VK_VERSION_PATCH(p.apiVersion), p.driverVersion, (int)p.deviceType);
            if (gpddp && have_display) {
                uint32_t ndisp = 0;
                r = gpddp(devs[i], &ndisp, NULL);
                say("      vkGetPhysicalDeviceDisplayPropertiesKHR = %d, %u display(s) (VideoOut through RADV's WSI)", (int)r, ndisp);
            }
        }
    }
    if (di) di(inst, NULL);
    say("      vkDestroyInstance done");
}

static void diagnose_egl(void)
{
    say("diag  libEGL.so.1 on its own (EGL_PLATFORM=videoout):");
    setenv("EGL_PLATFORM", "videoout", 0);
    void *egl = dlopen("libEGL.so.1", RTLD_NOW | RTLD_GLOBAL);
    if (!egl) { const char *e = dlerror(); say("      dlopen(libEGL.so.1) failed: %s", e ? e : "?"); return; }
    void *gles = dlopen("libGLESv2.so.2", RTLD_NOW | RTLD_GLOBAL);
    say("      dlopen(libEGL.so.1) ok, dlopen(libGLESv2.so.2) %s", gles ? "ok" : "FAILED");
    EGLDisplay (*getDisplay)(EGLNativeDisplayType) = (EGLDisplay (*)(EGLNativeDisplayType))dlsym(egl, "eglGetDisplay");
    EGLBoolean (*initialize)(EGLDisplay, EGLint *, EGLint *) = (EGLBoolean (*)(EGLDisplay, EGLint *, EGLint *))dlsym(egl, "eglInitialize");
    EGLint (*getError)(void) = (EGLint (*)(void))dlsym(egl, "eglGetError");
    const char *(*queryString)(EGLDisplay, EGLint) = (const char *(*)(EGLDisplay, EGLint))dlsym(egl, "eglQueryString");
    EGLBoolean (*terminate)(EGLDisplay) = (EGLBoolean (*)(EGLDisplay))dlsym(egl, "eglTerminate");
    if (!getDisplay || !initialize || !getError) { say("      eglGetDisplay/eglInitialize/eglGetError not found"); return; }
    if (queryString) { const char *ce = queryString(EGL_NO_DISPLAY, EGL_EXTENSIONS); say("      client extensions: %s", ce ? ce : "(none)"); }
    EGLDisplay d = getDisplay(EGL_DEFAULT_DISPLAY);
    say("      eglGetDisplay(EGL_DEFAULT_DISPLAY) = %p, eglGetError 0x%x", d, (unsigned)getError());
    if (d == EGL_NO_DISPLAY) return;
    EGLint major = 0, minor = 0;
    EGLBoolean ok = initialize(d, &major, &minor);
    say("      eglInitialize = %d (EGL %d.%d), eglGetError 0x%x %s", (int)ok, major, minor, (unsigned)getError(),
        ok ? "" : "(0x3001 = EGL_NOT_INITIALIZED: the reason is in Mesa's lines above)");
    if (ok && queryString) {
        say("      EGL_VENDOR=%s EGL_VERSION=%s", queryString(d, EGL_VENDOR), queryString(d, EGL_VERSION));
        say("      EGL_CLIENT_APIS=%s", queryString(d, EGL_CLIENT_APIS));
        if (terminate) terminate(d);
    }
}
static void fail(const char *what)
{
    char msg[512];
    snprintf(msg, sizeof msg, "GL probe FAILED: %s", what);
    say("FAIL  %s", what);
    toast(msg);
    if (g_log) fclose(g_log);
    SDL_Quit();
    exit(1);
}

/* ---- the GL functions, by name through SDL (as glow's from_loader_function) ----------------- */
#define GL_FUNCTIONS(X) \
    X(const GLubyte *, glGetString, (GLenum)) \
    X(const GLubyte *, glGetStringi, (GLenum, GLuint)) \
    X(void, glGetIntegerv, (GLenum, GLint *)) \
    X(GLenum, glGetError, (void)) \
    X(void, glClearColor, (GLfloat, GLfloat, GLfloat, GLfloat)) \
    X(void, glClear, (GLbitfield)) \
    X(void, glViewport, (GLint, GLint, GLsizei, GLsizei)) \
    X(GLuint, glCreateShader, (GLenum)) \
    X(void, glShaderSource, (GLuint, GLsizei, const GLchar *const *, const GLint *)) \
    X(void, glCompileShader, (GLuint)) \
    X(void, glGetShaderiv, (GLuint, GLenum, GLint *)) \
    X(void, glGetShaderInfoLog, (GLuint, GLsizei, GLsizei *, GLchar *)) \
    X(GLuint, glCreateProgram, (void)) \
    X(void, glAttachShader, (GLuint, GLuint)) \
    X(void, glLinkProgram, (GLuint)) \
    X(void, glGetProgramiv, (GLuint, GLenum, GLint *)) \
    X(void, glGetProgramInfoLog, (GLuint, GLsizei, GLsizei *, GLchar *)) \
    X(void, glUseProgram, (GLuint)) \
    X(GLint, glGetAttribLocation, (GLuint, const GLchar *)) \
    X(GLint, glGetUniformLocation, (GLuint, const GLchar *)) \
    X(void, glUniformMatrix4fv, (GLint, GLsizei, GLboolean, const GLfloat *)) \
    X(void, glGenBuffers, (GLsizei, GLuint *)) \
    X(void, glBindBuffer, (GLenum, GLuint)) \
    X(void, glBufferData, (GLenum, GLsizeiptr, const void *, GLenum)) \
    X(void, glGenVertexArrays, (GLsizei, GLuint *)) \
    X(void, glBindVertexArray, (GLuint)) \
    X(void, glEnableVertexAttribArray, (GLuint)) \
    X(void, glVertexAttribPointer, (GLuint, GLint, GLenum, GLboolean, GLsizei, const void *)) \
    X(void, glDrawElements, (GLenum, GLsizei, GLenum, const void *)) \
    X(void, glEnable, (GLenum)) \
    X(void, glBlendFunc, (GLenum, GLenum)) \
    X(void, glFinish, (void))

#define DECLARE(ret, name, params) static ret (*name##_)params;
GL_FUNCTIONS(DECLARE)
#undef DECLARE

static int load_gl(void)
{
    int missing = 0;
#define LOAD(ret, name, params) \
    name##_ = (ret (*)params)SDL_GL_GetProcAddress(#name); \
    if (!name##_) { say("      missing GL function: %s", #name); missing++; }
    GL_FUNCTIONS(LOAD)
#undef LOAD
    return missing;
}

/* ---- Ruffle's shader dialect: GLSL ES 1.00 -------------------------------------------------- */
static const char *VERT =
    "#version 100\n"
    "#ifdef GL_FRAGMENT_PRECISION_HIGH\n precision highp float;\n#else\n precision mediump float;\n#endif\n"
    "uniform mat4 view_matrix;\n"
    "attribute vec2 position;\n"
    "attribute vec4 color;\n"
    "varying vec4 frag_color;\n"
    "void main() {\n"
    "  frag_color = color;\n"
    "  gl_Position = view_matrix * vec4(position, 0.0, 1.0);\n"
    "}\n";
static const char *FRAG =
    "#version 100\n"
    "#ifdef GL_FRAGMENT_PRECISION_HIGH\n precision highp float;\n#else\n precision mediump float;\n#endif\n"
    "varying vec4 frag_color;\n"
    "void main() { gl_FragColor = frag_color; }\n";

static GLuint compile(GLenum kind, const char *src, const char *label)
{
    GLuint s = glCreateShader_(kind);
    glShaderSource_(s, 1, &src, NULL);
    glCompileShader_(s);
    GLint ok = 0;
    glGetShaderiv_(s, GL_COMPILE_STATUS, &ok);
    if (!ok) {
        char info[2048] = "";
        glGetShaderInfoLog_(s, sizeof info, NULL, info);
        say("      %s shader log: %s", label, info);
        fail("GLSL ES 1.00 shader did not compile");
    }
    return s;
}

/* ---- direct memory: what RADV's winsys will get from this process ------------------------- */
int32_t sceKernelAllocateDirectMemory(int64_t, int64_t, size_t, size_t, int, int64_t *);
int32_t sceKernelReleaseDirectMemory(int64_t, size_t);
int32_t sceKernelMapDirectMemory(void **, size_t, int, int, int64_t, size_t);
int64_t sceKernelGetDirectMemorySize(void);
int32_t sceKernelAvailableDirectMemorySize(int64_t, int64_t, size_t, int64_t *, size_t *);
int32_t sceKernelMunmap(void *, size_t);
#define DM_TYPE 12                  /* RADV_PS5_DIRECT_TYPE */
#define DM_PROT 0x33                /* CPU read|write, GPU read|write (RADV_PS5_PROTECTION) */
#define DM_ALIGN ((size_t)0x200000) /* RADV_PS5_LARGE_BYTES: 2 MiB */
#define DM_GPU_LIMIT (UINT64_C(1) << 47)
#define DM_REGION_BASE UINT64_C(0x4000000000)

/* one allocation the way radv_ps5_memory_alloc does it; 0 = ok, else the failing step */
static int dm_try(size_t bytes, int with_hint)
{
    int64_t phys = -1;
    int32_t r = sceKernelAllocateDirectMemory(0, sceKernelGetDirectMemorySize(), bytes, DM_ALIGN, DM_TYPE, &phys);
    if (r != 0) { say("      sceKernelAllocateDirectMemory(%zu MB) = 0x%08x", bytes >> 20, (unsigned)r); return 1; }
    void *addr = with_hint ? (void *)(uintptr_t)DM_REGION_BASE : NULL;
    r = sceKernelMapDirectMemory(&addr, bytes, DM_PROT, 0, phys, DM_ALIGN);
    if (r != 0) {
        say("      sceKernelAllocateDirectMemory(%zu MB) ok at 0x%llx, sceKernelMapDirectMemory%s = 0x%08x", bytes >> 20,
            (unsigned long long)phys, with_hint ? "(hint 0x4000000000)" : "", (unsigned)r);
        sceKernelReleaseDirectMemory(phys, bytes);
        return 2;
    }
    uint64_t va = (uint64_t)(uintptr_t)addr;
    say("      %zu MB: physical 0x%llx mapped at 0x%llx%s (%s the GPU's 47-bit range)", bytes >> 20,
        (unsigned long long)phys, (unsigned long long)va, with_hint ? " (hint 0x4000000000)" : "",
        va < DM_GPU_LIMIT ? "inside" : "OUTSIDE");
    ((volatile char *)addr)[0] = 1;  /* touch it */
    sceKernelMunmap(addr, bytes);
    sceKernelReleaseDirectMemory(phys, bytes);
    return va < DM_GPU_LIMIT ? 0 : 3;
}

static void dm_report(const char *when)
{
    int64_t pool = sceKernelGetDirectMemorySize();
    int64_t start = -1;
    size_t avail = 0;
    int32_t r = pool > 0 ? sceKernelAvailableDirectMemorySize(0, pool, 0x4000, &start, &avail) : -1;
    say("      direct memory %s: pool (sceKernelGetDirectMemorySize) = %lld MB, available = %zu MB (query 0x%08x, start 0x%llx)",
        when, (long long)(pool >> 20), avail >> 20, (unsigned)r, (unsigned long long)start);
}

/* 0 when a 16 MB and a 64 MB allocation both work (RADV's submission ring is 16 MB) */
static int dm_probe(void)
{
    dm_report("now");
    int a = dm_try((size_t)16 << 20, 0);
    int b = dm_try((size_t)64 << 20, 0);
    int c = dm_try((size_t)16 << 20, 1);
    say("      16 MB %s, 64 MB %s, 16 MB at RADV's region %s", a ? "FAILED" : "ok", b ? "FAILED" : "ok", c ? "FAILED" : "ok");
    return a || b;
}

static double now_s(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec + ts.tv_nsec / 1e9;
}

int main(int argc, char **argv)
{
    (void)argc; (void)argv;
    mkdir(LOG_DIR, 0777);
    g_log = fopen(LOG_PATH, "w");
    say("PS5 GL probe (PS5_GL_PROBE_V1) — SDL %d.%d.%d, Mesa from " LIB_DIR " or /user/homebrew/lib",
        SDL_MAJOR_VERSION, SDL_MINOR_VERSION, SDL_PATCHLEVEL);
    if (!g_log) say("      (no log file: %s, errno %d)", LOG_PATH, errno);
    say("      pid %d uid %d euid %d", (int)getpid(), (int)getuid(), (int)geteuid());
    /* Mesa talks on stderr; a payload started from a launcher has no reader there: into the log */
    if (g_log) {
        fflush(stdout); fflush(stderr);
        dup2(fileno(g_log), 1);
        dup2(fileno(g_log), 2);
        setvbuf(stdout, NULL, _IOLBF, 0);
        setvbuf(stderr, NULL, _IONBF, 0);
        g_log_is_stdout = 1;
    }
    setenv("EGL_LOG_LEVEL", "debug", 0);   /* _eglError's reason, e.g. "videoout: zink did not start" */
    setenv("RADV_DEBUG", "info", 0);        /* the device RADV found, or why it could not set it up */
    setenv("LIBGL_DEBUG", "verbose", 0);
    setenv("MESA_DEBUG", "1", 0);
    say("      EGL_LOG_LEVEL=debug RADV_DEBUG=info LIBGL_DEBUG=verbose (Mesa's own messages follow in this log)");

    /* The payload loader looks in LD_LIBRARY_PATH, then /user/homebrew/lib, then the cwd. */
    if (!getenv("LD_LIBRARY_PATH")) setenv("LD_LIBRARY_PATH", LIB_DIR, 1);
    say("      LD_LIBRARY_PATH=%s", getenv("LD_LIBRARY_PATH"));
    for (const char *name = "libEGL.so.1"; name; name = NULL) {
        char p[256];
        struct stat st;
        snprintf(p, sizeof p, "%s/%s", LIB_DIR, name);
        say("      %s: %s", p, stat(p, &st) == 0 ? "present" : "not there (then /user/homebrew/lib must have it)");
    }
    /* direct memory first: it is what vkCreateDevice ran out of in v2 */
    say("..    direct memory, the way RADV allocates it (type 12, 2 MiB aligned, CPU+GPU)");
    if (dm_probe()) {
        say("      -> this process cannot give RADV its memory; trying budget_set(0), as elfldr/selfldr do before exec");
        long rc = syscall(0x23b, 0);
        say("      syscall(0x23b /* budget_set */, 0) = %ld%s", rc, rc ? " (errno set)" : "");
        if (rc) say("      errno %d", errno);
        if (dm_probe()) {
            say("warn  direct memory still short: launch this payload through websrv or elfldr (port 9021), which set the budget");
            toast("GL probe: no direct memory for the GPU in this process (see log)");
        } else {
            say("ok    budget_set(0) gave this process its direct memory");
        }
    } else {
        say("ok    direct memory is there");
    }

    /* AGC from the executable itself, before any module is dlopen()ed (v4) */
    say("..    sceAgcInit(8) from the executable (PS5_Vulkan: from a dlopen()ed module it was refused)");
    int32_t agc = sceAgcInit(8);
    say("%s  sceAgcInit(8) from the executable = 0x%08x", agc == 0 ? "ok  " : "warn", (unsigned)agc);
    if (agc != 0) {
        for (long k = 1; k <= 8 && agc != 0; k++) {
            long rc = syscall(0x23b, k);
            if (rc != 0) { say("      budget_set(%ld) = %ld (errno %d)", k, rc, errno); continue; }
            agc = sceAgcInit(8);
            say("      budget_set(%ld) = 0, sceAgcInit(8) = 0x%08x, pool %lld MB", k, (unsigned)agc,
                (long long)(sceKernelGetDirectMemorySize() >> 20));
        }
        if (agc != 0) say("warn  AGC refused this process under every budget tried; the GPU library's reason is the line above");
        else toast("GL probe: AGC initialised after a budget change (see log)");
    }
    toast("GL probe: starting SDL");

    /* 1. SDL */
    if (SDL_Init(SDL_INIT_VIDEO | SDL_INIT_GAMECONTROLLER) != 0) {
        say("      SDL_Init: %s", SDL_GetError());
        fail("SDL_Init");
    }
    say("ok    SDL_Init: video driver %s, %d joystick(s)", SDL_GetCurrentVideoDriver(), SDL_NumJoysticks());
    SDL_DisplayMode mode;
    if (SDL_GetCurrentDisplayMode(0, &mode) == 0)
        say("      display 0: %dx%d @ %d Hz, format %s", mode.w, mode.h, mode.refresh_rate, SDL_GetPixelFormatName(mode.format));
    else
        say("      SDL_GetCurrentDisplayMode: %s", SDL_GetError());

    /* 2. an ES window: SDL loads libEGL.so.1 + libGLESv2.so.2 here */
    SDL_Window *win = NULL;
    SDL_GLContext ctx = NULL;
    static const int versions[][2] = { {3, 0}, {2, 0} };
    for (size_t i = 0; i < sizeof versions / sizeof versions[0] && !ctx; i++) {
        SDL_GL_ResetAttributes();
        SDL_GL_SetAttribute(SDL_GL_CONTEXT_PROFILE_MASK, SDL_GL_CONTEXT_PROFILE_ES);
        SDL_GL_SetAttribute(SDL_GL_CONTEXT_MAJOR_VERSION, versions[i][0]);
        SDL_GL_SetAttribute(SDL_GL_CONTEXT_MINOR_VERSION, versions[i][1]);
        SDL_GL_SetAttribute(SDL_GL_DOUBLEBUFFER, 1);
        SDL_GL_SetAttribute(SDL_GL_DEPTH_SIZE, 0);
        SDL_GL_SetAttribute(SDL_GL_STENCIL_SIZE, 8);   /* ruffle_render_glow masks with the stencil buffer */
        if (!win) {
            win = SDL_CreateWindow("ps5-gl-probe", SDL_WINDOWPOS_UNDEFINED, SDL_WINDOWPOS_UNDEFINED,
                                   mode.w > 0 ? mode.w : 1920, mode.h > 0 ? mode.h : 1080,
                                   SDL_WINDOW_OPENGL | SDL_WINDOW_FULLSCREEN | SDL_WINDOW_SHOWN);
            if (!win) { say("      SDL_CreateWindow (ES %d.%d): %s", versions[i][0], versions[i][1], SDL_GetError()); continue; }
            say("ok    window: SDL_WINDOW_OPENGL | FULLSCREEN");
        }
        ctx = SDL_GL_CreateContext(win);
        if (!ctx) say("      SDL_GL_CreateContext (ES %d.%d): %s", versions[i][0], versions[i][1], SDL_GetError());
        else say("ok    GL context: requested ES %d.%d", versions[i][0], versions[i][1]);
    }
    if (!win || !ctx) {
        say("      SDL could not get a GL window (%s); the same stack by hand:", SDL_GetError());
        diagnose_vulkan();
        diagnose_egl();
    }
    if (!win) fail("SDL_CreateWindow — Mesa's own reason is in /data/ruffle/gl-probe.log");
    if (!ctx) fail("SDL_GL_CreateContext — see /data/ruffle/gl-probe.log");
    if (SDL_GL_MakeCurrent(win, ctx) != 0) { say("      SDL_GL_MakeCurrent: %s", SDL_GetError()); fail("SDL_GL_MakeCurrent"); }
    int w = 0, h = 0;
    SDL_GL_GetDrawableSize(win, &w, &h);
    say("      drawable %dx%d, swap interval %d", w, h, SDL_GL_GetSwapInterval());

    /* 3. what the driver says */
    if (load_gl()) fail("SDL_GL_GetProcAddress gave NULL for a GL function");
    const char *vendor = (const char *)glGetString_(GL_VENDOR);
    const char *renderer = (const char *)glGetString_(GL_RENDERER);
    const char *version = (const char *)glGetString_(GL_VERSION);
    const char *glsl = (const char *)glGetString_(GL_SHADING_LANGUAGE_VERSION);
    say("ok    GL_VENDOR=%s", vendor ? vendor : "(null)");
    say("ok    GL_RENDERER=%s", renderer ? renderer : "(null)");
    say("ok    GL_VERSION=%s", version ? version : "(null)");
    say("ok    GL_SHADING_LANGUAGE_VERSION=%s", glsl ? glsl : "(null)");
    GLint max_tex = 0, max_samples = 0, n_ext = 0, max_rb = 0, max_vattr = 0;
    glGetIntegerv_(GL_MAX_TEXTURE_SIZE, &max_tex);
    glGetIntegerv_(GL_MAX_SAMPLES, &max_samples);
    glGetIntegerv_(GL_MAX_RENDERBUFFER_SIZE, &max_rb);
    glGetIntegerv_(GL_MAX_VERTEX_ATTRIBS, &max_vattr);
    glGetIntegerv_(GL_NUM_EXTENSIONS, &n_ext);
    say("      MAX_TEXTURE_SIZE=%d MAX_SAMPLES=%d MAX_RENDERBUFFER_SIZE=%d MAX_VERTEX_ATTRIBS=%d extensions=%d",
        max_tex, max_samples, max_rb, max_vattr, n_ext);
    int have_uint_index = 0, have_vao = 0;
    for (GLint i = 0; i < n_ext && glGetStringi_; i++) {
        const char *e = (const char *)glGetStringi_(GL_EXTENSIONS, (GLuint)i);
        if (!e) continue;
        if (!strcmp(e, "GL_OES_element_index_uint")) have_uint_index = 1;
        if (!strcmp(e, "GL_OES_vertex_array_object")) have_vao = 1;
        if (i < 12) say("      ext[%d]=%s", i, e);
    }
    int es3 = version && strstr(version, "OpenGL ES 3") != NULL;
    say("%s  OES_element_index_uint: %s (ruffle_render_glow needs it; core in ES 3)",
        have_uint_index || es3 ? "ok  " : "warn", have_uint_index ? "extension" : es3 ? "core" : "ABSENT");
    say("      OES_vertex_array_object: %s", have_vao ? "yes" : es3 ? "core" : "no");

    /* 4. Ruffle's shader dialect */
    GLuint vs = compile(GL_VERTEX_SHADER, VERT, "vertex");
    GLuint fs = compile(GL_FRAGMENT_SHADER, FRAG, "fragment");
    GLuint prog = glCreateProgram_();
    glAttachShader_(prog, vs);
    glAttachShader_(prog, fs);
    glLinkProgram_(prog);
    GLint linked = 0;
    glGetProgramiv_(prog, GL_LINK_STATUS, &linked);
    if (!linked) {
        char info[2048] = "";
        glGetProgramInfoLog_(prog, sizeof info, NULL, info);
        say("      program log: %s", info);
        fail("GLSL ES 1.00 program did not link");
    }
    say("ok    GLSL ES 1.00 (#version 100) vertex + fragment compiled and linked");
    GLint a_pos = glGetAttribLocation_(prog, "position");
    GLint a_col = glGetAttribLocation_(prog, "color");
    GLint u_view = glGetUniformLocation_(prog, "view_matrix");

    /* 5. a triangle, drawn the way ruffle_render_glow draws: VAO + VBO + element indices */
    static const GLfloat verts[] = {
        /* x,     y,    r, g, b, a */
        -0.6f, -0.5f, 1.f, 0.2f, 0.2f, 1.f,
         0.6f, -0.5f, 0.2f, 1.f, 0.2f, 1.f,
         0.0f,  0.7f, 0.2f, 0.4f, 1.f, 1.f,
    };
    static const GLuint indices[] = { 0, 1, 2 };
    GLuint vao = 0, vbo = 0, ibo = 0;
    if (es3 || have_vao) { glGenVertexArrays_(1, &vao); glBindVertexArray_(vao); }
    glGenBuffers_(1, &vbo);
    glBindBuffer_(GL_ARRAY_BUFFER, vbo);
    glBufferData_(GL_ARRAY_BUFFER, sizeof verts, verts, GL_STATIC_DRAW);
    glGenBuffers_(1, &ibo);
    glBindBuffer_(GL_ELEMENT_ARRAY_BUFFER, ibo);
    glBufferData_(GL_ELEMENT_ARRAY_BUFFER, sizeof indices, indices, GL_STATIC_DRAW);
    glEnableVertexAttribArray_((GLuint)a_pos);
    glVertexAttribPointer_((GLuint)a_pos, 2, GL_FLOAT, GL_FALSE, 6 * sizeof(GLfloat), (void *)0);
    glEnableVertexAttribArray_((GLuint)a_col);
    glVertexAttribPointer_((GLuint)a_col, 4, GL_FLOAT, GL_FALSE, 6 * sizeof(GLfloat), (void *)(2 * sizeof(GLfloat)));
    glEnable_(GL_BLEND);
    glBlendFunc_(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
    glUseProgram_(prog);
    glViewport_(0, 0, w, h);
    GLenum err = glGetError_();
    say("%s  GL setup, glGetError=0x%x", err == GL_NO_ERROR ? "ok  " : "warn", err);
    SDL_GL_SetSwapInterval(1);

    toast("GL probe: drawing (6 s)");
    double t0 = now_s(), last = t0, worst = 0, sum = 0;
    int frames = 0, quit = 0;
    float aspect = h > 0 ? (float)w / (float)h : 1.0f;
    while (!quit) {
        SDL_Event ev;
        while (SDL_PollEvent(&ev)) {
            if (ev.type == SDL_QUIT || ev.type == SDL_CONTROLLERBUTTONDOWN) quit = 1;
        }
        double t = now_s();
        float a = (float)((t - t0) * 1.5);
        float c = cosf(a), s = sinf(a);
        const GLfloat view[16] = {
            c / aspect, s, 0, 0,
            -s / aspect, c, 0, 0,
            0, 0, 1, 0,
            0, 0, 0, 1,
        };
        float bg = 0.5f + 0.5f * sinf((float)(t - t0));
        glClearColor_(0.05f, 0.05f * bg, 0.15f, 1.0f);
        glClear_(GL_COLOR_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
        glUniformMatrix4fv_(u_view, 1, GL_FALSE, view);
        glDrawElements_(GL_TRIANGLES, 3, GL_UNSIGNED_INT, (void *)0);
        SDL_GL_SwapWindow(win);
        double dt = now_s() - last;
        last += dt;
        if (frames > 0) { sum += dt; if (dt > worst) worst = dt; }
        frames++;
        if (frames == 1) say("ok    first frame presented after %.0f ms", (last - t0) * 1000.0);
        if (last - t0 > RUN_SECONDS) quit = 1;
    }
    glFinish_();
    err = glGetError_();
    double avg_ms = frames > 1 ? sum / (frames - 1) * 1000.0 : 0;
    say("%s  %d frames in %.1f s: %.1f fps, avg %.2f ms, worst %.1f ms, glGetError=0x%x",
        err == GL_NO_ERROR ? "ok  " : "warn", frames, last - t0, frames / (last - t0), avg_ms, worst * 1000.0, err);

    char msg[400];
    snprintf(msg, sizeof msg, "GL probe OK: %s — %s — %.0f fps", renderer ? renderer : "?", version ? version : "?", frames / (last - t0));
    say("done  %s", msg);
    toast(msg);
    if (g_log) fclose(g_log);
    SDL_GL_DeleteContext(ctx);
    SDL_DestroyWindow(win);
    SDL_Quit();
    return 0;
}
