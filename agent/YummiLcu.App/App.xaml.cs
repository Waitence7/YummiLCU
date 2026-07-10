using System.Windows;
using YummiLcu.App.ViewModels;
using YummiLcu.Core;

namespace YummiLcu.App;

public partial class App : System.Windows.Application
{
    private static Mutex? _singleInstanceMutex;
    private AgentViewModel? _vm;
    private CancellationTokenSource? _activateListenCts;

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        if (!SingleInstanceHelper.TryAcquireMutex(out _singleInstanceMutex))
        {
            SingleInstanceHelper.SignalActivate();
            Shutdown();
            return;
        }

        var config = AgentConfig.Load();
        if (IsTestModeArg(e.Args))
            config.UiTestMode = true;

        if (!config.UiTestMode && !LeagueClientWatcher.IsClientPresent(config))
        {
            Shutdown();
            return;
        }

        if (!config.UiTestMode)
        {
            try
            {
                await UpdateChecker.TryAutoUpdateAsync(config);
            }
            catch
            {
                // 업데이트 실패 시 계속 실행
            }
        }

        _vm = new AgentViewModel(config);
        _vm.RequestApplicationShutdown += () =>
        {
            Current.Dispatcher.Invoke(() =>
            {
                if (MainWindow is MainWindow mw)
                    mw.ShutdownApplication();
            });
        };

        var main = new MainWindow { DataContext = _vm };
        MainWindow = main;
        main.Show();

        _vm.StartLeagueWatcherIfEnabled();

        _activateListenCts = new CancellationTokenSource();
        var listenCt = _activateListenCts.Token;
        _ = Task.Run(() =>
        {
            SingleInstanceHelper.ListenForActivate(() =>
            {
                Current.Dispatcher.Invoke(() =>
                {
                    if (MainWindow is MainWindow mw)
                        mw.RestoreFromTray();
                });
            }, listenCt);
        }, listenCt);

        _ = _vm.CheckUpdatesOnStartupAsync();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _activateListenCts?.Cancel();
        _activateListenCts?.Dispose();
        _vm?.Dispose();
        try { _singleInstanceMutex?.ReleaseMutex(); }
        catch (ApplicationException) { /* abandoned */ }
        _singleInstanceMutex?.Dispose();
        base.OnExit(e);
    }

    private static bool IsTestModeArg(string[] args) =>
        args.Any(a => a.Equals("--test", StringComparison.OrdinalIgnoreCase)
                   || a.Equals("-test", StringComparison.OrdinalIgnoreCase)
                   || a.Equals("/test", StringComparison.OrdinalIgnoreCase));
}
