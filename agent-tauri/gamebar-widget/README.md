# Yummi LCU Xbox Game Bar widget

Windows Xbox Game Bar hosts this UWP XAML widget. It is intentionally a thin, read-only UI: the existing Tauri LCU agent remains the owner of Riot lockfile credentials and publishes a sanitized snapshot through `\\.\pipe\LOCAL\YummiLcuGameBarV1`.

## Build / sideload

Open `YummiGameBarWidget.csproj` in Visual Studio 2022 on Windows with **Universal Windows Platform development** and a Windows 10/11 SDK installed. Restore NuGet packages, build x64, then deploy/sideload the package. Press **Win+G**, open the widget menu, choose **유미 LCU**, and pin it if desired.

The agent must be running. No Riot LCU password, Discord token, OAuth secret, or relay credential is sent to the widget.

## Interaction test panel

The widget includes a small test area for validating Game Bar input behavior before production controls are added:

- click counter and reset button
- ToggleSwitch
- Slider
- tap, double-tap, and right-click detection
- mouse/touch drag and swipe detection
- manipulation inertia test inside the widget
- expandable content and vertical scrolling

These controls are local UI tests only and do not send LCU write commands.
