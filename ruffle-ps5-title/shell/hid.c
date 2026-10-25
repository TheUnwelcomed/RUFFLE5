/* PS5_RUFFLE_HID_V3 — a USB keyboard and mouse, for src/hid.rs.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * libSceKeyboard and libSceMouse are not loaded for a title. They are system modules: loaded with
 * sceSysmoduleLoadModule (0x0106 keyboard, 0x00a9 mouse) and their four functions then looked up
 * with sceKernelDlsym. Nothing is imported from those libraries, so a console where a step fails
 * just has no keyboard or mouse (the log says which step and what the console answered).
 *
 * V2 (1.000.022), after V1 failed on hardware with "step -1, 0x80020002" for both:
 *   - V1 found the loaded module in sceKernelGetModuleList by name, else loaded
 *     /system/common/lib/<lib>.sprx by path. Neither works from a title: the module list shows only
 *     the title's own modules (prospero-win, native/pw_hid_ps5.c, measured on the console), and a
 *     title's sandbox does not see /system/common/lib (0x80020002 = ENOENT; loading by path from
 *     outside the sandbox answers 0x80020063, ESDKVERSION — Ruffle-Flash-PS5, PS5SX2).
 *     V2 asks libSceSysmodule for the module's handle instead, by its id and then its internal id
 *     (0x80000000 | id), as prospero-win and Playden do.
 *   - The sysmodule loads now run first thing in main() (ps5_hid_preload), before the title opens
 *     /data or initialises the GPU — as Ruffle-Flash-PS5 (.preinit_array), ProsperoLight and
 *     prospero-win do ("before the /data grant changes the title's credentials").
 *   - V1 overwrote sceSysmoduleLoadModule's answer with the path load's. V2 keeps both: the load's
 *     answer (ps5_hid_sysmodule_rc) and the handle lookup's (the `detail` of step -1).
 * Research: reports/PS5 homebrew USB keyboard input.md (2026-10-24).
 *
 * V3 (1.000.023), after V2 on hardware: both sysmodule loads answered 0 (loading first works), but
 * sceSysmoduleGetModuleHandleInternal is an import this console leaves unbound ("step -3, 1").
 * So the functions are now imported directly, as Ruffle-Flash-PS5 (ps5/scripts/ps5_early.c) and
 * ProsperoLight (hardware-checked keyboard and mouse) do: build-title.sh links stubs for
 * libSceKeyboard and libSceMouse, and the loader binds the imports once the sysmodule is loaded.
 * They are read through volatile pointers, since an unbound import reads as NULL. The handle
 * lookup stays as the second way, for a console that exports it.
 *
 * The system calls it does use are checked first against the title's import table (build-title.sh
 * links a table of every import; one the console does not export is bound to NULL, and calling it
 * would end the title): `ps5_import_missing`, also used by src/cpu.rs.
 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

int32_t sceSysmoduleLoadModule(uint16_t id);
/* Weak: the PS5 payload SDK's libSceSysmodule stub exports it (sce_stubs/libSceSysmodule.c at
 * a7685ce, 2026-10-12), but an older SDK's may not; then the link still succeeds, the symbol is
 * NULL (build-title.sh links with -z nodynamic-undefined-weak) and step -3 says so. */
__attribute__((weak)) int32_t sceSysmoduleGetModuleHandleInternal(uint32_t id, int32_t *handle);
int sceKernelDlsym(int handle, const char *symbol, void **address);

/* Imported directly (build-title.sh: stubs for libSceKeyboard.sprx and libSceMouse.sprx). */
int sceKeyboardInit(void);
int sceKeyboardOpen(int user, int type, int index, const void *param);
int sceKeyboardRead(int handle, void *data, int count);
int sceKeyboardClose(int handle);
int sceMouseInit(void);
int sceMouseOpen(int user, int type, int index, const void *param);
int sceMouseRead(int handle, void *data, int count);
int sceMouseClose(int handle);

/* The direct imports for the keyboard (0) or mouse (1); NULL where the loader left one unbound. */
static void direct_imports(int which, void *fns[4])
{
    void *volatile v[4];
    if (which == 0) {
        v[0] = (void *)sceKeyboardInit;
        v[1] = (void *)sceKeyboardOpen;
        v[2] = (void *)sceKeyboardRead;
        v[3] = (void *)sceKeyboardClose;
    } else {
        v[0] = (void *)sceMouseInit;
        v[1] = (void *)sceMouseOpen;
        v[2] = (void *)sceMouseRead;
        v[3] = (void *)sceMouseClose;
    }
    for (int i = 0; i < 4; i++)
        fns[i] = v[i];
}

/* From the import table build-title.sh links (import_check.c). */
int ps5_import_count(void);
const char *ps5_import_null(int i);

/* 1 when `name` is an import the console left unbound (NULL). */
int ps5_import_missing(const char *name)
{
    int n = ps5_import_count();
    for (int i = 0; i < n; i++) {
        const char *missing = ps5_import_null(i);
        if (missing && strcmp(missing, name) == 0)
            return 1;
    }
    return 0;
}

struct hid_api {
    uint16_t sysmodule;
    const char *names[4]; /* init, open, read, close */
    int (*init)(void);
    int (*open)(int user, int type, int index, const void *param);
    int (*read)(int handle, void *data, int count);
    int (*close)(int handle);
    int32_t sysmodule_rc; /* sceSysmoduleLoadModule's answer; 1 = not called yet */
    int source;           /* 1 direct imports, 2 the module handle; 0 none yet */
    int loaded;           /* 0 not tried, 1 ready, <0 the step that failed */
    int detail;
};

static struct hid_api g_hid[2] = {
    { 0x0106, { "sceKeyboardInit", "sceKeyboardOpen", "sceKeyboardRead", "sceKeyboardClose" }, 0, 0, 0, 0, 1, 0, 0, 0 },
    { 0x00a9, { "sceMouseInit", "sceMouseOpen", "sceMouseRead", "sceMouseClose" }, 0, 0, 0, 0, 1, 0, 0, 0 },
};

/* Load both system modules. main() calls this before anything else; ps5_hid_load does it later
 * if it was not called. */
void ps5_hid_preload(void)
{
    if (ps5_import_missing("sceSysmoduleLoadModule"))
        return;
    for (int i = 0; i < 2; i++)
        if (g_hid[i].sysmodule_rc == 1)
            g_hid[i].sysmodule_rc = sceSysmoduleLoadModule(g_hid[i].sysmodule);
}

/* sceSysmoduleLoadModule's answer for the keyboard (0) or mouse (1); 1 if it was not called. */
int32_t ps5_hid_sysmodule_rc(int which)
{
    return which >= 0 && which <= 1 ? g_hid[which].sysmodule_rc : 1;
}

/* The module's handle by its sysmodule id, else by its internal id (0x80000000 | id); -1 and the
 * first lookup's answer in *rc when neither gives one. */
static int32_t sysmodule_handle(uint16_t id, int32_t *rc)
{
    int32_t handle = -1;
    int32_t r = sceSysmoduleGetModuleHandleInternal(id, &handle);
    if (r == 0 && handle >= 0)
        return handle;
    *rc = r != 0 ? r : handle;
    handle = -1;
    if (sceSysmoduleGetModuleHandleInternal(0x80000000u | id, &handle) == 0 && handle >= 0)
        return handle;
    return -1;
}

/* 0 when the keyboard (0) or mouse (1) functions are ready; else the failed step (<0):
 *   -3  not bound: sceSysmoduleLoadModule (detail 0), or neither the direct imports nor the
 *       handle lookup (detail 1)
 *   -1  no handle for the module (detail: the handle lookup's answer)
 *   -2  the module has no such functions (detail: the handle; 0 for the direct imports) */
int ps5_hid_load(int which, int32_t *detail)
{
    if (which < 0 || which > 1)
        return -9;
    struct hid_api *api = &g_hid[which];
    if (api->loaded) {
        *detail = api->detail;
        return api->loaded > 0 ? 0 : api->loaded;
    }
    if (ps5_import_missing("sceSysmoduleLoadModule")) {
        api->loaded = -3;
        api->detail = 0;
        *detail = 0;
        return -3;
    }
    if (api->sysmodule_rc == 1)
        api->sysmodule_rc = sceSysmoduleLoadModule(api->sysmodule);
    void *fns[4] = { NULL, NULL, NULL, NULL };
    int32_t handle = 0;
    /* 1. the direct imports, bound by the loader once the sysmodule is loaded */
    direct_imports(which, fns);
    if (fns[0] && fns[1] && fns[2]) {
        api->source = 1;
    } else {
        /* 2. the module's handle (a console that exports the lookup) and its functions */
        if (!sceSysmoduleGetModuleHandleInternal || ps5_import_missing("sceSysmoduleGetModuleHandleInternal") ||
            ps5_import_missing("sceKernelDlsym")) {
            api->loaded = -3;
            api->detail = 1;
            *detail = 1;
            return -3;
        }
        int32_t lookup = 0;
        handle = sysmodule_handle(api->sysmodule, &lookup);
        if (handle < 0) {
            api->loaded = -1;
            api->detail = lookup;
            *detail = lookup;
            return -1;
        }
        for (int i = 0; i < 4; i++) {
            fns[i] = NULL;
            sceKernelDlsym(handle, api->names[i], &fns[i]);
        }
        api->source = 2;
    }
    api->init = (int (*)(void))fns[0];
    api->open = (int (*)(int, int, int, const void *))fns[1];
    api->read = (int (*)(int, void *, int))fns[2];
    api->close = (int (*)(int))fns[3];
    if (!(api->init && api->open && api->read)) {
        api->loaded = -2;
        api->detail = handle;
        *detail = handle;
        return -2;
    }
    api->loaded = 1;
    api->detail = handle;
    *detail = handle;
    return 0;
}

/* How the keyboard (0) or mouse (1) functions were found: 1 direct imports, 2 module handle. */
int ps5_hid_source(int which)
{
    return which >= 0 && which <= 1 ? g_hid[which].source : 0;
}

int ps5_hid_init(int which)
{
    return which >= 0 && which <= 1 && g_hid[which].init ? g_hid[which].init() : -1;
}

/* Type 0 (standard) at `index`; a handle, or negative. */
int ps5_hid_open(int which, int user, int index)
{
    static const uint8_t param[8] = { 0 };
    return which >= 0 && which <= 1 && g_hid[which].open ? g_hid[which].open(user, 0, index, param) : -1;
}

/* Records read (oldest first), 0 for none, negative on error. */
int ps5_hid_read(int which, int handle, void *data, int count)
{
    return which >= 0 && which <= 1 && g_hid[which].read ? g_hid[which].read(handle, data, count) : -1;
}
