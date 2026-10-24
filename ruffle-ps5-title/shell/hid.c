/* PS5_RUFFLE_HID_V1 — a USB keyboard and mouse, for src/hid.rs.
 * SPDX-License-Identifier: GPL-3.0-or-later
 *
 * libSceKeyboard and libSceMouse are not loaded for a title. As Ruffle-Flash-PS5 does it
 * (ps5/scripts/ps5_early.c, GPL-3.0, by ZiZc3, after the device-tested
 * ps5-native-gamepad-input-research probe): load each through its sysmodule id (0x0106 keyboard,
 * 0x00a9 mouse), find the module among the loaded ones (or load its .sprx), and look its four
 * functions up with sceKernelDlsym. Nothing is imported from those libraries, so a console where
 * a step fails just has no keyboard or mouse (the log says which step).
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
int sceKernelGetModuleList(int *handles, size_t count, size_t *actual);
int sceKernelGetModuleInfo(int handle, void *info);
int sceKernelDlsym(int handle, const char *symbol, void **address);
int sceKernelLoadStartModule(const char *path, size_t argc, const void *argv, uint32_t flags, void *opt, int *result);

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
    const char *module;
    const char *names[4]; /* init, open, read, close */
    int (*init)(void);
    int (*open)(int user, int type, int index, const void *param);
    int (*read)(int handle, void *data, int count);
    int (*close)(int handle);
    int loaded; /* 0 not tried, 1 ready, <0 the step that failed */
    int detail;
};

static struct hid_api g_hid[2] = {
    { 0x0106, "libSceKeyboard", { "sceKeyboardInit", "sceKeyboardOpen", "sceKeyboardRead", "sceKeyboardClose" }, 0, 0, 0, 0, 0, 0 },
    { 0x00a9, "libSceMouse", { "sceMouseInit", "sceMouseOpen", "sceMouseRead", "sceMouseClose" }, 0, 0, 0, 0, 0, 0 },
};

/* The handle of a loaded module whose name contains `module`, or -1. */
static int find_module(const char *module)
{
    static int handles[512];
    size_t count = 0;
    if (sceKernelGetModuleList(handles, sizeof(handles) / sizeof(handles[0]), &count) != 0)
        return -1;
    /* SceKernelModuleInfo's size differs between systems: try a few. */
    static const uint64_t sizes[] = { 0x160, 0x1a8, 0x1b0, 0x158, 0x200 };
    for (size_t i = 0; i < count; i++) {
        for (size_t k = 0; k < sizeof(sizes) / sizeof(sizes[0]); k++) {
            struct { uint64_t size; char name[256]; uint8_t rest[0x400]; } info;
            memset(&info, 0, sizeof(info));
            info.size = sizes[k];
            if (sceKernelGetModuleInfo(handles[i], &info) != 0)
                continue;
            info.name[sizeof(info.name) - 1] = 0;
            if (strstr(info.name, module))
                return handles[i];
            break;
        }
    }
    return -1;
}

/* 0 when the keyboard (0) or mouse (1) functions are ready; else the failed step (<0). */
int ps5_hid_load(int which, int32_t *detail)
{
    if (which < 0 || which > 1)
        return -9;
    struct hid_api *api = &g_hid[which];
    if (api->loaded) {
        *detail = api->detail;
        return api->loaded > 0 ? 0 : api->loaded;
    }
    static const char *const needed[] = { "sceSysmoduleLoadModule", "sceKernelGetModuleList", "sceKernelGetModuleInfo",
                                          "sceKernelDlsym", "sceKernelLoadStartModule" };
    for (size_t i = 0; i < sizeof(needed) / sizeof(needed[0]); i++) {
        if (ps5_import_missing(needed[i])) {
            api->loaded = -3;
            api->detail = (int)i;
            *detail = (int)i;
            return -3;
        }
    }
    api->detail = sceSysmoduleLoadModule(api->sysmodule);
    int handle = find_module(api->module);
    if (handle < 0) {
        char path[96];
        snprintf(path, sizeof(path), "/system/common/lib/%s.sprx", api->module);
        handle = sceKernelLoadStartModule(path, 0, NULL, 0, NULL, NULL);
        if (handle < 0) {
            api->loaded = -1;
            api->detail = handle;
            *detail = handle;
            return -1;
        }
    }
    void *fns[4] = { NULL, NULL, NULL, NULL };
    for (int i = 0; i < 4; i++)
        sceKernelDlsym(handle, api->names[i], &fns[i]);
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
    *detail = handle;
    return 0;
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
