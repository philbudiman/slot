# Slot+

A Game Boy-centric frontend for the Anbernic RG SP, based on [Slot by Brandon T. Kowalski](https://github.com/BrandonKowalski/slot).

Slot+ keeps Slot's cartridge-inspired design while adding built-in Wi-Fi management, browser-based wireless file transfer, on-device updates, and ongoing independent maintenance.

![Slot+ running on an Anbernic RG SP](site/media/main.webp)

## About

Games appear as cartridges on a shelf. Insert a cartridge to launch it; eject it to return to the shelf and save state. Slot+ is deliberately focused rather than a general-purpose frontend for every emulator and system.

Supported systems are Game Boy Advance, Game Boy Color, and Game Boy. Games run through mGBA or gpSP using libretro.

## What's different in Slot+

### Wi-Fi management

Configure Wi-Fi directly on the handheld. See the [Wi-Fi guide](docs/WIFI.md).

### Browser file transfer

Upload and download games and related files from another device on the same local network. See [File Transfer](docs/FILE-TRANSFER.md).

### On-device updates

Check GitHub and install verified releases from the handheld. See [Updating Slot+](docs/UPDATES.md).

## Installation

Choose a system for SD1:

- [BaseOS](https://github.com/pvaibhav/BaseOS) is recommended for new installs. Current BaseOS releases natively recognize Slot's frontend layout, which Slot+ continues to use; no Slot-specific launcher layer is needed.
- [AGS-102](https://github.com/BrandonKowalski/AGS-102) remains supported for existing users.

Then set up SD2:

1. Download the latest Slot+ [H700 release archive](https://github.com/philbudiman/slot-plus/releases/latest) (the `-h700.zip` asset).
2. Format SD2 as FAT32 or exFAT with an MBR partition scheme.
3. Extract the archive and copy the contents of its release directory to the root of SD2.
4. Confirm the card contains `BIOS/`, `Games/`, `Labels/`, and `System/`. The frontend binary remains `System/slot`.
5. Add ROMs under `Games/GBA`, `Games/GBC`, or `Games/GB`. Optional labels go in matching `Labels/...` folders; optional BIOS files go in `BIOS/`.

ROMs and BIOS files are not included.

## Using Slot+

**Carousel:** Left / Right browse; L1 / R1 switch platform; tap A resumes the latest save state; hold A starts fresh; START chooses the emulator core; MENU opens settings.

**In game:** Hold MENU to save, eject, and return; double-tap MENU opens the save-state switcher; SELECT + R1 saves; SELECT + L1 loads the most recent state; hold L2 to rewind; hold R2 to fast-forward; double-tap R2 locks or unlocks fast-forward; SELECT + MENU opens the link menu.

Slot+ also supports save-state history and undo, configurable fast-forward, colour correction, rumble, brightness and blue-light controls, lid detection, per-game emulator selection, and supported GBA multiplayer linking.

## Documentation

| Guide | Description |
| --- | --- |
| [Wi-Fi](docs/WIFI.md) | Connect and configure Wi-Fi; includes advanced SFTP access. |
| [File Transfer](docs/FILE-TRANSFER.md) | Transfer files in a browser, including compatible test builds. |
| [Updating Slot+](docs/UPDATES.md) | Use the on-device updater or update manually. |

## Development

Common Task commands:

```sh
task build
task test
task check
task sdcard
task run
task build:device
task dist:device
```

Run `task --list` to see all available tasks.

## Project lineage

Slot+ began as a fork of [Brandon T. Kowalski's original Slot](https://github.com/BrandonKowalski/slot) and is now maintained independently at [philbudiman/slot-plus](https://github.com/philbudiman/slot-plus). Slot+ does not claim original authorship. Its aim is to preserve Slot's deliberately focused design while carrying additional features and maintenance; upstream improvements may still be incorporated where appropriate.

## AI disclosure

The original Slot project discloses that its Rust frontend was developed with assistance from Claude Opus and reviewed by Brandon Kowalski. Slot+ changes have been developed with assistance from OpenAI Codex and reviewed by the maintainer.

## Credits and license

With thanks to Brandon T. Kowalski and the original Slot project; the mGBA, gpSP, and libretro projects; BaseOS and its maintainer pvaibhav; and AGS-102 for users who continue to use it.

Slot+ remains under the MIT License. The original copyright notice is preserved in [LICENSE](LICENSE).
