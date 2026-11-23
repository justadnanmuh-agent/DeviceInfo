# Hardware, Device Manager & Task Manager Monitor (`hw-monitor`)

Aplikasi monitoring hardware, Device Manager, dan Task Manager lokal berbasis Rust untuk Windows 10 dan Windows 11. Dikompilasi menjadi 1 file `.exe` standalone dengan UI `index.html` yang sudah di-embed via `include_str!`.

## Fitur Utama
- **Hardware & Port Scanner (WMI, di-cache saat startup / rescan):**
  - CPU (`Win32_Processor`): model, physical cores, logical threads, base clock.
  - VGA / GPU (`Win32_VideoController`): deteksi iGPU vs dedicated GPU (NVIDIA/AMD/Intel Arc), VRAM, driver, status.
  - LAN / Network Card (`Win32_NetworkAdapter`): adaptor fisik, link speed, status kabel tersambung/putus.
  - Status Port Layar / HDMI (`root\wmi` `WmiMonitorConnectionParams`): deteksi HDMI, DisplayPort, eDP/Internal aktif atau kosong (aman jika tidak didukung VM/PC).
  - USB 3.0 vs USB 2.0 (`Win32_USBController` + `Win32_USBControllerDevice` + `Win32_PnPEntity`): controller xHCI vs EHCI serta perangkat USB yang sedang tercolok.
  - Device Manager Lengkap (`Win32_PnPEntity`): dikelompokkan berdasarkan `PNPClass`, mendukung search, filter kategori, filter `Error`, dan highlight `ConfigManagerErrorCode != 0`.
- **Live Task Manager (`sysinfo`, polling ringan 1–2 detik):**
  - CPU usage (%), frekuensi, uptime, RAM terpakai/sisa, Disk usage & aktivitas R/W, Network upload/download per interface, serta Top Processes (sortable by CPU/RAM).
- **Optimize & Debloat (dilindungi session token CSRF):**
  - **Startup Manager (`HKCU\...\Run`):** Enable/Disable aplikasi startup secara aman dan reversible.
  - **Safe Windows Debloat Toggles (`HKCU`):** Matikan Windows Widgets, Bing Web Search di Start Menu, Start Tips/Ads, dan batasi Background Apps.
  - **Temp Cleaner:** Bersihkan file sementara di `%TEMP%` secara aman.
  - **Process Group Killer:** Tutup grup proses background sekaligus (dengan proteksi otomatis untuk proses inti OS).
- **Web Server Ringan (`tiny_http`):**
  - Otomatis berjalan di `http://127.0.0.1:8080` dengan fallback port (`8081..8100` atau port acak) jika 8080 sedang dipakai.

## Cara Build

Untuk target MSVC (Windows 10/11 standalone tanpa runtime tambahan):
```powershell
cargo build --release --target x86_64-pc-windows-msvc
```
Output binary berada di `target\x86_64-pc-windows-msvc\release\hw-monitor.exe`.

Atau dengan toolchain default yang terpasang:
```powershell
cargo build --release
```
Output binary berada di `target\release\hw-monitor.exe`.

## Paket Rilis Portable (Zero-Install)
Cukup salin 2 file berikut ke folder mana saja di PC Windows 10/11 target:
- `hw-monitor.exe`
- `run.bat`

Klik dua kali `run.bat` — server lokal akan berjalan dan browser default otomatis terbuka ke URL dashboard.
