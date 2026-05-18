namespace YummiLcu.Agent;

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        var config = AgentConfig.Load();
        try
        {
            UpdateChecker.TryAutoUpdateAsync(config).GetAwaiter().GetResult();
        }
        catch
        {
            // 업데이트 실패 시 기존 버전으로 실행
        }

        Application.Run(new MainForm());
    }
}
