# Wi-Fi

Wi-Fi is supported in Slot+ on current BaseOS releases, and AGS-102 remains supported. The implementation uses the BaseOS-style networking environment: `wlan0`, `wpa_supplicant`, `wpa_cli`, and BusyBox DHCP.

## Connecting

From Slot+'s cartridge shelf, press MENU and open **Wi-Fi**. The first row turns Wi-Fi on or off. Press **X** to scan for visible networks, then select a network and press **A**. Opening the page checks connection status but does not scan.

For a password-protected network, use the D-pad and A to type. **Y** changes between lowercase, uppercase, and symbols; **X** deletes a character; **START** connects; **B** cancels. Passwords are case-sensitive. The `SP` key inserts a space. All printable ASCII characters are available. Open networks connect immediately.

The screen reports connection errors or the assigned IP address. It stays responsive while scanning or connecting; B returns to settings while the operation finishes. The current network appears directly below **Saved networks** with a **[Connected]** label, even after a scan. Select it and press **Y** to disconnect while leaving Wi-Fi on and keeping its saved profile. Successful connections are saved. Open **Saved networks** to see them alphabetically; press **A** to connect to one or **X** to forget it. If Wi-Fi was left on, Slot+ reconnects to an available saved network at boot.

Turning Wi-Fi off disconnects, blocks the radio, and remembers that choice for the next boot without removing saved networks. You can view and forget saved networks while Wi-Fi is off, but must turn it on to scan or connect. Opening the menu while Wi-Fi is off does not turn it back on. BaseOS briefly enables Wi-Fi during boot before Slot+ can restore the off setting.

## Supported networks

Visible open and WPA/WPA2 personal networks are supported, including WPA2/WPA3 transition networks that offer PSK. Hidden SSIDs, WPA3-only, WEP, enterprise authentication, and captive portal sign-in are not supported. Non-UTF-8 network names are omitted. Do not run Wi-Fi setup alongside a multiplayer Wi-Fi service; Slot+ refuses to replace another supplicant.

Desktop builds display an explanatory message and never change the computer's network.

## Optional SFTP access

Use [File Transfer](FILE-TRANSFER.md) for normal game and label transfers. It is built into Slot+ and runs only while its screen is open.

Slot+ does not start or check an SFTP server. If BaseOS has its SSH/SFTP service enabled, advanced users can connect from the same local network using the displayed IP, port **22**, username **root**, and the password configured in the OS. The card is mounted at `/mnt/sdcard`. Restart Slot+ after adding games.

## Persistence and security

The active card holds `System/wifi.conf` (all saved networks) and, when Wi-Fi was left off, `System/wifi.disabled`. Existing single-network files remain valid. Older Slot+ builds can read this file, but connecting through an older build replaces its saved networks with one. Preserve these files when merging future System updates. No additional startup script is needed. Network work runs on a background thread, including automatic reconnection when Slot+ starts.

The password is converted to a WPA PSK before saving; neither plaintext passwords nor keys appear in process arguments or logs. The PSK still grants network access, so treat the configuration as private. FAT/exFAT cards do not enforce Unix permissions.

## Implementation notes

Slot+ owns a separate supplicant socket under `/run/slot-wifi` and a DHCP client identified by `/run/slot-wifi-dhcp.pid`. DHCP remains running to renew its lease. The Wi-Fi feature does not change BaseOS SSH passwords or require changes to the OS image.
