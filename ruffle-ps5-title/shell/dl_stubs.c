/* PS5_RUFFLE_DLSTUBS_V1 — a title cannot dlopen (the SDK's loader makes raw syscalls, which a
 * title may not execute, and the sandbox has no modules for it to find). ash's "loaded" feature,
 * which wgpu-hal turns on, references these through libloading; nothing calls them (src/gpu.rs
 * builds ash's Entry from the statically linked RADV), but the link must resolve them. They answer
 * "no" and bind before any stub library's names (--version-script keeps them local).
 * SPDX-License-Identifier: GPL-3.0-or-later */
void *dlopen(const char *file, int mode) { (void)file; (void)mode; return 0; }
void *dlsym(void *handle, const char *symbol) { (void)handle; (void)symbol; return 0; }
int dlclose(void *handle) { (void)handle; return -1; }
const char *dlerror(void) { return "dlopen is not available in a PS5 title"; }
