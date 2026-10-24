# Ruffle5 1.000.020 (Beta)

**Ruffle5** plays Flash games on a jailbroken PS5 as a native title, a tile under Games. It runs [Ruffle](https://ruffle.rs), the Flash Player emulator, on the console's own GPU (wgpu on a statically linked RADV Vulkan driver).

## Download

[PPSA99620.zip](https://github.com/user-attachments/files/33910118/PPSA99620.zip)

## Install

1. Copy the `PPSA99620` folder to `/data/homebrew/` on the console (FTP), so that `/data/homebrew/PPSA99620/eboot.bin` exists. A **Ruffle5** tile appears under Games.
2. Put your games in `/data/homebrew/PPSA99620/games/`. That can be `.swf` files, or one folder per game with a `cover.png` or `icon.png` for its picture. A folder called `ruffle5` on a USB drive works too.

**Updating from an earlier build:**
- Copy `eboot.bin` and `sce_sys/` over the old ones. The first time, also copy the new `Custom Mappings` folder.
- Don't delete the title folder: your games are in it.
- If you used Alpha, move your games from `/data/ruffle` into the `games` folder.

## What's new since 1.000.010 (Alpha)

### A menu
Alpha played a single `movie.swf`. Ruffle5 now opens on a menu:
- **Tabs:** Games, Favorites, Recent, Badges and Settings. Switch with L1 / R1.
- **Stage:** the focused game is shown big, with what its file says (SWF version, size, frame rate, frames), above a shelf of covers.
- **Search:** L2 opens the on-screen keyboard, or type on a USB keyboard.
- **USB drive:** games in a `ruffle5` folder on the drive are listed too.
- **Screensaver:** after 2 minutes without input. Any button wakes it.
- **Close:** hold Circle to close Ruffle5.

### Ruffle's look
- Ruffle's blue and orange throughout, with Ruffle's logo beside the name.
- The ruffle.rs logo animation plays behind the menu, run by Ruffle itself. You can turn it off in Settings › App.
- A new launch icon.
- **Animation:**
  - the focused cover gets a breathing glow, a ring, and a light running round its border;
  - tiles lift and wobble when focused;
  - Ruffle-style ribbons, and sparks when you favourite a game or unlock a badge;
  - a ribbon wipe into and out of games.
- Menu sounds.

### 4K
The display now runs at 3840×2160. Settings › Display › Resolution offers two choices:
- **4K:** the game is rendered at 4K and drawn straight to the screen, with no extra copy. Flash is vector art, so this is a real 4K rendering, not an enlarged picture.
- **1080p** (the default): the game is rendered at 1080p and scaled up on the GPU. This is lighter for heavy games.

Menu text is also drawn at 4K.

### Controls
- **Controls editor:** press **Triangle** on a game, or go to Settings › Controls & Sound › Controls for all games. Pick a button, then what it does. A button can:
  - press up to four keys at once;
  - click either mouse button;
  - open the keyboard;
  - do nothing.
- **Where they're saved:** every mapping lives in `/data/homebrew/PPSA99620/Custom Mappings/`, as `<game>.controls.txt` or `_All Games.controls.txt`. They are plain text you can also edit by hand. Mapping files from the old locations are moved there automatically.
- **USB keyboard and mouse** are supported.
- **On-screen keyboard:** L2 in a game. It also opens when a game's text box is selected (you can turn this off).

### In a game
- **Quick menu:** hold **OPTIONS** (or press Esc on a USB keyboard). It has Resume, Restart game, Volume, Controls for this game, Take cover picture, FPS counter, and Back to games.
- **Saves:** games keep their saves on the console (Flash SharedObjects).
- **Covers:** a game with no picture gets a cover from its own screen after 8 seconds.
- **Media:** FLV video (H.263, VP6, Screen Video), AAC and Nellymoser audio, and LZMA-compressed SWFs.
- **Play time:** kept per game, saved every 30 seconds so it survives the console being switched off mid-game.

### Settings
- **Display:**
  - resolution;
  - quality, from Low to High (16×16) Linear;
  - scale mode;
  - letterbox;
  - stage alignment;
  - force scale mode and force alignment.
- **Player:**
  - frame rate;
  - Flash Player version;
  - runtime (Flash Player or Adobe AIR);
  - load behaviour;
  - max script time;
  - dummy ExternalInterface;
  - "Pretend to be on" for sitelocked games (Newgrounds, Kongregate, Armor Games, Miniclip, Addicting Games, Y8).
- **Controls & Sound:** controls for all games, keyboard for text boxes, USB mouse speed, game volume, menu sounds.
- **App:** animated background, screensaver, FPS counter, automatic covers, badges.
- **Library:** rescan games, clear recently played.

### Badges
There are 34 to unlock just by using the app.

### Performance
- Built with full optimisation for the PS5's Zen 2 CPU: opt-level 3, fat LTO, `znver2`.
- Sound runs on a CPU core of its own, so a busy game doesn't make it crackle.
- **Frame-time log:** every 5 seconds the log gets a `[Game] ms per loop …` line in the same format as Ruffle-Flash-PS5's, so you can compare the two players on the same game. A second line shows the resolution and fps.

## Changed
- **L2** now opens the on-screen keyboard; it was the right mouse button. To change it back, set L2 to right click in the controls editor, or put `l2 = rightclick` in a mapping file.
- **Holding OPTIONS** in a game now opens the quick menu instead of going straight back to the menu.
- **Ruffle5's own files moved** from `/data/ruffle` to `/data/ruffle5`: the log, settings, saves and covers. It no longer shares a folder with Ruffle-Flash-PS5.

## Default controls

| Pad | In a game |
|---|---|
| D-pad / left stick | Arrow keys |
| Cross / Square / Triangle / Circle | Space / Z / X / C |
| L1 / R1 | Shift / Ctrl |
| L3 / R3 | A / S |
| OPTIONS | Enter (hold: quick menu) |
| Touchpad | Mouse: touch to point, press to click |
| Right stick | Moves the pointer |
| R2 | Left click |
| L2 | On-screen keyboard |

A preset ships for Dad 'n Me.

| Pad | In the menu |
|---|---|
| D-pad / left stick | Move (hold to repeat) |
| Cross | Play / select |
| Triangle | This game's controls |
| Square | Favourite |
| L2 | Search |
| L1 / R1 | Switch tabs |
| Circle (hold) | Close Ruffle5 |

## Notes
- This is a beta. If something goes wrong, the log is at `/data/ruffle5/ruffle.log`; please attach it to an issue.
- Some games need a particular Flash version or a sitelock site. Try Settings › Player.

## Credits
- [Ruffle](https://github.com/ruffle-rs/ruffle) (MIT / Apache-2.0): the Flash player, its logo, and the ruffle.rs logo animation.
- [Ruffle-Flash-PS5](https://github.com/ZiZc3/Ruffle-Flash-PS5) by ZiZc3 (GPL-3.0): ported to Ruffle5 and rewritten:
  - USB keyboard and mouse
  - the sound's own CPU core
  - on-screen keyboard
  - quick menu
  - saves
  - covers
  - badges
  - favourites, recent and play time
  - menu sounds
  - the player options
  - the frame-time log format
- [ProsperoStore](https://github.com/darkxex/ProsperoStore) (GPL-3.0): the menu's layout and focus animation.
- [PS5_Vulkan](https://github.com/mihawk-99/PS5_Vulkan): the RADV Vulkan driver build for the PS5.
- [wgpu](https://github.com/gfx-rs/wgpu).
- The Inter and Montserrat fonts (SIL OFL 1.1).

Created by @TheUnwelcomed
