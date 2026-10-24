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

## 배포 방식

Game Bar 오버레이는 Yummi LCU Agent 설치파일에 포함하지 않습니다. `.github/workflows/build-gamebar-widget.yml`이 별도의 Windows UWP sideload 패키지를 빌드하며, 가능한 경우 현재 버전과 일치하는 GitHub Release에 `Yummi-GameBar-Overlay-<version>.zip`을 업로드합니다.

사용자는 Yummi LCU Agent 설치 후 오버레이 ZIP을 별도로 받아 압축을 풀고 `Install-YummiGameBar.ps1`을 실행합니다. 설치가 끝나면 `Win+G` → 위젯 메뉴 → `유미 LCU`에서 실행할 수 있습니다.
## 위젯 위치 이동

Xbox Game Bar SDK는 위젯 콘텐츠에서 호스트 창 좌표를 직접 변경하는 API를 제공하지 않습니다. `Win+G`로 Game Bar를 연 뒤 위젯의 Game Bar 제목 표시줄을 드래그해 위치를 옮기고, 원하는 위치에서 핀 고정하세요. 위치는 Game Bar가 관리합니다.

