# Wi-Fi

Wi-Fi is supported in Slot+ on current BaseOS releases, and AGS-102 remains supported. The implementation uses the BaseOS-style networking environment: `wlan0`, `wpa_supplicant`, `wpa_cli`, and BusyBox DHCP.

## Connecting

From Slot+'s cartridge shelf, press MENU and open **Wi-Fi**. The first row turns Wi-Fi on or off. When on, the menu scans for visible networks. Press **X** to scan again, then select a network and press **A**.

For a password-protected network, use the D-pad and A to type. **Y** changes between lowercase, uppercase, and symbols; **X** deletes a character; **START** connects; **B** cancels. Passwords are case-sensitive. The `SP` key inserts a space. All printable ASCII characters are available. Open networks connect immediately.

The screen reports connection errors or the assigned IP address. It stays responsive while scanning or connecting; B returns to settings while the operation finishes. Slot+ saves one network; a successful connection replaces the previous one. If Wi-Fi was left on, Slot+ reconnects to that network at boot.

Turning Wi-Fi off disconnects, blocks the radio, and remembers that choice for the next boot without removing the saved network. Turn it on to scan or reconnect. **Forget** also removes the saved network and turns Wi-Fi off. Opening the menu while Wi-Fi is off does not turn it back on. BaseOS briefly enables Wi-Fi during boot before Slot+ can restore the off setting.

## Supported networks

Visible open and WPA/WPA2 personal networks are supported, including WPA2/WPA3 transition networks that offer PSK. Hidden SSIDs, WPA3-only, WEP, enterprise authentication, and captive portal sign-in are not supported. Non-UTF-8 network names are omitted. Do not run Wi-Fi setup alongside a multiplayer Wi-Fi service; Slot+ refuses to replace another supplicant.

Desktop builds display an explanatory message and never change the computer's network.

## SFTP access

For advanced, manual transfers, connect your computer to the same local network, keep the handheld awake, and use an SFTP client with the displayed IP, port **22**, username **root**, and the password configured in the OS (`root` by default). The card is mounted at `/mnt/sdcard`; GBA games go in `Games/GBA` and labels in `Labels/GBA`. Restart Slot+ after adding games.

For normal file transfers, use [File Transfer](FILE-TRANSFER.md) from another device on the same LAN.

## Persistence and security

The active card holds `System/wifi.conf` (one network) and, when Wi-Fi was left off, `System/wifi.disabled`. Preserve those files when merging future System updates. No additional startup script is needed. Network work runs on a background thread, including automatic reconnection when Slot+ starts.

The password is converted to a WPA PSK before saving; neither plaintext passwords nor keys appear in process arguments or logs. The PSK still grants network access, so treat the configuration as private. FAT/exFAT cards do not enforce Unix permissions.

## Implementation notes

Slot+ owns a separate supplicant socket under `/run/slot-wifi` and a DHCP client identified by `/run/slot-wifi-dhcp.pid`. DHCP remains running to renew its lease. The Wi-Fi feature does not change BaseOS SSH passwords or require changes to the OS image.
