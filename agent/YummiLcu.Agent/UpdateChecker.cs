using System.Reflection;
using System.Text.Json;

namespace YummiLcu.Agent;

internal static class UpdateChecker
{
    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(8) };

    public static string CurrentVersion =>
        Assembly.GetExecutingAssembly().GetName().Version?.ToString(3) ?? "0.0.0";

    public static async Task<UpdateInfo?> CheckAsync(string manifestUrl, CancellationToken ct = default)
    {
        try
        {
            var json = await Http.GetStringAsync(manifestUrl, ct);
            var info = JsonSerializer.Deserialize<UpdateInfo>(json, new JsonSerializerOptions
            {
                PropertyNameCaseInsensitive = true,
            });
            if (info is null || string.IsNullOrWhiteSpace(info.Version))
                return null;
            if (Version.Parse(info.Version) <= Version.Parse(CurrentVersion))
                return null;
            return info;
        }
        catch
        {
            return null;
        }
    }

    /// <summary>새 버전이면 다운로드·교체·재시작. 성공 시 프로세스가 곧 종료됨.</summary>
    public static async Task<(bool Updating, string Message)> TryAutoUpdateAsync(
        AgentConfig config,
        CancellationToken ct = default)
    {
        if (!config.CheckUpdatesOnStartup || !config.AutoUpdateEnabled)
            return (false, "");
        if (string.IsNullOrWhiteSpace(config.UpdateManifestUrl))
            return (false, "");

        var info = await CheckAsync(config.UpdateManifestUrl.Trim(), ct);
        if (info is null || string.IsNullOrWhiteSpace(info.Url))
            return (false, "");

        var (started, msg) = await AgentUpdater.DownloadAndApplyAsync(info.Url, info.Version, ct);
        if (!started)
            return (false, msg);

        await Task.Delay(500, ct);
        Environment.Exit(0);
        return (true, msg);
    }

    internal sealed class UpdateInfo
    {
        public string Version { get; set; } = "";
        public string? Url { get; set; }
        public string? Notes { get; set; }
    }
}
