# Updating Slot+

Slot+ can check GitHub for a release and install it from the handheld. Manual updates are also available from the [latest Slot+ release](https://github.com/philbudiman/slot-plus/releases/latest).

## Requirements

- An Anbernic RG SP.
- Slot+ running on BaseOS or AGS-102.
- A working Wi-Fi connection and access to GitHub.

## Checking for updates

From the carousel, press MENU to open the quick menu and choose **About**. Press **A** on the About screen to open the updater. It checks the latest release and displays its release notes; when an update is available, press **A** to install it.

## What happens during an update

Slot+:

1. Queries the latest Slot+ GitHub release and compares its build tag with the current build.
2. Shows the release notes.
3. Downloads the standalone `slot-h700` asset.
4. Checks that the file size matches the release metadata.
5. Checks that the file is a 64-bit, little-endian AArch64 ELF binary.
6. Verifies its SHA-256 digest against the release metadata.
7. Copies the installed binary to `System/slot.previous`.
8. Replaces `System/slot` with the verified download.
9. Exits so BaseOS can start the newly installed binary.

The update is downloaded first as `System/slot.download`. A failed download or verification removes the temporary file and leaves the current `System/slot` intact. The previous binary is kept as `System/slot.previous` so it can be restored manually if needed.

## Manual update

If the on-device updater cannot reach GitHub, download the latest H700 `-h700.zip` release archive from [GitHub Releases](https://github.com/philbudiman/slot-plus/releases/latest). Extract it, then replace `System/slot` on the Slot+ card with the `System/slot` binary from the archive. Keep other files in `System/`, including Wi-Fi settings, intact. Safely eject the card and boot the handheld.
