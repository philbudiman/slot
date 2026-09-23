# Browser file transfer

Connect through Slot+'s quick menu → **Wi-Fi**, then open **File Transfer**. Slot+ shows an address such as `http://192.168.1.42:8080/` and an eight-digit session code. Open the address on a computer or phone on the same local network and enter the code.

Choose a destination folder, then drag files onto the page or use **Choose files**. Multiple files upload one at a time with progress. The page asks before replacing an existing file. Files in the supported folders can also be downloaded.

| Folder | Allowed file type |
| --- | --- |
| `Games/GBA` | `.gba` |
| `Games/GB` | `.gb` |
| `Games/GBC` | `.gbc` |
| `Labels/GBA`, `Labels/GB`, `Labels/GBC` | `.png` |
| `BIOS` | `.bin` |

Files must be nonempty and no larger than 128 MiB. Labels should have the same filename stem as their game. ZIP files need to be extracted on your computer first. The server does not expose System files, Wi-Fi credentials, saves, or save states, and it does not expose arbitrary filesystem paths.

Keep the File Transfer screen open and the handheld awake while transferring. **B** or **MENU** stops the server and returns to settings. Closing the lid, restarting, or powering off also stops it. A new session gets a new code. Restart Slot+ after adding games or labels to reload the library.

Uploads are streamed and synced to temporary files on the card, then published with a rename. Only completed uploads replace their destination; a disconnect, cancellation, or full card preserves the original. If a response is lost after completion, refresh the folder to check the result.

The page and server are built into the `slot` binary. No extra app, cloud service, startup script, or web server package is needed. This is local-only HTTP on your LAN with a session code; use it on a trusted home network, not through port forwarding.

## Test builds

The transfer page can upload a compatible AArch64 Slot binary. Choose **Install a test build**, select the binary, and upload it while the handheld remains on the File Transfer screen. The server checks that it is a nonempty, at most 128 MiB, 64-bit little-endian AArch64 ELF binary, then stages it as `System/slot.upload`. It does not replace the running binary during upload.

On the handheld, press **A** to review the install and press **A** again to confirm (or **B** to cancel). Slot+ checks the staged file again, backs up the current binary as `System/slot.previous`, installs the test build as `System/slot`, and restarts into it. The architecture check confirms the binary format; it does not establish who built or signed the binary.

## Development verification

`cargo test -p slot --no-default-features --test transfer --test transfer_menu` exercises loopback uploads, downloads, explicit replacement, interrupted writes, stop during upload, PIN/rate limits, Origin/Host checks, file types, traversal and symlink rejection, plus settings navigation. Tests use temporary card folders.

`cargo run -p slot --no-default-features --example transfer_server -- <fixture-card>` serves an explicitly supplied fixture card at `127.0.0.1:8080` for browser testing.
