Ruffle5 — games folder
======================

Put Flash games here, then open Ruffle5: the menu lists them.

  games/dadnme.swf                 a game
  games/dadnme.png                 its picture on the menu (optional; any PNG, 16:9 looks best)
  games/Some Game/game.swf         or a game in its own folder (files it loads go beside it)...
  games/Some Game/cover.png        ...with its picture (cover.png or icon.png)

A USB drive works too: put the games in a folder called ruffle5 on it.
A game with no picture gets one from its own screen after 8 seconds of play (Settings > App >
Automatic covers), or when you pick "Cover" in the quick menu (hold OPTIONS in a game).

Controls: press Triangle on a game in the menu to change its controls, or Settings > Controls &
Sound > Controls for all games. They are saved in the "Custom Mappings" folder next to this one
(<game>.controls.txt, and "_All Games.controls.txt"), where the shipped presets are too; the
format is in controls.txt in the title folder if you want to edit one by hand.

When you update Ruffle5, copy the new eboot.bin (and sce_sys) over the old ones — don't delete the
title folder: this games folder and Custom Mappings are in it.
