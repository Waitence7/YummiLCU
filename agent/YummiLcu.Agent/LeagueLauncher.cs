using System.Diagnostics;

namespace YummiLcu.Agent;

/// <summary>Riot Client로 리그 오브 레전드 실행 (LCU 미연결 시).</summary>
internal static class LeagueLauncher
{
    private const string LaunchArgs = "--launch-product=league_of_legends --launch-patchline=live";

    public static (bool Ok, string Message) TryLaunch()
    {
        var exe = FindRiotClientServices();
        if (exe is not null)
        {
            try
            {
                Process.Start(new ProcessStartInfo
                {
                    FileName = exe,
                    Arguments = LaunchArgs,
                    UseShellExecute = false,
                });
                return (true, "롤 클라이언트 실행 요청");
            }
            catch (Exception ex)
            {
                return (false, $"실행 실패: {ex.Message}");
            }
        }

        try
        {
            Process.Start(new ProcessStartInfo("riotclient://launch product=league_of_legends patchline=live")
            {
                UseShellExecute = true,
            });
            return (true, "롤 클라이언트 실행 요청 (riotclient://)");
        }
        catch (Exception ex)
        {
            return (false, $"Riot Client를 찾을 수 없습니다. ({ex.Message})");
        }
    }

    private static string? FindRiotClientServices()
    {
        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var candidates = new List<string>
        {
            Path.Combine(localAppData, "Riot Games", "Riot Client", "RiotClientServices.exe"),
        };

        foreach (var drive in new[] { "C", "D", "E", "F" })
        {
            candidates.Add($@"{drive}:\Riot Games\Riot Client\RiotClientServices.exe");
            candidates.Add($@"{drive}:\Program Files\Riot Games\Riot Client\RiotClientServices.exe");
        }

        return candidates.FirstOrDefault(File.Exists);
    }
}
