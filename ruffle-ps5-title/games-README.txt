Ruffle5 — games folder
======================

Put Flash games here, then open Ruffle5: the menu lists them.

  games/dadnme.swf                 a game
  games/dadnme.png                 its picture on the menu (optional; any PNG, 16:9 looks best)
  games/Some Game/game.swf         or a game in its own folder (files it loads go beside it)...
  games/Some Game/cover.png        ...with its picture (cover.png or icon.png)

Controls for a game: games/<name>.controls.txt (see controls.txt in the title folder for the
format), or /data/ruffle/<name>.controls.txt. Presets for known games ship in the title folder.

When you update Ruffle5, copy the new eboot.bin over the old one — don't delete the title folder,
this games folder is in it.
