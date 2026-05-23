# Yummi Client Foundation Work Summary

## Scope

This document summarizes the Phase 1 foundation work completed for Yummi Client, including app state, events, LCU state monitoring, pet placeholder foundation, atmosphere reactions, audio foundation, reusable UI interactions, settings, and developer debugging tools.

## Architecture Foundation

Created a lightweight foundation layer without introducing a full dependency injection framework.

Added `AppServices` as the simple composition root for app-wide services:

- `AppStateManager`
- `EventBus`
- `ShellStateCoordinator`
- `LcuStateMonitor`
- `ThemeManager`
- `AnimationManager`
- `PetController`
- `AtmosphereController`
- `AudioManager`
- `InteractionPreferencesService`

This keeps service construction centralized while staying small and easy to replace later.

## App State

Added `AppStateManager` and `AppStateSnapshot` as the central observable state holder.

Tracked state includes:

- current theme
- LCU connection state
- current League game state
- current atmosphere state
- current pet state
- relay running state
- test mode
- current page
- interaction preferences

The state manager is intentionally simple and does not own feature behavior.

## Event System

Added a minimal `EventBus` with strongly typed app events.

Current events include:

- `ThemeChangedEvent`
- `TestModeChangedEvent`
- `LcuConnectionChangedEvent`
- `RelayStateChangedEvent`
- `NavigationChangedEvent`
- `AppGameStateChangedEvent`
- `PetStateChangedEvent`
- `AtmosphereStateChangedEvent`

Events are used so future systems like pet, audio, UI effects, and Discord RPC can react without being directly coupled to `ShellViewModel`.

## Shell State Coordinator

Added `ShellStateCoordinator` to move shell-level state publishing out of `ShellViewModel`.

It handles:

- test mode state updates
- LCU connection state updates
- relay running state updates
- current page updates
- publishing semantic events
- duplicate-event suppression

This reduced `ShellViewModel` responsibility and keeps it focused on commands, navigation, and binding.

## LCU State Monitoring

Added `LcuStateMonitor` to keep future League state reactions out of `ShellViewModel`.

It observes the existing LCU connector and maps League gameflow phases into `AppGameState`.

Gameflow mapping:

- no connection -> `Disconnected`
- `None` / `Lobby` -> `Lobby`
- `Matchmaking` -> `Queue`
- `ReadyCheck` -> `MatchFound`
- `ChampSelect` -> `ChampionSelect`
- `InProgress` -> `InGame`
- `PreEndOfGame` / `EndOfGame` / `WaitingForStats` -> `EndOfGame`
- unclear values -> `Unknown`

Duplicate game-state events are suppressed.

The core LCU connector interface was extended with `GetGameflowPhaseAsync()` and the app connector implementation delegates to the existing LCU client method.

## Pet System Foundation

Added a minimal pet foundation without real sprites, audio, or physics.

Added:

- `PetState`
- `PetController`
- `IPetSystem`

`PetController` listens to `AppGameStateChangedEvent` and maps game states to pet states:

- `Lobby` -> `Idle`
- `Queue` -> `Waiting`
- `MatchFound` -> `Excited`
- `ChampionSelect` -> `Focused`
- `InGame` -> `Hidden`
- `EndOfGame` -> `Curious`
- `Disconnected` -> `Sleeping`
- `Unknown` -> `Idle`

Duplicate pet-state changes are suppressed.

The shell exposes `CurrentPetState` so the UI can bind to it.

## Pet Placeholder UI

Added a minimal bottom-right pet placeholder in the shell.

It uses simple WPF transforms only:

- `ScaleTransform`
- `TranslateTransform`
- `RotateTransform`
- `Opacity`

Interactions:

- hover gently scales and brightens the placeholder
- click performs a short bounce/pulse
- pet-state changes trigger subtle visual changes
- hidden state fades out and disables hit testing

No real image assets were added.

## Atmosphere Reactions

Added a lightweight atmosphere system without particles, blur, or heavy effects.

Added:

- `AtmosphereState`
- `AtmosphereController`

`AtmosphereController` listens to `AppGameStateChangedEvent` and maps game state to atmosphere state:

- `Disconnected` -> `Dimmed`
- `Lobby` -> `Calm`
- `Queue` -> `Active`
- `MatchFound` -> `Alert`
- `ChampionSelect` -> `Focused`
- `InGame` -> `Resting`
- `EndOfGame` -> `Result`
- `Unknown` -> `Neutral`

The shell uses low-opacity accent overlays and short opacity animations for subtle reactions.

Atmosphere reactions now check `EnableAtmosphereReactions` before reacting.

## Audio Foundation

Added a minimal no-op-safe audio foundation.

Added:

- `IAudioSystem`
- `AudioManager`
- `AudioCue`

Supported methods:

- `PlayHover()`
- `PlayClick()`
- `PlayNotification()`
- `PlayMatchFound()`
- `PlayStateChanged(AppGameState state)`
- `SetMuted(bool)`
- `SetVolume(double)`

No audio files were added.

The implementation safely returns if sounds are muted or assets do not exist yet.

## Theme And Animation Foundation

Added:

- `IThemeManager`
- `ThemeManager`
- `IAnimationManager`
- `AnimationManager`
- `MotionTokens`

Themes are now applied through the manager instead of direct theme service calls.

Page transitions go through `AnimationManager`, which checks `EnableUiAnimations` before animating.

## Reusable Interactive UI Styles

Updated shared WPF styles for common UI interactions.

Added subtle interactions for:

- navigation buttons
- accent buttons
- title bar buttons
- panel/card borders

Interactions use simple opacity and transform animations.

No heavy blur, particle effects, or expensive shadows were added.

## Settings Page

Added a minimal Settings page integrated into the existing navigation structure.

Settings include:

- enable pet placeholder
- enable UI animations
- enable atmosphere reactions
- enable sounds
- sound volume placeholder

Safe defaults:

- pet placeholder enabled
- UI animations enabled
- atmosphere reactions enabled
- sounds disabled
- volume set to 50%

Settings are exposed through `SettingsViewModel`.

Settings are persisted as local JSON at:

`AppContext.BaseDirectory/yummi-interactions.json`

The settings service catches persistence errors so the app does not crash if saving fails.

## Developer Debug Panel

Added a developer/debug section to the Settings page for testing game-state reactions without the League Client.

It can simulate:

- `Disconnected`
- `Lobby`
- `Queue`
- `MatchFound`
- `ChampionSelect`
- `InGame`
- `EndOfGame`
- `Unknown`

The debug panel uses `LcuStateMonitor.SetDebugGameState(...)`, so simulated states go through the same state pipeline as real LCU updates:

Settings button -> `SettingsViewModel` -> `LcuStateMonitor` -> `AppStateManager` -> `AppGameStateChangedEvent` -> pet, atmosphere, audio, and shell UI reactions.

## ShellViewModel Changes

`ShellViewModel` now focuses more on:

- navigation
- UI commands
- bindings
- relay commands
- shell status text

Moved or delegated responsibilities:

- app-state publishing -> `ShellStateCoordinator`
- LCU game-state monitoring -> `LcuStateMonitor`
- pet reactions -> `PetController`
- atmosphere reactions -> `AtmosphereController`
- audio reactions -> `AudioManager`
- preferences persistence -> `InteractionPreferencesService`

## Performance Notes

The current implementation stays lightweight by:

- using WPF transforms instead of layout-changing animation where possible
- avoiding heavy blur effects
- avoiding particle systems
- avoiding real-time loops for UI effects
- suppressing duplicate state events
- using short, subtle storyboards
- keeping audio no-op until real assets exist

## Build Status

Verified:

- `dotnet build agent/YummiLcu.Core/YummiLcu.Core.csproj` succeeds.

Not fully verifiable in this Linux environment:

- `dotnet build agent/YummiLcu.App/YummiLcu.App.csproj`

The app build fails because this environment does not have the WPF WindowsDesktop SDK target installed:

`Microsoft.NET.Sdk.WindowsDesktop`

This is an environment limitation. The WPF app should be built on Windows with the appropriate .NET desktop workload installed.

## Important Notes

No Discord behavior was added.

No real pet sprites or image assets were added.

No real audio assets were added.

No complex physics were added.

No full dependency injection framework was introduced.

The foundation is intentionally minimal and designed so future systems can subscribe to app events instead of coupling directly to shell UI code.
