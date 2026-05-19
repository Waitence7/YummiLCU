using System.Windows;
using YummiLcu.App.Services;
using YummiLcu.App.ViewModels;
using YummiLcu.Core;
using YummiLcu.Core.Lcu;

namespace YummiLcu.App;

public partial class App : Application
{
    private ShellViewModel? _shell;

    protected override async void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);
        ThemeService.Apply(AppTheme.Cat);

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

        var lcu = new LcuConnector();
        if (config.UiTestMode)
            lcu.SetTestMode(true);

        _shell = new ShellViewModel(config, lcu);
        var main = new MainWindow { DataContext = _shell };
        MainWindow = main;
        ModalOverlayService.Initialize(main);
        main.Show();
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _shell?.Dispose();
        base.OnExit(e);
    }

    private static bool IsTestModeArg(string[] args) =>
        args.Any(a => a.Equals("--test", StringComparison.OrdinalIgnoreCase)
                   || a.Equals("-test", StringComparison.OrdinalIgnoreCase)
                   || a.Equals("/test", StringComparison.OrdinalIgnoreCase));
}
