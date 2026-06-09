using System.Threading;
using System.Windows;
using YummiLcu.App.ViewModels;
using YummiLcu.Core;

namespace YummiLcu.App;

public partial class App : System.Windows.Application
{
    private static Mutex? _singleInstanceMutex;
    private AgentViewModel? _vm;

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        _singleInstanceMutex = new Mutex(true, "YummiLcu.Agent.SingleInstance", out var created);
        if (!created)
        {
            System.Windows.MessageBox.Show(
                "Yummi Agent가 이미 실행 중입니다.",
                "Yummi Agent",
                MessageBoxButton.OK,
                MessageBoxImage.Information);
            Shutdown();
            return;
        }

        var config = AgentConfig.Load();
        if (IsTestModeArg(e.Args))
            config.UiTestMode = true;

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
        var main = new MainWindow { DataContext = _vm };
        MainWindow = main;
        main.Show();
        _ = _vm.CheckUpdatesOnStartupAsync();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _vm?.Dispose();
        _singleInstanceMutex?.ReleaseMutex();
        _singleInstanceMutex?.Dispose();
        base.OnExit(e);
    }

    private static bool IsTestModeArg(string[] args) =>
        args.Any(a => a.Equals("--test", StringComparison.OrdinalIgnoreCase)
                   || a.Equals("-test", StringComparison.OrdinalIgnoreCase)
                   || a.Equals("/test", StringComparison.OrdinalIgnoreCase));
}
