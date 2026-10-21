GL probe, launched the way pacbrew's GL/SDL homebrew is meant to run: as a "big app" through websrv's
Homebrew page (not as a plain payload). A plain payload process has no direct-memory budget for the GPU
(v3's log: pool 0 MB, 16 MB ok, 64 MB EAGAIN; budget_set(0) after the fact changes nothing). websrv's
homebrew loader starts a title process (sceSystemServiceLaunchApp "FAKE00000"), sets the budget before the
ELF runs, and replaces the process image with the ELF — the process then has a game's memory and the display.

1. FTP the GLProbe folder to the console:   /data/homebrew/GLProbe/eboot.elf  (+ sce_sys/icon0.png)
   (the Mesa libraries stay in /data/ruffle/lib as before)
2. Start websrv: websrv/websrv.elf is pacbrew v0.41.1's (0.34.r9); launch it from your payload manager like any
   payload (if your manager already offers websrv, that one is fine too). It needs elfldr running for its
   payload page, but the Homebrew page does not.
3. On a phone or PC on the same network open   http://<PS5-IP>:8080/index.html   and click "GLProbe".
   (A running game or title will be closed by the launch — that is how the big-app loader works.)
4. Watch the TV; then fetch /data/ruffle/gl-probe.log.
